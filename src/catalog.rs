use crate::{
    AppState,
    auth::{Auth, id},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{OptionalExtension, named_params, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Library {
    pub item_id: String,
    pub name: String,
    pub collection_type: String,
    pub locations: Vec<String>,
    pub is_remote: bool,
    pub remote_server_name: Option<String>,
    pub is_object_store: bool,
}
pub async fn libraries(auth: Auth, State(state): State<AppState>) -> Result<Json<Vec<Library>>> {
    let admin = auth.user.policy.is_administrator;
    state
        .db
        .call(move |c| {
            let mut s = c.prepare(
                "SELECT l.id,l.name,l.kind,l.path,l.remote_server_id,
                        COALESCE(s.name,o.name),l.object_store_id IS NOT NULL
                 FROM libraries l LEFT JOIN remote_servers s ON s.id=l.remote_server_id
                 LEFT JOIN object_stores o ON o.id=l.object_store_id
                 ORDER BY l.name,l.id",
            )?;
            let rows = s
                .query_map([], |r| {
                    Ok(Library {
                        item_id: r.get(0)?,
                        name: r.get(1)?,
                        collection_type: r.get(2)?,
                        locations: if admin
                            && r.get::<_, Option<String>>(4)?.is_none()
                            && !r.get::<_, bool>(6)?
                        {
                            vec![r.get(3)?]
                        } else {
                            vec![]
                        },
                        is_remote: r.get::<_, Option<String>>(4)?.is_some()
                            || r.get::<_, bool>(6)?,
                        remote_server_name: r.get(5)?,
                        is_object_store: r.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(Json(rows))
        })
        .await
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewLibrary {
    pub name: String,
    pub collection_type: String,
    pub locations: Vec<String>,
}
pub async fn create_library(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<NewLibrary>,
) -> Result<(StatusCode, Json<Value>)> {
    auth.admin()?;
    if input.name.trim().is_empty() || input.name.len() > 128 || input.locations.len() != 1 {
        return Err(Error::bad(
            "Provide a name and exactly one existing library directory",
        ));
    }
    if !["movies", "tvshows", "music", "homevideos"].contains(&input.collection_type.as_str()) {
        return Err(Error::bad("Unsupported collection type"));
    }
    let path = tokio::fs::canonicalize(&input.locations[0])
        .await
        .map_err(|_| Error::bad("Library directory does not exist"))?;
    let metadata = tokio::fs::metadata(&path).await?;
    let root_identity = crate::scanner::root_identity(&metadata);
    if !metadata.is_dir() {
        return Err(Error::bad("Library location must be a directory"));
    }
    let path = path
        .to_str()
        .ok_or_else(|| Error::bad("Library path must be UTF-8"))?
        .to_owned();
    state
        .db
        .call(move |c| {
            // Overlapping roots would otherwise make ownership and pruning ambiguous.
            let mut s = c.prepare("SELECT path FROM libraries WHERE remote_server_id IS NULL")?;
            let roots = s
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if roots.iter().any(|root| {
                std::path::Path::new(root).starts_with(&path)
                    || std::path::Path::new(&path).starts_with(root)
            }) {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "Library directories must not overlap".into(),
                ));
            }
            let library_id = id();
            c.execute(
                "INSERT INTO libraries(id,name,path,kind,root_identity) VALUES (?1,?2,?3,?4,?5)",
                params![
                    library_id,
                    input.name.trim(),
                    path,
                    input.collection_type,
                    root_identity
                ],
            )?;
            Ok((StatusCode::CREATED, Json(json!({"Id":library_id}))))
        })
        .await
}
pub async fn delete_library(
    auth: Auth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    auth.admin()?;
    let _permit = state.scan_gate.clone().try_acquire_owned().map_err(|_| {
        Error(
            StatusCode::CONFLICT,
            "Wait for the library scan to finish".into(),
        )
    })?;
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM libraries WHERE id=?1", [id])? == 0 {
                return Err(Error::missing());
            }
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PathsQuery {
    pub path: Option<String>,
}
/// Admin-only filesystem navigation for the frontend media-folder picker.
/// Lists the subdirectories of a server-side path (default no path = the
/// server's current working directory, like other relative library roots).
/// Only directory names and paths are disclosed to the trusted administrator.
pub async fn directories(auth: Auth, Query(query): Query<PathsQuery>) -> Result<Json<Value>> {
    auth.admin()?;
    let requested = query
        .path
        .filter(|path| !path.trim().is_empty())
        .unwrap_or_else(|| ".".to_owned());
    let base = tokio::fs::canonicalize(&requested)
        .await
        .map_err(|_| Error::bad("Directory does not exist"))?;
    let metadata = tokio::fs::metadata(&base)
        .await
        .map_err(|_| Error::bad("Directory does not exist"))?;
    if !metadata.is_dir() {
        return Err(Error::bad("Path is not a directory"));
    }
    let mut directory = tokio::fs::read_dir(&base)
        .await
        .map_err(|_| Error::bad("Cannot read directory"))?;
    let mut children: Vec<(String, String)> = vec![];
    while let Some(entry) = directory
        .next_entry()
        .await
        .map_err(|_| Error::bad("Cannot read directory"))?
    {
        let path = entry.path();
        if !tokio::fs::metadata(&path)
            .await
            .map_err(Error::internal)?
            .is_dir()
        {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let Some(path) = path.to_str().map(str::to_owned) else {
            continue;
        };
        children.push((name, path));
        if children.len() > 2000 {
            return Err(Error::bad("Too many subdirectories"));
        }
    }
    children.sort_by(|a, b| a.0.cmp(&b.0));
    let parent = base.parent().and_then(|parent| parent.to_str());
    Ok(Json(json!({
        "Path": base.to_str(),
        "Parent": parent,
        "Directories": children
            .iter()
            .map(|(name, path)| json!({"Name": name, "Path": path}))
            .collect::<Vec<Value>>(),
    })))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ItemQuery {
    pub user_id: Option<String>,
    pub recursive: Option<bool>,
    pub parent_id: Option<String>,
    pub search_term: Option<String>,
    pub include_item_types: Option<String>,
    pub is_favorite: Option<bool>,
    pub is_played: Option<bool>,
    pub start_index: Option<u32>,
    pub limit: Option<u32>,
    pub sort_by: Option<String>,
}
// Keep one representation for listing, detail, and playlist items.
pub const ITEM_COLUMNS: &str = "i.id,i.name,i.kind,i.library_id,i.container,i.size,i.runtime_ticks,i.media_streams,COALESCE(u.position_ticks,0),COALESCE(u.played,0),COALESCE(u.favorite,0),i.tmdb_id,i.year,i.overview,i.genres,i.rating,i.parent_id,i.index_number,i.parent_index_number,(SELECT COUNT(*) FROM items child WHERE child.parent_id=i.id),CASE WHEN i.kind='Series' THEN i.id WHEN i.kind='Season' THEN i.parent_id ELSE (SELECT parent_id FROM items season WHERE season.id=i.parent_id) END,(SELECT name FROM items series WHERE series.id=CASE WHEN i.kind='Series' THEN i.id WHEN i.kind='Season' THEN i.parent_id ELSE (SELECT parent_id FROM items season WHERE season.id=i.parent_id) END),(SELECT tmdb_id FROM items series WHERE series.id=CASE WHEN i.kind='Series' THEN i.id WHEN i.kind='Season' THEN i.parent_id ELSE (SELECT parent_id FROM items season WHERE season.id=i.parent_id) END),i.remote_server_id IS NOT NULL";
pub fn item_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let kind: String = row.get(2)?;
    Ok(
        json!({"Id":row.get::<_,String>(0)?,"Name":row.get::<_,String>(1)?,"Type":kind,
        "MediaType":if kind=="Audio" {"Audio"} else {"Video"},"IsFolder":matches!(kind.as_str(),"Series"|"Season"),"IsRemote":row.get::<_,bool>(23)?,
        "LibraryId":row.get::<_,String>(3)?,"ParentId":row.get::<_,Option<String>>(16)?.or(Some(row.get::<_,String>(3)?)),"IndexNumber":row.get::<_,Option<i64>>(17)?,"ParentIndexNumber":row.get::<_,Option<i64>>(18)?,"ChildCount":row.get::<_,i64>(19)?,"SeriesId":row.get::<_,Option<String>>(20)?,"SeriesName":row.get::<_,Option<String>>(21)?,"SeriesTmdbId":row.get::<_,Option<String>>(22)?,"Container":row.get::<_,String>(4)?,"Size":row.get::<_,i64>(5)?,
        "RunTimeTicks":row.get::<_,Option<i64>>(6)?,
        "MediaStreams":serde_json::from_str::<Value>(&row.get::<_,String>(7)?).unwrap_or(json!([])),
        "UserData":{"PlaybackPositionTicks":row.get::<_,i64>(8)?,"Played":row.get::<_,bool>(9)?,"IsFavorite":row.get::<_,bool>(10)?},
        "TmdbId":row.get::<_,Option<String>>(11)?,"Year":row.get::<_,Option<i64>>(12)?,
        "Overview":row.get::<_,Option<String>>(13)?,
        "Genres":serde_json::from_str::<Value>(&row.get::<_,String>(14)?).unwrap_or(json!([])),
        "CommunityRating":row.get::<_,Option<f64>>(15)?}),
    )
}
pub async fn query_items(
    auth: Auth,
    state: AppState,
    query: ItemQuery,
    resume: bool,
) -> Result<Json<Value>> {
    if let Some(ref user) = query.user_id {
        auth.own(user)?;
    }
    if query.search_term.as_ref().is_some_and(|s| s.len() > 256) {
        return Err(Error::bad("Search term too long"));
    }
    let limit = query.limit.unwrap_or(100).min(200);
    let offset = query.start_index.unwrap_or(0);
    state.db.call(move |c| {
        let filter="FROM items i LEFT JOIN user_data u ON u.item_id=i.id AND u.user_id=:user
            WHERE (:parent IS NULL OR i.parent_id=:parent OR (i.library_id=:parent AND (:recursive=1 OR i.parent_id IS NULL)))
            AND (:search IS NULL OR instr(lower(i.name),lower(:search))>0)
            AND (:types IS NULL OR instr(','||:types||',',','||i.kind||',')>0)
            AND (:favorite IS NULL OR COALESCE(u.favorite,0)=:favorite)
            AND (:played IS NULL OR COALESCE(u.played,0)=:played)
            AND (:resume=0 OR (u.position_ticks>0 AND u.played=0))";
        let args=named_params!{":user":auth.user.id,":parent":query.parent_id,":search":query.search_term,
            ":types":query.include_item_types,":favorite":query.is_favorite,":played":query.is_played,":resume":resume,":recursive":query.recursive.unwrap_or(false)};
        let total:i64=c.query_row(&format!("SELECT COUNT(*) {filter}"),args,|r|r.get(0))?;
        let order=if resume {"u.updated_at DESC,i.id"} else if query.sort_by.as_deref()==Some("DateCreated") {"i.modified DESC,i.name COLLATE NOCASE,i.id"} else {"COALESCE(i.parent_index_number,-1),COALESCE(i.index_number,-1),i.name COLLATE NOCASE,i.id"};
        // Pagination integers are parsed and bounded before interpolation. All user text is bound.
        let mut s=c.prepare(&format!("SELECT {ITEM_COLUMNS} {filter} ORDER BY {order} LIMIT {limit} OFFSET {offset}"))?;
        let rows=s.query_map(args,item_row)?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(Json(json!({"Items":rows,"TotalRecordCount":total,"StartIndex":offset})))
    }).await
}
pub async fn items(
    auth: Auth,
    State(state): State<AppState>,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Value>> {
    query_items(auth, state, query, false).await
}
pub async fn user_items(
    auth: Auth,
    State(state): State<AppState>,
    Path(user): Path<String>,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Value>> {
    auth.own(&user)?;
    query_items(auth, state, query, false).await
}
pub async fn resume(
    auth: Auth,
    State(state): State<AppState>,
    Path(user): Path<String>,
    Query(query): Query<ItemQuery>,
) -> Result<Json<Value>> {
    auth.own(&user)?;
    query_items(auth, state, query, true).await
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct RecommendationQuery {
    pub user_id: Option<String>,
    pub item_limit: Option<u32>,
}

#[derive(Default)]
struct TasteProfile {
    genres: HashMap<String, f64>,
    kinds: HashMap<String, f64>,
    year_total: f64,
    year_weight: f64,
}

fn recommendation_score(
    profile: &TasteProfile,
    kind: &str,
    genres: &[String],
    year: Option<i64>,
    rating: Option<f64>,
) -> f64 {
    let genre_score: f64 = genres
        .iter()
        .map(|genre| {
            profile
                .genres
                .get(&genre.to_lowercase())
                .copied()
                .unwrap_or(0.0)
        })
        .sum();
    let kind_score = profile.kinds.get(kind).copied().unwrap_or(0.0) * 0.35;
    let year_score = if profile.year_weight > 0.0 {
        year.map(|candidate| {
            let preferred = profile.year_total / profile.year_weight;
            (2.0 - ((candidate as f64 - preferred).abs() / 10.0)).max(0.0)
        })
        .unwrap_or(0.0)
    } else {
        0.0
    };
    genre_score + kind_score + year_score + rating.unwrap_or(0.0) * 0.2
}

/// Jellyfin-compatible personalized recommendation category. The profile is
/// derived from this server's play state, so it also works for imported media.
pub async fn recommendations(
    auth: Auth,
    State(state): State<AppState>,
    Query(query): Query<RecommendationQuery>,
) -> Result<Json<Value>> {
    if let Some(ref user) = query.user_id {
        auth.own(user)?;
    }
    let user_id = auth.user.id;
    let limit = query.item_limit.unwrap_or(10).clamp(1, 50) as usize;
    state
        .db
        .call(move |c| {
            let mut profile = TasteProfile::default();
            let mut baseline = None;
            let mut history = c.prepare(
                "SELECT i.name,
                CASE WHEN i.kind='Episode' THEN 'Series' ELSE i.kind END,
                CASE WHEN i.kind='Episode' THEN COALESCE(series.genres,i.genres) ELSE i.genres END,
                CASE WHEN i.kind='Episode' THEN COALESCE(series.year,i.year) ELSE i.year END,
                u.played,u.favorite,u.position_ticks
             FROM user_data u JOIN items i ON i.id=u.item_id
             LEFT JOIN items season ON season.id=i.parent_id AND i.kind='Episode'
             LEFT JOIN items series ON series.id=season.parent_id
             WHERE u.user_id=?1 AND (u.played=1 OR u.favorite=1 OR u.position_ticks>0)
             ORDER BY u.updated_at DESC LIMIT 100",
            )?;
            let history_rows = history
                .query_map([&user_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, bool>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            for (rank, (name, kind, genres_json, year, played, favorite, position)) in
                history_rows.iter().enumerate()
            {
                baseline.get_or_insert_with(|| name.clone());
                let action = if *favorite {
                    5.0
                } else if *played {
                    3.0
                } else if *position > 0 {
                    2.0
                } else {
                    0.0
                };
                let weight = action / (1.0 + rank as f64 * 0.08);
                *profile.kinds.entry(kind.clone()).or_default() += weight;
                if let Some(year) = year {
                    profile.year_total += *year as f64 * weight;
                    profile.year_weight += weight;
                }
                for genre in serde_json::from_str::<Vec<String>>(genres_json).unwrap_or_default() {
                    *profile.genres.entry(genre.to_lowercase()).or_default() += weight;
                }
            }

            let sql = format!(
                "SELECT {ITEM_COLUMNS},i.modified FROM items i
            LEFT JOIN user_data u ON u.item_id=i.id AND u.user_id=?1
            WHERE i.parent_id IS NULL AND i.kind IN ('Movie','Series','Audio','Video')
              AND COALESCE(u.played,0)=0 AND COALESCE(u.position_ticks,0)=0"
            );
            let mut statement = c.prepare(&sql)?;
            let mut candidates = statement
                .query_map([&user_id], |row| {
                    let item = item_row(row)?;
                    let kind = row.get::<_, String>(2)?;
                    let genres = serde_json::from_str::<Vec<String>>(&row.get::<_, String>(14)?)
                        .unwrap_or_default();
                    let score =
                        recommendation_score(&profile, &kind, &genres, row.get(12)?, row.get(15)?);
                    Ok((score, row.get::<_, i64>(23)?, item))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
            let items = candidates
                .into_iter()
                .take(limit)
                .map(|(_, _, item)| item)
                .collect::<Vec<_>>();
            let category = if baseline.is_some() {
                "SimilarToRecentlyPlayed"
            } else {
                "Popular"
            };
            Ok(Json(json!([{
                "BaselineItemName": baseline,
                "CategoryId": "personalized",
                "RecommendationType": category,
                "Items": items
            }])))
        })
        .await
}
pub async fn item(
    auth: Auth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state.db.call(move |c| {
        let sql=format!("SELECT {ITEM_COLUMNS} FROM items i LEFT JOIN user_data u ON u.item_id=i.id AND u.user_id=?1 WHERE i.id=?2");
        Ok(Json(c.query_row(&sql,params![auth.user.id,id],item_row).optional()?.ok_or_else(Error::missing)?))
    }).await
}
pub async fn user_item(
    auth: Auth,
    State(state): State<AppState>,
    Path((user, id)): Path<(String, String)>,
) -> Result<Json<Value>> {
    auth.own(&user)?;
    item(auth, State(state), Path(id)).await
}

pub async fn adjacent_episodes(
    _auth: Auth,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    state.db.call(move |c| {
        let series: String = c.query_row("SELECT season.parent_id FROM items episode JOIN items season ON season.id=episode.parent_id WHERE episode.id=?1 AND episode.kind='Episode'",[&id],|r|r.get(0)).optional()?.ok_or_else(Error::missing)?;
        let (previous,next) = c.query_row("WITH ordered AS (
            SELECT e.id,lag(e.id) OVER (ORDER BY e.parent_index_number,e.index_number,e.id) previous,
                lead(e.id) OVER (ORDER BY e.parent_index_number,e.index_number,e.id) next
            FROM items e JOIN items s ON s.id=e.parent_id WHERE s.parent_id=?1 AND e.kind='Episode')
            SELECT previous,next FROM ordered WHERE id=?2",params![series,id],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?)))?;
        Ok(Json(json!({"PreviousId":previous,"NextId":next})))
    }).await
}

#[cfg(test)]
mod tests {
    use super::{TasteProfile, recommendation_score};

    #[test]
    fn recommendations_prefer_the_users_genres_and_media_type() {
        let mut profile = TasteProfile::default();
        profile.genres.insert("science fiction".into(), 8.0);
        profile.kinds.insert("Movie".into(), 6.0);
        profile.year_total = 2020.0 * 6.0;
        profile.year_weight = 6.0;

        let matching = recommendation_score(
            &profile,
            "Movie",
            &["Science Fiction".into()],
            Some(2021),
            Some(7.0),
        );
        let unrelated = recommendation_score(
            &profile,
            "Series",
            &["Romance".into()],
            Some(1980),
            Some(9.0),
        );
        assert!(matching > unrelated);
    }
}

use crate::{
    AppState,
    auth::{Auth, id},
    catalog::{ITEM_COLUMNS, item_row},
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewPlaylist {
    pub name: String,
    #[serde(default)]
    pub ids: Vec<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct AddItems {
    pub ids: Vec<String>,
}
fn check_owner(c: &Connection, playlist: &str, user: &str) -> Result<()> {
    let owner: String = c
        .query_row(
            "SELECT user_id FROM playlists WHERE id=?1",
            [playlist],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(Error::missing)?;
    if owner != user {
        return Err(Error::missing());
    } // Private playlists do not expose existence to other users.
    Ok(())
}
fn insert_items(c: &Connection, playlist: &str, items: &[String]) -> Result<()> {
    if items.len() > 1000 {
        return Err(Error::bad("At most 1000 items can be added per request"));
    }
    let offset: i64 = c.query_row(
        "SELECT COALESCE(MAX(position)+1,0) FROM playlist_items WHERE playlist_id=?1",
        [playlist],
        |r| r.get(0),
    )?;
    for (index, item) in items.iter().enumerate() {
        if !c.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)",
            [item],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::bad("Playlist contains an unknown item"));
        }
        c.execute(
            "INSERT INTO playlist_items VALUES (?1,?2,?3)",
            params![playlist, item, offset + index as i64],
        )?;
    }
    Ok(())
}
pub async fn create(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<NewPlaylist>,
) -> Result<(StatusCode, Json<Value>)> {
    if input.name.trim().is_empty() || input.name.len() > 128 {
        return Err(Error::bad("Playlist name must be 1–128 bytes"));
    }
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let playlist = id();
            tx.execute(
                "INSERT INTO playlists VALUES (?1,?2,?3)",
                params![playlist, auth.user.id, input.name.trim()],
            )?;
            insert_items(&tx, &playlist, &input.ids)?;
            tx.commit()?;
            Ok((StatusCode::CREATED, Json(json!({"Id":playlist}))))
        })
        .await
}
pub async fn list(auth: Auth, State(state): State<AppState>) -> Result<Json<Value>> {
    state.db.call(move |c| {
        let mut s=c.prepare("SELECT p.id,p.name,COUNT(i.item_id) FROM playlists p LEFT JOIN playlist_items i ON i.playlist_id=p.id WHERE p.user_id=?1 GROUP BY p.id ORDER BY p.name,p.id")?;
        let items=s.query_map([auth.user.id],|r|Ok(json!({"Id":r.get::<_,String>(0)?,"Name":r.get::<_,String>(1)?,"Type":"Playlist","ChildCount":r.get::<_,i64>(2)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(Json(json!({"TotalRecordCount":items.len(),"Items":items})))
    }).await
}
pub async fn append(
    auth: Auth,
    State(state): State<AppState>,
    Path(playlist): Path<String>,
    Json(input): Json<AddItems>,
) -> Result<StatusCode> {
    state
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            check_owner(&tx, &playlist, &auth.user.id)?;
            insert_items(&tx, &playlist, &input.ids)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Page {
    pub start_index: Option<u32>,
    pub limit: Option<u32>,
}
pub async fn items(
    auth: Auth,
    State(state): State<AppState>,
    Path(playlist): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    state.db.call(move |c| {
        check_owner(c,&playlist,&auth.user.id)?;
        let total:i64=c.query_row("SELECT COUNT(*) FROM playlist_items WHERE playlist_id=?1",[&playlist],|r|r.get(0))?;
        let mut s=c.prepare(&format!("SELECT {ITEM_COLUMNS} FROM playlist_items p JOIN items i ON i.id=p.item_id
            LEFT JOIN user_data u ON u.item_id=i.id AND u.user_id=?1 WHERE p.playlist_id=?2 ORDER BY p.position LIMIT ?3 OFFSET ?4"))?;
        let items=s.query_map(params![auth.user.id,playlist,page.limit.unwrap_or(100).min(200),page.start_index.unwrap_or(0)],item_row)?.collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(Json(json!({"Items":items,"TotalRecordCount":total,"StartIndex":page.start_index.unwrap_or(0)})))
    }).await
}
pub async fn remove(
    auth: Auth,
    State(state): State<AppState>,
    Path(playlist): Path<String>,
) -> Result<StatusCode> {
    state
        .db
        .call(move |c| {
            check_owner(c, &playlist, &auth.user.id)?;
            c.execute("DELETE FROM playlists WHERE id=?1", [playlist])?;
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

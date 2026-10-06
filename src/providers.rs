use crate::{
    AppState,
    auth::Auth,
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

const INTRODB_API: &str = "https://api.theintrodb.org/v3";
const SEGMENT_CACHE_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Clone, Default)]
pub struct ProviderKeys {
    pub tmdb: Option<String>,
    pub introdb: Option<String>,
    pub opensubtitles: Option<String>,
}

impl ProviderKeys {
    pub async fn load(db: &crate::db::Database, tmdb_default: Option<String>) -> Result<Self> {
        let stored = db.call(|connection| {
            let mut statement = connection.prepare(
                "SELECT key,value FROM settings WHERE key IN ('provider.tmdb','provider.introdb','provider.opensubtitles')",
            )?;
            let values = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
                .collect::<std::result::Result<HashMap<_, _>, _>>()?;
            Ok(values)
        }).await?;
        let value = |name: &str, fallback: Option<String>| {
            stored
                .get(name)
                .cloned()
                .or(fallback)
                .map(|key| key.trim().to_owned())
                .filter(|key| !key.is_empty())
        };
        Ok(Self {
            tmdb: value("provider.tmdb", tmdb_default),
            introdb: value(
                "provider.introdb",
                std::env::var("JELLYMAX_INTRODB_API_KEY").ok(),
            ),
            opensubtitles: value(
                "provider.opensubtitles",
                std::env::var("JELLYMAX_OPENSUBTITLES_API_KEY").ok(),
            ),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
struct ProviderStatus {
    tmdb_configured: bool,
    intro_db_configured: bool,
    intro_db_public_lookups: bool,
    open_subtitles_configured: bool,
}

pub async fn get(auth: Auth, State(state): State<AppState>) -> Result<Json<Value>> {
    auth.admin()?;
    let keys = state.provider_keys.read().map_err(Error::internal)?;
    Ok(Json(json!(ProviderStatus {
        tmdb_configured: keys.tmdb.is_some(),
        intro_db_configured: keys.introdb.is_some(),
        intro_db_public_lookups: true,
        open_subtitles_configured: keys.opensubtitles.is_some(),
    })))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ProviderUpdate {
    tmdb_api_key: Option<String>,
    intro_db_api_key: Option<String>,
    open_subtitles_api_key: Option<String>,
}

fn clean(value: Option<String>) -> Result<Option<Option<String>>> {
    value
        .map(|value| {
            let value = value.trim();
            if value.len() > 1024 || value.contains(['\n', '\r', '\0']) {
                return Err(Error::bad("API keys must be at most 1024 characters"));
            }
            Ok((!value.is_empty()).then(|| value.to_owned()))
        })
        .transpose()
}

pub async fn update(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<ProviderUpdate>,
) -> Result<Json<Value>> {
    auth.admin()?;
    let tmdb = clean(input.tmdb_api_key)?;
    let introdb = clean(input.intro_db_api_key)?;
    let opensubtitles = clean(input.open_subtitles_api_key)?;
    if tmdb.is_none() && introdb.is_none() && opensubtitles.is_none() {
        return Err(Error::bad("No provider setting was supplied"));
    }
    if let Some(Some(ref key)) = tmdb {
        let response = state
            .http
            .get("https://api.themoviedb.org/3/configuration")
            .query(&[("api_key", key)])
            .send()
            .await
            .map_err(|_| Error(StatusCode::BAD_GATEWAY, "Could not reach TMDB".into()))?;
        if !response.status().is_success() {
            return Err(Error::bad("TMDB rejected this API key"));
        }
    }
    let changes = [
        ("provider.tmdb", tmdb.clone()),
        ("provider.introdb", introdb.clone()),
        ("provider.opensubtitles", opensubtitles.clone()),
    ];
    state.db.call(move |connection| {
        let transaction = connection.transaction()?;
        for (name, change) in changes {
            if let Some(value) = change {
                let value = value.unwrap_or_default();
                transaction.execute(
                    "INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![name, value],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }).await?;
    {
        let mut keys = state.provider_keys.write().map_err(Error::internal)?;
        if let Some(value) = tmdb {
            keys.tmdb = value;
        }
        if let Some(value) = introdb {
            keys.introdb = value;
        }
        if let Some(value) = opensubtitles {
            keys.opensubtitles = value;
        }
    }
    get(auth, State(state)).await
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
struct Segment {
    start_ticks: i64,
    end_ticks: i64,
}

fn intro_segments(body: &Value) -> Vec<Segment> {
    body.get("intro")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let start = entry
                .get("start_ms")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                .max(0);
            let end = entry.get("end_ms").and_then(Value::as_i64)?;
            (end > start).then_some(Segment {
                start_ticks: start.saturating_mul(10_000),
                end_ticks: end.saturating_mul(10_000),
            })
        })
        .collect()
}

fn cache_lifetime(value: &Value) -> i64 {
    if value
        .get("Intro")
        .and_then(Value::as_array)
        .is_some_and(|segments| !segments.is_empty())
    {
        SEGMENT_CACHE_SECONDS
    } else {
        // New server/plugin analysis and corrected community submissions should
        // become visible quickly. An empty result is not authoritative.
        5 * 60
    }
}

pub(crate) async fn store_segment_cache(
    state: &AppState,
    item_id: String,
    value: &Value,
    fetched_at: i64,
) -> Result<()> {
    let payload = serde_json::to_string(value).map_err(Error::internal)?;
    state
        .db
        .call(move |db| {
            db.execute(
                "INSERT INTO media_segments(item_id,payload,fetched_at) VALUES (?1,?2,?3)
                 ON CONFLICT(item_id) DO UPDATE SET payload=excluded.payload,fetched_at=excluded.fetched_at",
                params![item_id, payload, fetched_at],
            )?;
            Ok(())
        })
        .await
}

pub async fn segments(
    _auth: Auth,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
) -> Result<Json<Value>> {
    let lookup_id = item_id.clone();
    let metadata: (Option<String>, Option<i64>, Option<i64>, Option<i64>) = state
        .db
        .call(move |db| {
            db.query_row(
                "SELECT (SELECT tmdb_id FROM items series WHERE series.id=(SELECT parent_id FROM items season WHERE season.id=i.parent_id)),i.parent_index_number,i.index_number,i.runtime_ticks
                 FROM items i WHERE i.id=?1 AND i.kind='Episode'",
                [lookup_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?
            .ok_or_else(Error::missing)
        })
        .await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let cache_id = item_id.clone();
    let cached = state
        .db
        .call(move |db| {
            Ok(db
                .query_row(
                    "SELECT payload,fetched_at FROM media_segments WHERE item_id=?1",
                    [cache_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?)
        })
        .await?
        .and_then(|(payload, fetched_at)| {
            serde_json::from_str::<Value>(&payload)
                .ok()
                .map(|value| (value, fetched_at))
        });

    // Playback only reads markers prepared ahead of time. Connected Jellyfin
    // markers are cached by remote synchronization; this endpoint never waits
    // for the media server that is currently supplying the video stream.
    if let Some((value, fetched_at)) = cached.as_ref() {
        let has_markers = value
            .get("Intro")
            .and_then(Value::as_array)
            .is_some_and(|segments| !segments.is_empty());
        let jellyfin = value.get("Source").and_then(Value::as_str) == Some("Jellyfin");
        if has_markers && (jellyfin || now - fetched_at < cache_lifetime(value)) {
            return Ok(Json(value.clone()));
        }
    }

    let (Some(tmdb_id), Some(season), Some(episode), duration_ticks) = metadata else {
        return Ok(Json(json!({"Intro":[],"Source":"None"})));
    };
    if !tmdb_id.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(Json(json!({"Intro":[],"Source":"None"})));
    }
    let base = if cfg!(debug_assertions) {
        std::env::var("JELLYMAX_INTRODB_TEST_BASE").unwrap_or_else(|_| INTRODB_API.into())
    } else {
        INTRODB_API.into()
    };
    let mut request = state
        .http
        .get(format!("{base}/media"))
        .timeout(std::time::Duration::from_secs(4))
        .query(&[
            ("tmdb_id", tmdb_id),
            ("season", season.to_string()),
            ("episode", episode.to_string()),
            (
                "duration_ms",
                duration_ticks
                    .unwrap_or(0)
                    .saturating_div(10_000)
                    .to_string(),
            ),
        ]);
    if let Some(key) = state.provider_key("introdb") {
        request = request.bearer_auth(key);
    }
    let value = match request.send().await {
        Ok(response) if response.status() == reqwest::StatusCode::NOT_FOUND => {
            json!({"Intro":[],"Source":"TheIntroDB"})
        }
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
            Ok(body) => json!({"Intro":intro_segments(&body),"Source":"TheIntroDB"}),
            Err(error) => {
                tracing::warn!(%error, item=%item_id, "TheIntroDB returned an invalid response");
                json!({"Intro":[],"Source":"None"})
            }
        },
        Ok(response) => {
            tracing::warn!(status=%response.status(), item=%item_id, "TheIntroDB marker lookup failed");
            json!({"Intro":[],"Source":"None"})
        }
        Err(error) => {
            tracing::warn!(%error, item=%item_id, "TheIntroDB marker lookup unavailable");
            json!({"Intro":[],"Source":"None"})
        }
    };
    store_segment_cache(&state, item_id, &value, now).await?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_marker_results_expire_quickly() {
        assert_eq!(cache_lifetime(&json!({"Intro":[]})), 5 * 60);
        assert_eq!(
            cache_lifetime(&json!({"Intro":[{"StartTicks":0,"EndTicks":1}]})),
            SEGMENT_CACHE_SECONDS
        );
    }

    #[test]
    fn normalizes_introdb_ranges_and_ignores_invalid_ones() {
        let body = json!({"intro":[
            {"start_ms":null,"end_ms":23_000},
            {"start_ms":30_000,"end_ms":90_000},
            {"start_ms":50_000,"end_ms":40_000},
            {"start_ms":10_000,"end_ms":null}
        ]});
        assert_eq!(
            intro_segments(&body),
            vec![
                Segment {
                    start_ticks: 0,
                    end_ticks: 230_000_000
                },
                Segment {
                    start_ticks: 300_000_000,
                    end_ticks: 900_000_000
                },
            ]
        );
    }

    #[test]
    fn empty_key_removes_a_provider_and_keys_reject_line_breaks() {
        assert_eq!(clean(Some("  ".into())).unwrap(), Some(None));
        assert!(clean(Some("bad\nkey".into())).is_err());
    }
}

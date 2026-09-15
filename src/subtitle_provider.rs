//! Optional OpenSubtitles search/download. The API key remains on this server.
use crate::{
    AppState,
    auth::Auth,
    error::{Error, Result},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Value, json};

const API: &str = "https://api.opensubtitles.com/api/v1";
const USER_AGENT: &str = "JellyfinRust v0.1.0";
type SubtitleSearchItem = (
    String,
    String,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

fn api_base() -> String {
    if cfg!(debug_assertions)
        && let Ok(value) = std::env::var("JELLYMAX_OPENSUBTITLES_TEST_BASE")
    {
        return value;
    }
    API.into()
}

fn api_key() -> Result<String> {
    std::env::var("JELLYMAX_OPENSUBTITLES_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "OpenSubtitles is not configured on this server".into(),
            )
        })
}

async fn provider_error(stage: &str, response: reqwest::Response) -> Error {
    let status = response.status();
    let code = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::BAD_GATEWAY
    };
    let detail = response.json::<Value>().await.ok().and_then(|body| {
        body["message"]
            .as_str()
            .or_else(|| body["errors"][0].as_str())
            .filter(|message| message.len() <= 240 && !message.contains(['\n', '\r']))
            .map(str::to_owned)
    });
    let explanation = if status == reqwest::StatusCode::UNAUTHORIZED {
        match stage {
            "search" => "OpenSubtitles rejected the API key during search (401)",
            "download request" => {
                "OpenSubtitles rejected download authorization (401); check the API key and account download access"
            }
            _ => {
                "OpenSubtitles rejected the temporary subtitle file link (401); try downloading it again"
            }
        }
    } else {
        return Error(
            code,
            format!(
                "OpenSubtitles {stage} failed ({status}){}",
                detail.map(|text| format!(": {text}")).unwrap_or_default()
            ),
        );
    };
    Error(
        code,
        format!(
            "{explanation}{}",
            detail.map(|text| format!(": {text}")).unwrap_or_default()
        ),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SearchQuery {
    language: Option<String>,
}

pub async fn search(
    _auth: Auth,
    State(state): State<AppState>,
    Path(item): Path<String>,
    Query(input): Query<SearchQuery>,
) -> Result<Json<Value>> {
    let key = api_key()?;
    let language = input
        .language
        .unwrap_or_else(|| "en".into())
        .to_ascii_lowercase();
    if language.len() < 2
        || language.len() > 3
        || !language.bytes().all(|byte| byte.is_ascii_lowercase())
    {
        return Err(Error::bad("Invalid subtitle language"));
    }
    let (name, kind, tmdb, year, series_name, season, episode): SubtitleSearchItem = state.db.call(move |db| {
        db.query_row("SELECT i.name,i.kind,i.tmdb_id,i.year,
            (SELECT s.name FROM items s WHERE s.id=(SELECT season.parent_id FROM items season WHERE season.id=i.parent_id)),
            i.parent_index_number,i.index_number FROM items i WHERE i.id=?1", [item], |row|
            Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)))
            .optional()?.ok_or_else(Error::missing)
    }).await?;
    let mut url =
        reqwest::Url::parse(&format!("{}/subtitles", api_base())).map_err(Error::internal)?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("languages", &language);
        if kind == "Movie"
            && tmdb
                .as_deref()
                .is_some_and(|id| id.bytes().all(|b| b.is_ascii_digit()))
        {
            query.append_pair("tmdb_id", tmdb.as_deref().unwrap_or(""));
        } else {
            let title = if kind == "Episode" {
                series_name.as_deref().unwrap_or(&name)
            } else {
                &name
            };
            query.append_pair("query", title);
            if kind == "Episode" {
                if let Some(number) = season {
                    query.append_pair("season_number", &number.to_string());
                }
                if let Some(number) = episode {
                    query.append_pair("episode_number", &number.to_string());
                }
            } else if let Some(year) = year {
                query.append_pair("year", &year.to_string());
            }
        }
    }
    let response = state
        .http
        .get(url)
        .header("Api-Key", key)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|error| Error::internal(format!("OpenSubtitles search connection: {error}")))?;
    if !response.status().is_success() {
        return Err(provider_error("search", response).await);
    }
    let data: Value = response.json().await.map_err(Error::internal)?;
    let results: Vec<Value> = data["data"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|entry| {
            let attributes = &entry["attributes"];
            let language = attributes["language"].as_str().unwrap_or("und");
            let downloads = attributes["download_count"].as_u64().unwrap_or(0);
            let hearing_impaired = attributes["hearing_impaired"].as_bool().unwrap_or(false);
            attributes["files"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(move |file| {
                    let id = file["file_id"].as_u64()?;
                    let name = file["file_name"].as_str().unwrap_or("Subtitle");
                    Some(json!({"FileId":id,"FileName":name,"Language":language,
                "DownloadCount":downloads,"HearingImpaired":hearing_impaired}))
                })
        })
        .take(50)
        .collect();
    Ok(Json(json!({"Results":results})))
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DownloadInput {
    file_id: u64,
}

pub async fn download(
    _auth: Auth,
    State(state): State<AppState>,
    Path(item): Path<String>,
    Json(input): Json<DownloadInput>,
) -> Result<Json<Value>> {
    let key = api_key()?;
    if input.file_id == 0 {
        return Err(Error::bad("Invalid subtitle file"));
    }
    let exists: bool = state
        .db
        .call(move |db| {
            Ok(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)",
                [item],
                |row| row.get(0),
            )?)
        })
        .await?;
    if !exists {
        return Err(Error::missing());
    }
    let response = state
        .http
        .post(format!("{}/download", api_base()))
        .header("Api-Key", key)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .json(&json!({"file_id":input.file_id,"sub_format":"srt"}))
        .send()
        .await
        .map_err(Error::internal)?;
    if !response.status().is_success() {
        return Err(provider_error("download request", response).await);
    }
    let data: Value = response.json().await.map_err(Error::internal)?;
    let link = data["link"]
        .as_str()
        .ok_or_else(|| Error::internal("OpenSubtitles returned no download link"))?;
    let url = reqwest::Url::parse(link).map_err(Error::internal)?;
    let host = url.host_str().unwrap_or("");
    let test_origin = reqwest::Url::parse(&api_base()).ok();
    let trusted_test_url = cfg!(debug_assertions)
        && test_origin
            .as_ref()
            .is_some_and(|base| base.origin() == url.origin())
        && std::env::var("JELLYMAX_OPENSUBTITLES_TEST_BASE").is_ok();
    if !trusted_test_url
        && (url.scheme() != "https"
            || !(host == "opensubtitles.com" || host.ends_with(".opensubtitles.com")))
    {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "OpenSubtitles returned an unsafe download link".into(),
        ));
    }
    let response = state
        .http
        .get(url)
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(Error::internal)?;
    if !response.status().is_success() {
        return Err(provider_error("subtitle file", response).await);
    }
    if response
        .content_length()
        .is_some_and(|length| length > 2_000_000)
    {
        return Err(Error::bad("Subtitle file is too large"));
    }
    let bytes = response.bytes().await.map_err(Error::internal)?;
    if bytes.len() > 2_000_000 {
        return Err(Error::bad("Subtitle file is too large"));
    }
    let content =
        String::from_utf8(bytes.to_vec()).map_err(|_| Error::bad("Subtitle text is not UTF-8"))?;
    Ok(Json(json!({"Content":content,"Format":"srt"})))
}

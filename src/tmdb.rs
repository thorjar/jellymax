//! TMDb-powered metadata enrichment for movies.
//!
//! Requests currently use the system `curl` executable, which must be installed. Requests are bounded by
//! timeouts and output size, and every failure is non-fatal: the scan records
//! a metadata failure and continues. Nothing contacts TMDb unless an API key
//! was configured at startup.

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
use serde::Deserialize;
use serde_json::{Value, json};
use std::{path::Path as FsPath, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, sync::Mutex};

const CURL: &str = "curl";
const MAX_OUTPUT: u64 = 1024 * 1024;
const CURL_TIMEOUT_SECS: u64 = 12;
/// Keeps a full library scan comfortably below TMDb's published request limits.
const ENRICHMENT_DELAY: Duration = Duration::from_millis(250);

/// Server-side TMDb settings, supplied by CLI flags or environment variables.
#[derive(Clone, Debug)]
pub struct TmdbConfig {
    pub api_key: Option<String>,
    pub language: String,
    pub api_base: String,
    pub image_base: String,
}
impl Default for TmdbConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            language: "en-US".into(),
            api_base: "https://api.themoviedb.org/3".into(),
            image_base: "https://image.tmdb.org/t/p/w500".into(),
        }
    }
}
impl TmdbConfig {
    pub fn enabled(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
    }
}

/// One TMDb search result, normalised for `crate::scanner`.
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub id: i64,
    pub title: String,
    pub release_date: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub rating: Option<f64>,
    pub genre_ids: Vec<i64>,
}
impl SearchHit {
    fn release_year(&self) -> Option<i64> {
        self.release_date
            .as_deref()
            .and_then(|date| date.get(..4))
            .and_then(|year| year.parse().ok())
    }
}

/// Query TMDb for a movie. `Ok(vec![])` means "no match"; `Err` means the
/// request itself failed (network, non-2xx, malformed body) so callers can
/// tell the difference.
pub async fn search(state: &AppState, query: &str, year: Option<i64>) -> Result<Vec<SearchHit>> {
    let Some(key) = state
        .tmdb
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    else {
        return Err(Error::internal("TMDb is not configured"));
    };
    let search_url = format!("{}/search/movie", state.tmdb.api_base);
    let mut url = format!(
        "{search_url}?api_key={}&language={}&include_adult=false&query={}",
        percent_encode(key),
        percent_encode(&state.tmdb.language),
        percent_encode(query),
    );
    if let Some(year) = year {
        url.push_str(&format!("&primary_release_year={year}"));
    }
    let Some(body) = curl_text(&url).await else {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "TMDb search request failed".into(),
        ));
    };
    parse_search(&body).ok_or_else(|| {
        Error(
            StatusCode::BAD_GATEWAY,
            "TMDb search response was malformed".into(),
        )
    })
}

/// Genre id→name map, cached per (process, language) so a scan makes a single
/// genre request no matter how large the library is. Failures degrade to an
/// empty map because genres are decorative, never structural.
pub async fn genre_map(state: &AppState) -> GenreMap {
    let language = state.tmdb.language.clone();
    if let Some((cached_language, genres)) = GENRE_CACHE.lock().await.as_ref()
        && *cached_language == language
    {
        return genres.clone();
    }
    let Some(key) = state.tmdb.api_key.as_deref() else {
        return vec![];
    };
    let genre_url = format!("{}/genre/movie/list", state.tmdb.api_base);
    let url = format!(
        "{genre_url}?api_key={}&language={}",
        percent_encode(key),
        percent_encode(&language),
    );
    let Some(body) = curl_text(&url).await else {
        tracing::warn!("TMDb genre list request failed; genres will be empty");
        return vec![];
    };
    let Some(genres) = parse_genres(&body) else {
        tracing::warn!("TMDb genre list response was malformed; genres will be empty");
        return vec![];
    };
    *GENRE_CACHE.lock().await = Some((language, genres.clone()));
    genres
}

/// Genre id→name pairs.
type GenreMap = Vec<(i64, String)>;

static GENRE_CACHE: Mutex<Option<(String, GenreMap)>> = Mutex::const_new(None);

/// Download a TMDb poster into the server data directory (never into media
/// folders). Returns true only when the file exists afterwards; failures are
/// non-fatal because the image endpoint falls back to sidecar artwork.
pub async fn download_poster(state: &AppState, poster_path: &str, destination: &FsPath) -> bool {
    if !poster_path.starts_with('/') || poster_path.contains("..") {
        return false;
    }
    if let Some(parent) = destination.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return false;
    }
    let url = format!("{}{poster_path}", state.tmdb.image_base);
    let temporary = destination.with_extension(format!("{}.tmp", crate::auth::id()));
    let Ok(mut child) = Command::new(CURL)
        .args([
            "-sSfL",
            "--max-time",
            "30",
            "--max-filesize",
            "5242880",
            "-o",
        ])
        .arg(&temporary)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return false;
    };
    let ok = matches!(child.wait().await, Ok(status) if status.success());
    if ok && tokio::fs::rename(&temporary, destination).await.is_ok() {
        return true;
    }
    tokio::fs::remove_file(&temporary).await.ok();
    false
}

/// Fetch a URL with the system curl binary. Mirrors `scanner::probe`:
/// bounded runtime, bounded output, non-zero exits and oversized bodies are
/// all treated as failures.
async fn curl_text(url: &str) -> Option<String> {
    let mut child = Command::new(CURL)
        .args(["-sSfL", "--max-time", &CURL_TIMEOUT_SECS.to_string()])
        .arg(url)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let stdout = child.stdout.take()?;
    let bytes = tokio::time::timeout(Duration::from_secs(CURL_TIMEOUT_SECS + 2), async move {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .await
            .ok()?;
        if bytes.len() as u64 > MAX_OUTPUT {
            return None;
        }
        if !child.wait().await.ok()?.success() {
            return None;
        }
        Some(bytes)
    })
    .await
    .ok()??;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Pick the most plausible TMDb match for a scanned file name. Exact title
/// matches and release-year agreement dominate the score.
fn best_match(hits: &[SearchHit], title: &str, year: Option<i64>) -> Option<SearchHit> {
    let lower_title = title.to_ascii_lowercase();
    fn normalize(value: &str) -> String {
        value
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    }
    hits.iter()
        .filter(|hit| normalize(&hit.title) == normalize(title))
        .max_by_key(|hit| {
            let mut score = 0i64;
            if hit.title.eq_ignore_ascii_case(title) {
                score += 1000;
            } else if hit.title.to_ascii_lowercase().contains(&lower_title) {
                score += 250;
            }
            if year.is_some() && hit.release_year() == year {
                score += 500;
            }
            score
                + (hit.rating.unwrap_or(0.0) * 10.0).round() as i64
                + i64::from(hit.poster_path.is_some()) * 5
                + i64::from(hit.overview.is_some()) * 5
        })
        .cloned()
}

/// Split names such as "The Matrix (1999)" or "Coco 2017" into a search
/// title and an optional release-year hint.
fn split_title_year(raw: &str) -> (String, Option<i64>) {
    let name = raw.trim();
    if let (Some(open), Some(close)) = (name.rfind('('), name.rfind(')'))
        && open < close
        && let Ok(year) = name[open + 1..close].trim().parse::<i64>()
        && (1870..=2100).contains(&year)
    {
        let title = name[..open].trim();
        if !title.is_empty() {
            return (title.to_owned(), Some(year));
        }
    }
    let tokens = name.split_whitespace().collect::<Vec<_>>();
    for (index, token) in tokens.iter().enumerate().skip(1) {
        if token.len() == 4
            && let Ok(year) = token.parse::<i64>()
            && (1870..=2100).contains(&year)
        {
            return (tokens[..index].join(" "), Some(year));
        }
    }
    (name.to_owned(), None)
}

/// Normalise a scanned file stem into a human search phrase.
fn clean_title(raw: &str) -> String {
    raw.replace(['.', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_release_token(token: &str) -> bool {
    let token = token
        .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '-')
        .to_ascii_lowercase();
    let compact = token.replace('-', "");
    let resolution = compact
        .strip_suffix(['p', 'i'])
        .is_some_and(|number| matches!(number.parse::<u32>(), Ok(480..=4320)));
    resolution
        || matches!(
            compact.as_str(),
            "4k" | "8k"
                | "uhd"
                | "hdr"
                | "hdr10"
                | "hdr10plus"
                | "dv"
                | "dolbyvision"
                | "x264"
                | "x265"
                | "h264"
                | "h265"
                | "hevc"
                | "av1"
                | "xvid"
                | "divx"
                | "8bit"
                | "10bit"
                | "12bit"
                | "bluray"
                | "brrip"
                | "bdrip"
                | "webrip"
                | "webdl"
                | "hdtv"
                | "dvdrip"
                | "remux"
                | "proper"
                | "repack"
                | "extended"
        )
        || [
            "aac", "ac3", "eac3", "ddp", "dts", "truehd", "atmos", "flac", "mp3",
        ]
        .iter()
        .any(|codec| compact.starts_with(codec))
}

fn strip_release_suffix(raw: &str) -> String {
    let mut title = Vec::new();
    for token in raw.split_whitespace() {
        if !title.is_empty() && (token.starts_with('[') || is_release_token(token)) {
            break;
        }
        title.push(token);
    }
    title
        .join(" ")
        .trim_matches(|character: char| character.is_whitespace() || "-–—[({".contains(character))
        .to_owned()
}

/// Convert a media release name into the title and optional year sent to TMDb.
fn media_title(raw: &str) -> (String, Option<i64>) {
    let cleaned = clean_title(raw);
    let (title, year) = split_title_year(&cleaned);
    let stripped = strip_release_suffix(&title);
    (if stripped.is_empty() { title } else { stripped }, year)
}

/// Parsed TV episode file name: show title plus season/episode numbers.
#[derive(Clone, Debug, PartialEq)]
pub struct EpisodeRef {
    pub show: String,
    pub season: u32,
    pub episode: u32,
}

/// Extract "Show Name", season and episode from stems such as
/// "Breaking Bad S02E05", "Show - 1x03", or "Show S1E4 (2010) 720p".
pub(crate) fn parse_episode(raw: &str) -> Option<EpisodeRef> {
    let name = clean_title(raw);
    let chars: Vec<char> = name.chars().collect();
    let upper: Vec<char> = chars.iter().map(|c| c.to_ascii_uppercase()).collect();

    // A marker must start at a word boundary so show titles like
    // "Better Call Saul S03E01" don't trip on letters inside words.
    fn boundary(chars: &[char], index: usize) -> bool {
        index == 0 || !chars[index - 1].is_ascii_alphanumeric()
    }

    // "S01E02" / "S1E2": boundary + S + digits + E + digits + boundary.
    for index in 0..upper.len() {
        if upper[index] != 'S' || !boundary(&chars, index) {
            continue;
        }
        let mut cursor = index + 1;
        let season_start = cursor;
        while cursor < upper.len() && upper[cursor].is_ascii_digit() {
            cursor += 1;
        }
        let season_len = cursor - season_start;
        if season_len == 0 || season_len > 3 || cursor >= upper.len() || upper[cursor] != 'E' {
            continue;
        }
        cursor += 1;
        let episode_start = cursor;
        while cursor < upper.len() && upper[cursor].is_ascii_digit() {
            cursor += 1;
        }
        let episode_len = cursor - episode_start;
        if episode_len == 0 || episode_len > 3 {
            continue;
        }
        if cursor < chars.len() && chars[cursor].is_ascii_alphanumeric() {
            continue;
        }
        let show: String = chars[..index]
            .iter()
            .collect::<String>()
            .trim()
            .trim_end_matches(['-', '–'])
            .trim()
            .to_owned();
        if show.is_empty() {
            continue;
        }
        let season: String = upper[season_start..season_start + season_len]
            .iter()
            .collect();
        let episode: String = upper[episode_start..episode_start + episode_len]
            .iter()
            .collect();
        return Some(EpisodeRef {
            show,
            season: season.parse().ok()?,
            episode: episode.parse().ok()?,
        });
    }

    // "1x02" form: 1-2 digits + X + 1-3 digits + boundary. Digits may sit
    // directly before the X (they are the season), but letters may not, so
    // words like "mix" never match.
    for index in 0..upper.len() {
        if upper[index] != 'X' || (index > 0 && chars[index - 1].is_ascii_alphabetic()) {
            continue;
        }
        let season_end = index;
        let season_start = if season_end >= 2 && upper[season_end - 2].is_ascii_digit() {
            season_end - 2
        } else if season_end >= 1 && upper[season_end - 1].is_ascii_digit() {
            season_end - 1
        } else {
            continue;
        };
        let after = index + 1;
        let episode_start = after;
        let mut episode_end = episode_start;
        while episode_end < upper.len() && upper[episode_end].is_ascii_digit() {
            episode_end += 1;
        }
        let episode_len = episode_end - episode_start;
        if episode_len == 0 || episode_len > 3 {
            continue;
        }
        if episode_end < chars.len() && chars[episode_end].is_ascii_alphanumeric() {
            continue;
        }
        let show: String = chars[..season_start]
            .iter()
            .collect::<String>()
            .trim()
            .trim_end_matches(['-', '–'])
            .trim()
            .to_owned();
        if show.is_empty() {
            continue;
        }
        let season: String = upper[season_start..season_end].iter().collect();
        let episode: String = upper[episode_start..episode_end].iter().collect();
        return Some(EpisodeRef {
            show,
            season: season.parse().ok()?,
            episode: episode.parse().ok()?,
        });
    }
    None
}

/// Fetch TMDb data for one movie item and persist it (release year, overview,
/// genres, rating, TMDb id) plus its poster under the data directory. Missing
/// matches are `Ok(())` — only request failures become errors, which the scan
/// counts as metadata failures.
pub async fn enrich_movie(state: &AppState, item_id: &str, raw_name: &str) -> Result<()> {
    if !state.tmdb.enabled() {
        return Ok(());
    }
    tokio::time::sleep(ENRICHMENT_DELAY).await;
    match_and_apply(state, item_id, raw_name, None)
        .await
        .map(|_| ())
}

/// Query TMDb's TV show search endpoint. Reuses `SearchHit`/`parse_search`,
/// which normalise the TV field names (`name`, `first_air_date`).
pub async fn search_tv(state: &AppState, query: &str) -> Result<Vec<SearchHit>> {
    let Some(key) = state
        .tmdb
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
    else {
        return Err(Error::internal("TMDb is not configured"));
    };
    let search_url = format!("{}/search/tv", state.tmdb.api_base);
    let url = format!(
        "{search_url}?api_key={}&language={}&include_adult=false&query={}",
        percent_encode(key),
        percent_encode(&state.tmdb.language),
        percent_encode(query),
    );
    let Some(body) = curl_text(&url).await else {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "TMDb TV search request failed".into(),
        ));
    };
    parse_search(&body).ok_or_else(|| {
        Error(
            StatusCode::BAD_GATEWAY,
            "TMDb TV search response was malformed".into(),
        )
    })
}

async fn tv_details(state: &AppState, path: &str) -> Result<Value> {
    let key = state
        .tmdb
        .api_key
        .as_deref()
        .ok_or_else(|| Error::bad("TMDb is not configured"))?;
    let url = format!(
        "{}/tv/{}?api_key={}&language={}",
        state.tmdb.api_base,
        path,
        percent_encode(key),
        percent_encode(&state.tmdb.language)
    );
    let body = curl_text(&url).await.ok_or_else(|| {
        Error(
            StatusCode::BAD_GATEWAY,
            "TMDb details request failed".into(),
        )
    })?;
    let data: Value = serde_json::from_str(&body)
        .map_err(|_| Error(StatusCode::BAD_GATEWAY, "Malformed TMDb details".into()))?;
    if data["id"].as_i64().is_none() {
        return Err(Error(
            StatusCode::BAD_GATEWAY,
            "Missing TMDb identifier".into(),
        ));
    }
    Ok(data)
}

async fn store_tv(state: &AppState, item: &str, data: Value) -> Result<bool> {
    let poster = non_empty(&data["poster_path"]).or_else(|| non_empty(&data["still_path"]));
    let item_id = item.to_owned();
    let changed = state.db.call(move |c| {
        let genres = data["genres"].as_array().map(|a|a.iter().filter_map(|v|v["name"].as_str()).collect::<Vec<_>>());
        let year = data["first_air_date"].as_str().or_else(||data["air_date"].as_str()).and_then(|v|v.get(..4)).and_then(|v|v.parse::<i64>().ok());
        Ok(c.execute("UPDATE items SET tmdb_id=?2,name=COALESCE(?3,name),overview=?4,year=?5,rating=?6,genres=COALESCE(?7,genres) WHERE id=?1",
            params![item_id,data["id"].as_i64().map(|v|v.to_string()),non_empty(&data["name"]),non_empty(&data["overview"]),year,data["vote_average"].as_f64(),genres.map(|g|serde_json::to_string(&g).unwrap())])?)
    }).await?;
    if changed > 0
        && let Some(poster) = poster
    {
        download_poster(
            state,
            &poster,
            &state.data_dir.join("artwork").join(format!("{item}.jpg")),
        )
        .await;
    }
    Ok(changed > 0)
}
async fn provider_id(state: &AppState, item: &str) -> Result<Option<String>> {
    let item = item.to_owned();
    state
        .db
        .call(move |c| {
            Ok(
                c.query_row("SELECT tmdb_id FROM items WHERE id=?1", [item], |r| {
                    r.get(0)
                })?,
            )
        })
        .await
}
pub async fn enrich_series(state: &AppState, item: &str) -> Result<bool> {
    let name = state
        .db
        .call({
            let item = item.to_owned();
            move |c| {
                Ok(
                    c.query_row("SELECT name FROM items WHERE id=?1", [item], |r| {
                        r.get::<_, String>(0)
                    })?,
                )
            }
        })
        .await?;
    let provider = if let Some(provider) = provider_id(state, item).await? {
        provider
    } else {
        let (title, year) = media_title(&name);
        let hits = search_tv(state, &title).await?;
        let Some(hit) = best_match(&hits, &title, year) else {
            return Ok(false);
        };
        hit.id.to_string()
    };
    store_tv(state, item, tv_details(state, &provider).await?).await
}
pub async fn enrich_season(state: &AppState, item: &str) -> Result<bool> {
    let (series, number) = state
        .db
        .call({
            let item = item.to_owned();
            move |c| {
                Ok(c.query_row(
                    "SELECT parent_id,index_number FROM items WHERE id=?1",
                    [item],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)),
                )?)
            }
        })
        .await?;
    if provider_id(state, &series).await?.is_none() && !enrich_series(state, &series).await? {
        return Ok(false);
    }
    let provider = provider_id(state, &series)
        .await?
        .ok_or_else(Error::missing)?;
    store_tv(
        state,
        item,
        tv_details(state, &format!("{provider}/season/{number}")).await?,
    )
    .await
}
pub async fn enrich_tv_parents(state: &AppState, season: &str) -> Result<()> {
    if provider_id(state, season).await?.is_none() {
        enrich_season(state, season).await?;
    }
    Ok(())
}
pub async fn enrich_episode(state: &AppState, item: &str, _raw_name: &str) -> Result<bool> {
    let reference = state.db.call({let item=item.to_owned();move |c| Ok(c.query_row(
        "SELECT e.parent_id,e.index_number,e.parent_index_number,s.parent_id FROM items e JOIN items s ON s.id=e.parent_id WHERE e.id=?1",[item],
        |r|Ok((r.get::<_,String>(0)?,r.get::<_,u32>(1)?,r.get::<_,u32>(2)?,r.get::<_,String>(3)?))).optional()?)}).await?;
    let Some((season, episode, number, series)) = reference else {
        return Ok(false);
    };
    enrich_tv_parents(state, &season).await?;
    let Some(provider) = provider_id(state, &series).await? else {
        return Ok(false);
    };
    let data = tv_details(
        state,
        &format!("{provider}/season/{number}/episode/{episode}"),
    )
    .await?;
    store_tv(state, item, data).await
}

/// Shared implementation for scan enrichment and the manual refresh endpoint.
/// `year_override` comes from an explicit admin request. `Ok(true)` means a
/// TMDb match was found and stored.
async fn match_and_apply(
    state: &AppState,
    item_id: &str,
    raw_name: &str,
    year_override: Option<i64>,
) -> Result<bool> {
    let (title, filename_year) = media_title(raw_name);
    let year = year_override.or(filename_year);
    let mut hits = search(state, &title, year).await?;
    if hits.is_empty() && year.is_some() {
        // Filenames can produce bogus year hints; retry without the hint
        // before declaring there is no match.
        hits = search(state, &title, None).await?;
    }
    let Some(hit) = best_match(&hits, &title, year) else {
        tracing::info!(item = %item_id, title = %title, "No TMDb match found");
        return Ok(false);
    };
    let genre_names = genre_map(state)
        .await
        .into_iter()
        .filter(|(genre_id, _)| hit.genre_ids.contains(genre_id))
        .map(|(_, name)| name)
        .collect::<Vec<_>>();
    let genres = serde_json::to_string(&genre_names).unwrap_or_else(|_| "[]".to_owned());
    let year = hit.release_year().or(year_override).or(filename_year);
    let tmdb_id = hit.id.to_string();
    let overview = hit.overview.clone();
    let rating = hit.rating;
    let title = hit.title.clone();
    let stored = state
        .db
        .call({
            let item_id = item_id.to_owned();
            move |c| {
                Ok(c.execute(
                    "UPDATE items SET tmdb_id=?2, year=?3, overview=?4, genres=?5, rating=?6,name=?7
                     WHERE id=?1",
                    params![item_id, tmdb_id, year, overview, genres, rating, title],
                )?)
            }
        })
        .await?;
    if stored == 0 {
        // The item vanished mid-scan; there is nothing to enrich.
        return Ok(false);
    }
    if let Some(poster) = hit.poster_path.as_deref() {
        let destination = state
            .data_dir
            .join("artwork")
            .join(format!("{item_id}.jpg"));
        if !download_poster(state, poster, &destination).await {
            tracing::warn!(item = %item_id, "TMDb poster download failed");
        }
    }
    tracing::info!(item = %item_id, tmdb = %hit.id, title = %hit.title, "TMDb metadata stored");
    Ok(true)
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct RefreshRequest {
    /// Optional replacement search phrase (e.g. the real title when the file
    /// name is unreadable).
    pub name: Option<String>,
    /// Optional release-year hint used to disambiguate the search.
    pub year: Option<i64>,
}

/// POST /Items/{id}/Metadata/Refresh — re-run the TMDb match for one movie on
/// demand. Admin-only. `{"Matched":false}` means TMDb had no plausible hit;
/// the caller should refetch the item to see any stored metadata.
pub async fn refresh(
    auth: Auth,
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    body: Option<Json<RefreshRequest>>,
) -> Result<Json<Value>> {
    auth.admin()?;
    if !state.tmdb.enabled() {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "TMDb is not configured. Start the server with --tmdb-api-key to enable metadata."
                .into(),
        ));
    }
    let input = body.map(|Json(input)| input).unwrap_or_default();
    let (name, kind): (String, String) = state
        .db
        .call({
            let item_id = item_id.clone();
            move |c| {
                c.query_row(
                    "SELECT CASE WHEN kind='Movie' THEN path ELSE name END,kind FROM items WHERE id=?1",
                    params![item_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?
                .ok_or_else(Error::missing)
            }
        })
        .await?;
    if !matches!(kind.as_str(), "Movie" | "Episode" | "Series" | "Season") {
        return Err(Error::bad(
            "This item does not support TMDb metadata refresh",
        ));
    }
    let name = if kind == "Movie" {
        FsPath::new(&name)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or(&name)
            .to_owned()
    } else {
        name
    };
    let search_name = input.name.as_deref().unwrap_or(&name);
    let matched = if kind == "Episode" {
        enrich_episode(&state, &item_id, search_name).await?
    } else if kind == "Series" {
        enrich_series(&state, &item_id).await?
    } else if kind == "Season" {
        enrich_season(&state, &item_id).await?
    } else {
        match_and_apply(&state, &item_id, search_name, input.year).await?
    };
    Ok(Json(json!({ "Matched": matched, "ItemId": item_id })))
}

/// Minimal RFC 3986 unreserved-character encoder for query parameters.
fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::<u8>::with_capacity(input.len());
    for byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(byte) {
            out.push(*byte);
        } else {
            out.push(b'%');
            out.push(HEX[(byte >> 4) as usize]);
            out.push(HEX[(byte & 0x0F) as usize]);
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn non_empty(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn parse_search(body: &str) -> Option<Vec<SearchHit>> {
    let data: Value = serde_json::from_str(body).ok()?;
    Some(
        data["results"]
            .as_array()?
            .iter()
            .filter_map(|result| {
                let id = result["id"].as_i64()?;
                // Movies use "title"/"release_date"; TV uses "name"/"first_air_date".
                let title = result["title"]
                    .as_str()
                    .or_else(|| result["name"].as_str())?;
                let release_date = result["release_date"]
                    .as_str()
                    .or_else(|| result["first_air_date"].as_str())
                    .map(str::to_owned)
                    .filter(|date| !date.is_empty());
                Some(SearchHit {
                    id,
                    title: title.to_owned(),
                    release_date,
                    overview: non_empty(&result["overview"]),
                    poster_path: non_empty(&result["poster_path"]),
                    rating: result["vote_average"]
                        .as_f64()
                        .filter(|rating| rating.is_finite() && *rating > 0.0),
                    genre_ids: result["genre_ids"]
                        .as_array()
                        .map(|genres| {
                            genres
                                .iter()
                                .filter_map(|genre| genre.as_i64())
                                .collect::<Vec<i64>>()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect(),
    )
}

fn parse_genres(body: &str) -> Option<GenreMap> {
    let data: Value = serde_json::from_str(body).ok()?;
    Some(
        data["genres"]
            .as_array()?
            .iter()
            .filter_map(|genre| {
                let id = genre["id"].as_i64()?;
                let name = genre["name"].as_str()?;
                Some((id, name.to_owned()))
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_titles_with_parenthesised_years() {
        assert_eq!(
            split_title_year("The Matrix (1999)"),
            ("The Matrix".to_owned(), Some(1999))
        );
        // A year inside parentheses wins even with technical noise after it.
        assert_eq!(
            split_title_year("Up (2009) 1080p"),
            ("Up".to_owned(), Some(2009))
        );
        assert_eq!(
            split_title_year("Coco 2017"),
            ("Coco".to_owned(), Some(2017))
        );
        assert_eq!(split_title_year("Alien"), ("Alien".to_owned(), None));
        assert_eq!(split_title_year("(1999)"), ("(1999)".to_owned(), None));
    }

    #[test]
    fn cleans_file_stems() {
        assert_eq!(
            clean_title("Big.Buck.Bunny.2008.1080p"),
            "Big Buck Bunny 2008 1080p"
        );
        assert_eq!(clean_title("my_movie-name"), "my movie-name");
        assert_eq!(clean_title("   spaced   out  "), "spaced out");
    }

    #[test]
    fn parses_release_style_movie_names() {
        assert_eq!(
            media_title("Fight Club (1999) [2160p x265 10bit]"),
            ("Fight Club".to_owned(), Some(1999))
        );
        assert_eq!(
            media_title("Hugo 2011 2160p 4K BluRay x265 10bit AAC5 1-[YTS MX]"),
            ("Hugo".to_owned(), Some(2011))
        );
        assert_eq!(
            media_title("Arrival.2160p.UHD.BluRay.x265.10bit.DTS"),
            ("Arrival".to_owned(), None)
        );
        assert_eq!(
            media_title("Dune 2160p 2021 BluRay"),
            ("Dune".to_owned(), Some(2021))
        );
    }

    #[test]
    fn percent_encoding_preserves_query_safety() {
        assert_eq!(percent_encode("up (2009)"), "up%20%282009%29");
        assert_eq!(percent_encode("a&b=c/d?e"), "a%26b%3Dc%2Fd%3Fe");
        assert_eq!(percent_encode("Café"), "Caf%C3%A9");
        assert_eq!(percent_encode("safe-._~09"), "safe-._~09");
    }

    #[test]
    fn parses_search_payloads() {
        let body = r#"{"results":[
            {"id":27205,"title":"Inception","release_date":"2010-07-16",
             "overview":"Cobb steals secrets.","poster_path":"/9gk.jpg",
             "vote_average":8.4,"genre_ids":[28,878]},
            {"id":1,"title":"Broken","overview":"","vote_average":0}
        ]}"#;
        let hits = parse_search(body).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].id, 27205);
        assert_eq!(hits[0].release_year(), Some(2010));
        assert_eq!(hits[0].genre_ids, vec![28, 878]);
        assert_eq!(hits[0].rating, Some(8.4));
        assert!(hits[1].poster_path.is_none());
        assert!(hits[1].rating.is_none());
        assert!(parse_search("not json").is_none());
    }

    #[test]
    fn parses_genre_payloads() {
        let body = r#"{"genres":[{"id":28,"name":"Action"},{"id":878,"name":"Science Fiction"}]}"#;
        let genres = parse_genres(body).unwrap();
        assert_eq!(
            genres,
            vec![
                (28, "Action".to_owned()),
                (878, "Science Fiction".to_owned())
            ]
        );
        assert!(parse_genres("{}").is_none());
    }

    #[test]
    fn parses_tv_search_payloads() {
        let body = r#"{"results":[
            {"id":1396,"name":"Breaking Bad","first_air_date":"2008-01-20",
             "overview":"A teacher cooks.","poster_path":"/gg.jpg","vote_average":8.9,
             "genre_ids":[18]},
            {"id":60059,"name":"Better Call Saul","first_air_date":"2015-02-08",
             "overview":"Lawyer show.","poster_path":null,"vote_average":8.7}
        ]}"#;
        let hits = parse_search(body).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].title, "Breaking Bad");
        assert_eq!(hits[0].release_year(), Some(2008));
    }

    #[test]
    fn parses_common_episode_file_names() {
        assert_eq!(
            parse_episode("Breaking Bad S02E05 720p"),
            Some(EpisodeRef {
                show: "Breaking Bad".into(),
                season: 2,
                episode: 5
            })
        );
        assert_eq!(
            parse_episode("Show - 1x03 - Title"),
            Some(EpisodeRef {
                show: "Show".into(),
                season: 1,
                episode: 3
            })
        );
        assert_eq!(
            parse_episode("Better Call Saul S03E01"),
            Some(EpisodeRef {
                show: "Better Call Saul".into(),
                season: 3,
                episode: 1
            })
        );
        // Titles containing letters that merely look like markers must not
        // split mid-word, and no-marker names return None.
        assert_eq!(parse_episode("Scenes from a Marriage"), None);
        assert_eq!(parse_episode("Some movie without a marker"), None);
    }

    #[test]
    fn prefers_exact_title_and_year_matches() {
        let hits = vec![
            SearchHit {
                id: 1,
                title: "Up".into(),
                release_date: Some("1934-01-01".into()),
                overview: None,
                poster_path: None,
                rating: Some(5.0),
                genre_ids: vec![],
            },
            SearchHit {
                id: 2,
                title: "Up".into(),
                release_date: Some("2009-05-29".into()),
                overview: Some("Carl".into()),
                poster_path: Some("/up.jpg".into()),
                rating: Some(7.8),
                genre_ids: vec![],
            },
        ];
        assert_eq!(best_match(&hits, "Up", Some(2009)).unwrap().id, 2);
        assert_eq!(best_match(&hits, "up", None).unwrap().id, 2);
        assert!(best_match(&[], "Up", Some(2009)).is_none());
    }
}

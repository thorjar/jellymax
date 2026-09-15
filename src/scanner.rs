use crate::{
    AppState,
    auth::{Auth, id},
    error::{Error, Result},
};
use axum::{Json, extract::State, http::StatusCode};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};
use tokio::{process::Command, sync::mpsc};
use walkdir::WalkDir;

#[derive(Clone, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ScanStatus {
    pub id: String,
    pub state: String,
    pub scanned: u64,
    pub removed: u64,
    pub probe_failures: u64,
    pub metadata_failures: u64,
    /// "TMDb" when a TMDb API key is configured, otherwise "Disabled".
    pub metadata_provider: String,
    pub errors: Vec<String>,
}
impl Default for ScanStatus {
    fn default() -> Self {
        Self {
            id: "library-scan".into(),
            state: "Idle".into(),
            scanned: 0,
            removed: 0,
            probe_failures: 0,
            metadata_failures: 0,
            metadata_provider: "Disabled".into(),
            errors: vec![],
        }
    }
}
pub async fn status(auth: Auth, State(state): State<AppState>) -> Result<Json<Vec<ScanStatus>>> {
    auth.admin()?;
    let mut status = state.scan_status.read().await.clone();
    status.metadata_provider = if state.tmdb.enabled() {
        "TMDb".into()
    } else {
        "Disabled".into()
    };
    Ok(Json(vec![status]))
}
pub async fn refresh(
    auth: Auth,
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<Value>)> {
    auth.admin()?;
    let permit = state
        .scan_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error(StatusCode::CONFLICT, "A scan is already running".into()))?;
    *state.scan_status.write().await = ScanStatus {
        state: "Running".into(),
        ..Default::default()
    };
    tokio::spawn(async move {
        let _permit = permit;
        let result = scan(&state).await;
        let mut status = state.scan_status.write().await;
        match result {
            Ok(()) => {
                status.state = if status.errors.is_empty() {
                    "Completed"
                } else {
                    "Failed"
                }
                .into()
            }
            Err(error) => {
                status.state = "Failed".into();
                status.errors.push(error.to_string());
            }
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({"Id":"library-scan"}))))
}
struct Candidate {
    path: String,
    name: String,
    container: String,
    size: i64,
    modified: i64,
}
fn media_extension(path: &std::path::Path, kind: &str) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let supported = if kind == "music" {
        [
            "mp3", "flac", "m4a", "aac", "ogg", "opus", "wav", "wma", "aiff",
        ]
        .contains(&extension.as_str())
    } else {
        [
            "mp4", "mkv", "avi", "mov", "webm", "m4v", "ts", "mpeg", "mpg", "wmv",
        ]
        .contains(&extension.as_str())
    };
    supported.then_some(extension)
}
async fn record_error(state: &AppState, message: String) {
    tracing::warn!(%message,"Library scan incomplete");
    let mut status = state.scan_status.write().await;
    if status.errors.len() < 20 {
        status.errors.push(message);
    }
}
async fn scan(state: &AppState) -> Result<()> {
    let libraries = state
        .db
        .call(|c| {
            let mut s =
                c.prepare("SELECT id,path,kind,root_identity FROM libraries WHERE remote_server_id IS NULL ORDER BY id")?;
            Ok(s.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .await?;
    for (library, root, kind, expected_identity) in libraries {
        let scan_id = id();
        let (sender, mut receiver) = mpsc::channel::<std::result::Result<Candidate, String>>(64);
        let walk_root = root.clone();
        let walk_kind = kind.clone();
        let walk_identity = expected_identity.clone();
        let walker = tokio::task::spawn_blocking(move || {
            let root = PathBuf::from(&walk_root);
            // A disappeared or replaced root must not erase the existing catalog.
            if !root.is_dir()
                || root.canonicalize().ok().as_ref() != Some(&root)
                || root.metadata().ok().as_ref().and_then(root_identity) != walk_identity
            {
                let _ = sender.blocking_send(Err(format!(
                    "Library root unavailable or changed: {walk_root}"
                )));
                return;
            }
            for entry in WalkDir::new(&root).follow_links(false) {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        if sender.blocking_send(Err(e.to_string())).is_err() {
                            break;
                        }
                        continue;
                    }
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                let Some(container) = media_extension(entry.path(), &walk_kind) else {
                    continue;
                };
                let candidate = (|| {
                    let path = entry.path().canonicalize().map_err(|e| e.to_string())?;
                    if !path.starts_with(&root) {
                        return Err("File escaped library root".into());
                    }
                    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
                    let modified = metadata
                        .modified()
                        .map_err(|e| e.to_string())?
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos()
                        .min(i64::MAX as u128) as i64;
                    Ok(Candidate {
                        path: path.to_str().ok_or("Non-UTF-8 media path")?.into(),
                        name: path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("Untitled")
                            .replace(['.', '_'], " "),
                        container,
                        size: metadata.len().min(i64::MAX as u64) as i64,
                        modified,
                    })
                })();
                if sender.blocking_send(candidate).is_err() {
                    break;
                }
            }
        });
        let mut complete = true;
        while let Some(candidate) = receiver.recv().await {
            let candidate = match candidate {
                Ok(c) => c,
                Err(e) => {
                    complete = false;
                    record_error(state, e).await;
                    continue;
                }
            };
            let path = candidate.path.clone();
            let previous = state
                .db
                .call(move |c| {
                    Ok(c.query_row(
                        "SELECT size,modified,runtime_ticks,media_streams,tmdb_id FROM items WHERE path=?1",
                        [path],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)?,
                                r.get::<_, Option<i64>>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, Option<String>>(4)?,
                            ))
                        },
                    )
                    .optional()?)
                })
                .await?;
            let (runtime, streams) = if let Some((size, modified, Some(runtime), streams, _)) =
                previous.as_ref()
                && size == &candidate.size
                && modified == &candidate.modified
            {
                (Some(*runtime), streams.clone())
            } else {
                match probe(&state.ffprobe, &candidate.path).await {
                    Some(result) => result,
                    None => {
                        state.scan_status.write().await.probe_failures += 1;
                        (None, "[]".into())
                    }
                }
            };
            let hierarchy = if kind == "tvshows" {
                ensure_hierarchy(state, &library, &root, &candidate.path, &scan_id).await?
            } else {
                None
            };
            let library = library.clone();
            let generation = scan_id.clone();
            let item_type = match kind.as_str() {
                "music" => "Audio",
                "tvshows" => "Episode",
                "movies" => "Movie",
                _ => "Video",
            };
            // Movies and TV episodes without TMDb data are enriched during the
            // scan when a key is configured. Items that already have data are
            // left untouched.
            let needs_metadata = matches!(item_type, "Movie" | "Episode")
                && state.tmdb.enabled()
                && !matches!(previous, Some((_, _, _, _, Some(_))));
            let (parent, season, episode) = hierarchy
                .clone()
                .map(|h| (Some(h.0), Some(h.1), Some(h.2)))
                .unwrap_or_default();
            let item_id = id();
            let insert_id = item_id.clone();
            let insert_path = candidate.path.clone();
            let insert_name = candidate.name.clone();
            let insert_container = candidate.container.clone();
            let item_id = state.db.call(move |c| {
                c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,parent_id,parent_index_number,index_number)
                    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
                    ON CONFLICT(path) DO UPDATE SET name=CASE WHEN items.tmdb_id IS NULL THEN excluded.name ELSE items.name END,kind=excluded.kind,container=excluded.container,
                    size=excluded.size,modified=excluded.modified,runtime_ticks=excluded.runtime_ticks,
                    media_streams=excluded.media_streams,scan_id=excluded.scan_id,parent_id=excluded.parent_id,parent_index_number=excluded.parent_index_number,index_number=excluded.index_number",
                    params![insert_id,library,insert_path,insert_name,item_type,insert_container,candidate.size,candidate.modified,runtime,streams,generation,parent,season,episode])?;
                Ok(c.query_row("SELECT id FROM items WHERE path=?1", [&insert_path], |r| r.get::<_,String>(0))?)
            }).await?;
            state.scan_status.write().await.scanned += 1;
            if state.tmdb.enabled()
                && let Some((ref season, _, _)) = hierarchy
                && let Err(error) = crate::tmdb::enrich_tv_parents(state, season).await
            {
                tracing::warn!(%error,"TV parent metadata enrichment failed");
                state.scan_status.write().await.metadata_failures += 1;
            }
            if needs_metadata {
                let result = if item_type == "Episode" {
                    crate::tmdb::enrich_episode(state, &item_id, &candidate.name).await
                } else {
                    crate::tmdb::enrich_movie(state, &item_id, &candidate.name)
                        .await
                        .map(|_| false)
                };
                match result {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(%error,"TMDb enrichment failed");
                        state.scan_status.write().await.metadata_failures += 1;
                    }
                }
            }
        }
        walker.await.map_err(Error::internal)?;
        if complete {
            let current_identity = tokio::fs::metadata(&root)
                .await
                .ok()
                .as_ref()
                .and_then(root_identity);
            if current_identity != expected_identity
                || tokio::fs::canonicalize(&root).await.ok().as_deref()
                    != Some(std::path::Path::new(&root))
            {
                record_error(state, format!("Library root changed during scan: {root}")).await;
                continue;
            }
            let cleanup_library = library.clone();
            let removed = state
                .db
                .call(move |c| {
                    Ok(c.execute(
                        "DELETE FROM items WHERE library_id=?1 AND scan_id<>?2",
                        params![cleanup_library, scan_id],
                    )?)
                })
                .await?;
            let duplicates = reconcile_local_duplicates(state, &library).await?;
            state.scan_status.write().await.removed += (removed + duplicates) as u64;
        }
    }
    Ok(())
}

/// Collapse alternate local files which TMDb identifies as the same movie or
/// episode. Files without a TMDb match remain separate because a guessed title
/// is not strong enough evidence to delete a catalog record.
async fn reconcile_local_duplicates(state: &AppState, library: &str) -> Result<usize> {
    let library = library.to_owned();
    state
        .db
        .call(move |connection| {
            let groups = {
                let mut statement = connection.prepare(
                    "SELECT kind,tmdb_id FROM items
                     WHERE library_id=?1 AND remote_server_id IS NULL
                       AND kind IN ('Movie','Episode') AND tmdb_id IS NOT NULL
                     GROUP BY kind,tmdb_id HAVING COUNT(*)>1",
                )?;
                statement
                    .query_map([&library], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            let transaction = connection.transaction()?;
            let mut removed = 0;
            for (kind, provider) in groups {
                let candidates = {
                    let mut statement = transaction.prepare(
                        "SELECT id FROM items
                         WHERE library_id=?1 AND kind=?2 AND tmdb_id=?3
                           AND remote_server_id IS NULL
                         ORDER BY runtime_ticks IS NOT NULL DESC,size DESC,modified DESC,id",
                    )?;
                    statement
                        .query_map(params![library, kind, provider], |row| {
                            row.get::<_, String>(0)
                        })?
                        .collect::<std::result::Result<Vec<_>, _>>()?
                };
                let Some(winner) = candidates.first() else {
                    continue;
                };
                for duplicate in candidates.iter().skip(1) {
                    transaction.execute(
                        "INSERT INTO user_data(user_id,item_id,position_ticks,played,favorite,updated_at)
                         SELECT user_id,?1,position_ticks,played,favorite,updated_at
                         FROM user_data WHERE item_id=?2
                         ON CONFLICT(user_id,item_id) DO UPDATE SET
                           position_ticks=MAX(position_ticks,excluded.position_ticks),
                           played=MAX(played,excluded.played),
                           favorite=MAX(favorite,excluded.favorite),
                           updated_at=MAX(updated_at,excluded.updated_at)",
                        params![winner, duplicate],
                    )?;
                    transaction.execute(
                        "UPDATE playlist_items SET item_id=?1 WHERE item_id=?2",
                        params![winner, duplicate],
                    )?;
                    removed +=
                        transaction.execute("DELETE FROM items WHERE id=?1", [duplicate])?;
                }
            }
            transaction.commit()?;
            Ok(removed)
        })
        .await
}
async fn probe(executable: &str, path: &str) -> Option<(Option<i64>, String)> {
    use tokio::io::AsyncReadExt;
    const MAX_OUTPUT: u64 = 1024 * 1024;
    let mut child = Command::new(executable)
        .args([
            "-v", "error", "-protocol_whitelist", "file,pipe",
            "-show_entries",
            "format=duration:stream=index,codec_type,codec_name,width,height,channels,sample_rate:stream_tags=language:stream_disposition=default",
            "-of", "json", path,
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn().ok()?;
    let stdout = child.stdout.take()?;
    let bytes = tokio::time::timeout(Duration::from_secs(30), async move {
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
    let data: Value = serde_json::from_slice(&bytes).ok()?;
    let runtime = data["format"]["duration"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
        .map(|n| (n * 10_000_000.0) as i64);
    let streams=data["streams"].as_array()?.iter().map(|s|json!({
        "Index":s["index"],"Type":match s["codec_type"].as_str(){Some("video")=>"Video",Some("audio")=>"Audio",Some("subtitle")=>"Subtitle",_=>"Unknown"},
        "Codec":s["codec_name"],"Width":s["width"],"Height":s["height"],"Channels":s["channels"],
        "SampleRate":s["sample_rate"].as_str().and_then(|s|s.parse::<u32>().ok()),
        "Language":s["tags"]["language"],"Title":s["tags"]["title"],"IsDefault":s["disposition"]["default"]==1,"IsForced":s["disposition"]["forced"]==1,"IsExternal":false
    })).collect::<Vec<_>>();
    Some((runtime, serde_json::to_string(&streams).ok()?))
}

/// A Unix directory's device/inode pair detects replaced mounts and root directories.
/// Non-Unix targets currently retain canonical-path validation only.
pub fn root_identity(metadata: &std::fs::Metadata) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}", metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        None
    }
}

async fn ensure_hierarchy(
    state: &AppState,
    library: &str,
    root: &str,
    path: &str,
    generation: &str,
) -> Result<Option<(String, u32, u32)>> {
    let path = std::path::Path::new(path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let folder = path
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.parent())
        .and_then(|p| p.components().next())
        .and_then(|p| p.as_os_str().to_str())
        .unwrap_or_else(|| {
            std::path::Path::new(root)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown series")
        });
    let Some(reference) = crate::tmdb::parse_episode(stem)
        .or_else(|| crate::tmdb::parse_episode(&format!("{folder} {stem}")))
    else {
        return Ok(None);
    };
    let library = library.to_owned();
    let generation = generation.to_owned();
    let season = reference.season;
    let episode = reference.episode;
    let parent = state.db.call(move |c| {
        let tx = c.transaction()?;
        let series_path = format!("series://{}/{}",library,hex::encode(reference.show.to_lowercase().as_bytes()));
        tx.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,scan_id) VALUES (?1,?2,?3,?4,'Series','',0,0,?5)
            ON CONFLICT(path) DO UPDATE SET scan_id=excluded.scan_id", params![id(),library,series_path,reference.show,generation])?;
        let series: String = tx.query_row("SELECT id FROM items WHERE path=?1",[&series_path],|r|r.get(0))?;
        let season_path = format!("{series_path}/season/{season}");
        let name = if season == 0 { "Specials".into() } else {format!("Season {season}")};
        tx.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,scan_id,parent_id,index_number)
            VALUES (?1,?2,?3,?4,'Season','',0,0,?5,?6,?7) ON CONFLICT(path) DO UPDATE SET scan_id=excluded.scan_id",
            params![id(),library,season_path,name,generation,series,season])?;
        let parent = tx.query_row("SELECT id FROM items WHERE path=?1",[season_path],|r|r.get::<_,String>(0))?;
        tx.commit()?; Ok(parent)
    }).await?;
    Ok(Some((parent, season, episode)))
}

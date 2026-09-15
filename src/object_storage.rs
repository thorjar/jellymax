use crate::{
    AppState,
    auth::{Auth, id},
    error::{Error, Result},
};
use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, Request, State},
    http::{Method, StatusCode, header},
    response::Response,
};
use futures_util::StreamExt;
use object_store::{
    GetOptions, GetRange, ObjectStore, ObjectStoreExt, aws::AmazonS3Builder, path::Path,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{ops::Range, sync::Arc, time::Duration};
use tokio::process::Command;

#[derive(Clone)]
struct StoreConfig {
    id: String,
    endpoint: Option<String>,
    region: String,
    bucket: String,
    prefix: String,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct StoreView {
    id: String,
    name: String,
    endpoint: Option<String>,
    region: String,
    bucket: String,
    prefix: String,
    library_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct NewStore {
    name: String,
    endpoint: Option<String>,
    region: String,
    bucket: String,
    prefix: Option<String>,
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
    collection_type: String,
}

fn store(config: &StoreConfig) -> Result<Arc<dyn ObjectStore>> {
    let mut builder = AmazonS3Builder::new()
        .with_region(&config.region)
        .with_bucket_name(&config.bucket)
        .with_access_key_id(&config.access_key_id)
        .with_secret_access_key(&config.secret_access_key);
    if let Some(token) = config
        .session_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.with_token(token);
    }
    if let Some(endpoint) = config.endpoint.as_deref().filter(|value| !value.is_empty()) {
        let parsed = url::Url::parse(endpoint).map_err(|_| Error::bad("Invalid S3 endpoint"))?;
        if !matches!(parsed.scheme(), "https" | "http") || parsed.host_str().is_none() {
            return Err(Error::bad("S3 endpoint must be an HTTP(S) origin"));
        }
        builder = builder
            .with_endpoint(endpoint.trim_end_matches('/'))
            .with_allow_http(parsed.scheme() == "http");
    }
    Ok(Arc::new(builder.build().map_err(|error| {
        Error::bad(&format!("Invalid object-store configuration: {error}"))
    })?))
}

async fn config_for_store(state: &AppState, store_id: String) -> Result<StoreConfig> {
    state
        .db
        .call(move |connection| {
            connection.query_row(
            "SELECT id,endpoint,region,bucket,prefix,access_key_id,secret_access_key,session_token
             FROM object_stores WHERE id=?1", [store_id], |row| Ok(StoreConfig {
                id: row.get(0)?, endpoint: row.get(1)?, region: row.get(2)?,
                bucket: row.get(3)?, prefix: row.get(4)?, access_key_id: row.get(5)?,
                secret_access_key: row.get(6)?, session_token: row.get(7)?,
            })
        ).optional()?.ok_or_else(Error::missing)
        })
        .await
}

async fn item_config(state: &AppState, item: String) -> Result<(StoreConfig, String)> {
    let result = state
        .db
        .call(move |connection| {
            connection
                .query_row(
                    "SELECT o.id,o.endpoint,o.region,o.bucket,o.prefix,o.access_key_id,
                    o.secret_access_key,o.session_token,i.path
             FROM items i JOIN libraries l ON l.id=i.library_id
             JOIN object_stores o ON o.id=l.object_store_id WHERE i.id=?1",
                    [item],
                    |row| {
                        Ok((
                            StoreConfig {
                                id: row.get(0)?,
                                endpoint: row.get(1)?,
                                region: row.get(2)?,
                                bucket: row.get(3)?,
                                prefix: row.get(4)?,
                                access_key_id: row.get(5)?,
                                secret_access_key: row.get(6)?,
                                session_token: row.get(7)?,
                            },
                            row.get::<_, String>(8)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(Error::missing)
        })
        .await?;
    let marker = format!("s3://{}/", result.0.id);
    let key = result
        .1
        .strip_prefix(&marker)
        .ok_or_else(|| Error::internal("Invalid object item path"))?
        .to_owned();
    Ok((result.0, key))
}

pub async fn list(auth: Auth, State(state): State<AppState>) -> Result<Json<Vec<StoreView>>> {
    auth.admin()?;
    state
        .db
        .call(|connection| {
            let mut statement = connection.prepare(
                "SELECT o.id,o.name,o.endpoint,o.region,o.bucket,o.prefix,l.id
             FROM object_stores o JOIN libraries l ON l.object_store_id=o.id ORDER BY o.name,o.id",
            )?;
            let values = statement
                .query_map([], |row| {
                    Ok(StoreView {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        endpoint: row.get(2)?,
                        region: row.get(3)?,
                        bucket: row.get(4)?,
                        prefix: row.get(5)?,
                        library_id: row.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(Json(values))
        })
        .await
}

pub async fn connect(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<NewStore>,
) -> Result<(StatusCode, Json<Value>)> {
    auth.admin()?;
    if input.name.trim().is_empty()
        || input.name.len() > 128
        || input.bucket.trim().is_empty()
        || input.access_key_id.trim().is_empty()
        || input.secret_access_key.is_empty()
        || !["movies", "tvshows", "music", "homevideos"].contains(&input.collection_type.as_str())
    {
        return Err(Error::bad("Complete all required object-store fields"));
    }
    let region = if input.region.trim().is_empty() {
        "us-east-1".to_owned()
    } else {
        input.region.trim().to_owned()
    };
    let prefix = input
        .prefix
        .unwrap_or_default()
        .trim_matches('/')
        .to_owned();
    let store_id = id();
    let library_id = id();
    let config = StoreConfig {
        id: store_id.clone(),
        endpoint: input.endpoint.filter(|v| !v.trim().is_empty()),
        region: region.clone(),
        bucket: input.bucket.trim().to_owned(),
        prefix: prefix.clone(),
        access_key_id: input.access_key_id.trim().to_owned(),
        secret_access_key: input.secret_access_key,
        session_token: input.session_token.filter(|v| !v.is_empty()),
    };
    let backend = store(&config)?;
    let path = (!prefix.is_empty()).then(|| Path::from(prefix.clone()));
    let mut check = backend.list(path.as_ref());
    tokio::time::timeout(Duration::from_secs(15), check.next())
        .await
        .map_err(|_| Error(StatusCode::GATEWAY_TIMEOUT, "Object store timed out".into()))?
        .transpose()
        .map_err(|e| {
            Error(
                StatusCode::BAD_GATEWAY,
                format!("Object store rejected the request: {e}"),
            )
        })?;
    let endpoint = config.endpoint.clone();
    let bucket = config.bucket.clone();
    let access = config.access_key_id.clone();
    let secret = config.secret_access_key.clone();
    let token = config.session_token.clone();
    let library_path = format!("s3://{store_id}/{prefix}");
    let display = input.name.trim().to_owned();
    let kind = input.collection_type;
    let inserted_store = store_id.clone();
    let inserted_library = library_id.clone();
    state.db.call(move|connection|{
        let tx=connection.transaction()?;
        tx.execute("INSERT INTO object_stores(id,name,endpoint,region,bucket,prefix,access_key_id,secret_access_key,session_token)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![inserted_store,display,endpoint,region,bucket,prefix,access,secret,token])?;
        tx.execute("INSERT INTO libraries(id,name,path,kind,object_store_id) VALUES (?1,?2,?3,?4,?5)",
            params![inserted_library,display,library_path,kind,inserted_store])?;
        tx.commit()?; Ok(())
    }).await?;
    if let Err(error) = sync_store(&state, store_id.clone()).await {
        let cleanup = store_id.clone();
        let _ = state
            .db
            .call(move |c| {
                c.execute("DELETE FROM object_stores WHERE id=?1", [cleanup])?;
                Ok(())
            })
            .await;
        return Err(error);
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({"Id":store_id,"LibraryId":library_id})),
    ))
}

pub async fn remove(
    auth: Auth,
    State(state): State<AppState>,
    AxumPath(store_id): AxumPath<String>,
) -> Result<StatusCode> {
    auth.admin()?;
    state
        .db
        .call(move |c| {
            if c.execute("DELETE FROM object_stores WHERE id=?1", [store_id])? == 0 {
                return Err(Error::missing());
            }
            Ok(())
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn sync(
    auth: Auth,
    State(state): State<AppState>,
    AxumPath(store_id): AxumPath<String>,
) -> Result<Json<Value>> {
    auth.admin()?;
    let count = sync_store(&state, store_id).await?;
    Ok(Json(json!({"Items":count})))
}

async fn sync_store(state: &AppState, store_id: String) -> Result<usize> {
    let config = config_for_store(state, store_id.clone()).await?;
    let backend = store(&config)?;
    let (library_id, kind, library_path) = state
        .db
        .call({
            let id = store_id.clone();
            move |c| {
                Ok(c.query_row(
                    "SELECT id,kind,path FROM libraries WHERE object_store_id=?1",
                    [id],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                        ))
                    },
                )?)
            }
        })
        .await?;
    let generation = id();
    let prefix = (!config.prefix.is_empty()).then(|| Path::from(config.prefix.clone()));
    let mut objects = backend.list(prefix.as_ref());
    let mut count = 0usize;
    while let Some(object) = objects.next().await {
        let object = object.map_err(|e| {
            Error(
                StatusCode::BAD_GATEWAY,
                format!("Object listing failed: {e}"),
            )
        })?;
        let key = object.location.to_string();
        let Some(container) = crate::scanner::media_extension(std::path::Path::new(&key), &kind)
        else {
            continue;
        };
        let item_type = match kind.as_str() {
            "music" => "Audio",
            "tvshows" => "Episode",
            "movies" => "Movie",
            _ => "Video",
        };
        let object_path = format!("s3://{store_id}/{key}");
        let name = std::path::Path::new(&key)
            .file_stem()
            .and_then(|v| v.to_str())
            .unwrap_or("Untitled")
            .replace(['.', '_'], " ");
        let size = object.size.min(i64::MAX as u64) as i64;
        let modified = object
            .last_modified
            .timestamp_nanos_opt()
            .unwrap_or(i64::MAX);
        let existing = state
            .db
            .call({
                let path = object_path.clone();
                move |connection| {
                    Ok(connection
                        .query_row(
                            "SELECT size,modified,runtime_ticks,tmdb_id FROM items WHERE path=?1",
                            [path],
                            |row| {
                                Ok((
                                    row.get::<_, i64>(0)?,
                                    row.get::<_, i64>(1)?,
                                    row.get::<_, Option<i64>>(2)?,
                                    row.get::<_, Option<String>>(3)?,
                                ))
                            },
                        )
                        .optional()?)
                }
            })
            .await?;
        let needs_probe = existing
            .as_ref()
            .is_none_or(|(old_size, old_modified, runtime, _)| {
                *old_size != size || *old_modified != modified || runtime.is_none()
            });
        let needs_metadata = existing
            .as_ref()
            .is_none_or(|(_, _, _, tmdb_id)| tmdb_id.is_none());
        let hierarchy = if kind == "tvshows" {
            crate::scanner::ensure_hierarchy(
                state,
                &library_id,
                &library_path,
                &object_path,
                &generation,
            )
            .await?
        } else {
            None
        };
        let (parent, season, episode) = hierarchy
            .clone()
            .map(|v| (Some(v.0), Some(v.1), Some(v.2)))
            .unwrap_or_default();
        let new_id = id();
        let insert_path = object_path.clone();
        let insert_name = name.clone();
        let library = library_id.clone();
        let generation_value = generation.clone();
        let container_value = container.clone();
        let kind_value = item_type.to_owned();
        let item_id=state.db.call(move|c|{
            c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,media_streams,scan_id,parent_id,parent_index_number,index_number)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'[]',?9,?10,?11,?12)
                ON CONFLICT(path) DO UPDATE SET size=excluded.size,modified=excluded.modified,scan_id=excluded.scan_id,
                parent_id=excluded.parent_id,parent_index_number=excluded.parent_index_number,index_number=excluded.index_number",
                params![new_id,library,insert_path,insert_name,kind_value,container_value,size,modified,generation_value,parent,season,episode])?;
            Ok(c.query_row("SELECT id FROM items WHERE path=?1",[object_path],|r|r.get::<_,String>(0))?)
        }).await?;
        if needs_probe {
            probe_object(state, &item_id).await;
        }
        if needs_metadata && state.tmdb.enabled() {
            if let Some((ref season_id, _, _)) = hierarchy {
                let _ = crate::tmdb::enrich_tv_parents(state, season_id).await;
            }
            if item_type == "Episode" {
                let _ = crate::tmdb::enrich_episode(state, &item_id, &name).await;
            } else if item_type == "Movie" {
                let _ = crate::tmdb::enrich_movie(state, &item_id, &name).await;
            }
        }
        count += 1;
    }
    let removed = state
        .db
        .call(move |c| {
            Ok(c.execute(
                "DELETE FROM items WHERE library_id=?1 AND scan_id<>?2",
                params![library_id, generation],
            )?)
        })
        .await?;
    tracing::info!(store=%config.id,count,removed,"S3-compatible library synchronized");
    Ok(count)
}

async fn probe_object(state: &AppState, item: &str) {
    let origin = state.internal_origin.read().await.clone();
    if origin.is_empty() {
        return;
    }
    let url = format!("{origin}/ObjectItems/{item}/stream");
    let headers = format!("X-Jellymax-Internal: {}\r\n", state.internal_token);
    let mut command = Command::new(&state.ffprobe);
    command.args(["-v","error","-headers",&headers,"-show_entries",
        "format=duration:stream=index,codec_type,codec_name,width,height,channels,sample_rate:stream_tags=language,title:stream_disposition=default,forced",
        "-of","json",&url]).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let Ok(Ok(output)) = tokio::time::timeout(Duration::from_secs(45), command.output()).await
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    let Ok(data) = serde_json::from_slice::<Value>(&output.stdout) else {
        return;
    };
    let runtime = data["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
        .map(|v| (v * 10_000_000.0) as i64);
    let streams=data["streams"].as_array().map(|values|values.iter().map(|s|json!({
        "Index":s["index"],"Type":match s["codec_type"].as_str(){Some("video")=>"Video",Some("audio")=>"Audio",Some("subtitle")=>"Subtitle",_=>"Unknown"},
        "Codec":s["codec_name"],"Width":s["width"],"Height":s["height"],"Channels":s["channels"],
        "SampleRate":s["sample_rate"].as_str().and_then(|v|v.parse::<u32>().ok()),"Language":s["tags"]["language"],
        "Title":s["tags"]["title"],"IsDefault":s["disposition"]["default"]==1,"IsForced":s["disposition"]["forced"]==1,"IsExternal":false
    })).collect::<Vec<_>>()).unwrap_or_default();
    let streams = serde_json::to_string(&streams).unwrap_or_else(|_| "[]".into());
    let item = item.to_owned();
    let _ = state
        .db
        .call(move |c| {
            c.execute(
                "UPDATE items SET runtime_ticks=?2,media_streams=?3 WHERE id=?1",
                params![item, runtime, streams],
            )?;
            Ok(())
        })
        .await;
}

fn parse_range(value: Option<&str>, size: u64) -> Result<Option<Range<u64>>> {
    let Some(value) = value else { return Ok(None) };
    let value = value
        .strip_prefix("bytes=")
        .ok_or_else(|| Error::bad("Invalid range"))?;
    if value.contains(',') {
        return Err(Error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "Multiple ranges are unsupported".into(),
        ));
    }
    let (start, end) = value
        .split_once('-')
        .ok_or_else(|| Error::bad("Invalid range"))?;
    let range = if start.is_empty() {
        let suffix = end
            .parse::<u64>()
            .map_err(|_| Error::bad("Invalid range"))?;
        if suffix == 0 || size == 0 {
            return Err(Error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "Range is outside the object".into(),
            ));
        }
        size.saturating_sub(suffix)..size
    } else {
        let start = start
            .parse::<u64>()
            .map_err(|_| Error::bad("Invalid range"))?;
        let end = if end.is_empty() {
            size
        } else {
            end.parse::<u64>()
                .map_err(|_| Error::bad("Invalid range"))?
                .saturating_add(1)
                .min(size)
        };
        if start >= end || start >= size {
            return Err(Error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "Range is outside the object".into(),
            ));
        }
        start..end
    };
    Ok(Some(range))
}

pub async fn stream(
    State(state): State<AppState>,
    AxumPath(item): AxumPath<String>,
    request: Request,
) -> Result<Response> {
    let internal = request
        .headers()
        .get("x-jellymax-internal")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == state.internal_token);
    let request = if internal {
        request
    } else {
        crate::tickets::authorize(&state, &item, request).await?
    };
    let method = request.method().clone();
    let requested = request
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok());
    let (config, key) = item_config(&state, item).await?;
    let backend = store(&config)?;
    let location = Path::from(key);
    let metadata = backend
        .head(&location)
        .await
        .map_err(|e| Error(StatusCode::BAD_GATEWAY, format!("Object read failed: {e}")))?;
    let range = parse_range(requested, metadata.size)?;
    if method == Method::HEAD {
        let status = if range.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        let length = range
            .as_ref()
            .map(|range| range.end - range.start)
            .unwrap_or(metadata.size);
        let mut builder = Response::builder()
            .status(status)
            .header(header::CONTENT_LENGTH, length)
            .header(header::ACCEPT_RANGES, "bytes");
        if let Some(range) = range {
            builder = builder.header(
                header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", range.start, range.end - 1, metadata.size),
            );
        }
        return builder.body(Body::empty()).map_err(Error::internal);
    }
    let options = GetOptions {
        range: range.clone().map(GetRange::Bounded),
        ..Default::default()
    };
    let result = backend
        .get_opts(&location, options)
        .await
        .map_err(|e| Error(StatusCode::BAD_GATEWAY, format!("Object read failed: {e}")))?;
    let status = if range.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let length = range
        .as_ref()
        .map(|r| r.end - r.start)
        .unwrap_or(metadata.size);
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_LENGTH, length)
        .header(header::ACCEPT_RANGES, "bytes");
    if let Some(range) = range {
        builder = builder.header(
            header::CONTENT_RANGE,
            format!("bytes {}-{}/{}", range.start, range.end - 1, metadata.size),
        );
    }
    if let Some(content_type) = result.attributes.get(&object_store::Attribute::ContentType) {
        builder = builder.header(header::CONTENT_TYPE, content_type.as_ref());
    }
    builder
        .body(Body::from_stream(result.into_stream()))
        .map_err(Error::internal)
}

pub async fn ffmpeg_source(
    state: &AppState,
    item: &str,
) -> Result<Option<(String, Option<String>)>> {
    if item_config(state, item.to_owned()).await.is_err() {
        return Ok(None);
    }
    let origin = state.internal_origin.read().await.clone();
    if origin.is_empty() {
        return Err(Error::internal("Internal object-stream origin unavailable"));
    }
    Ok(Some((
        format!("{origin}/ObjectItems/{item}/stream"),
        Some(format!("X-Jellymax-Internal: {}\r\n", state.internal_token)),
    )))
}

pub async fn is_item(state: &AppState, item: &str) -> bool {
    item_config(state, item.to_owned()).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browser_byte_ranges() {
        assert_eq!(parse_range(None, 100).unwrap(), None);
        assert_eq!(parse_range(Some("bytes=2-5"), 100).unwrap(), Some(2..6));
        assert_eq!(parse_range(Some("bytes=95-"), 100).unwrap(), Some(95..100));
        assert_eq!(parse_range(Some("bytes=-5"), 100).unwrap(), Some(95..100));
        assert_eq!(parse_range(Some("bytes=-500"), 100).unwrap(), Some(0..100));
        assert_eq!(
            parse_range(Some("bytes=100-"), 100).unwrap_err().0,
            StatusCode::RANGE_NOT_SATISFIABLE
        );
    }
}

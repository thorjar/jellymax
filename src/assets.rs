//! Local artwork and external subtitles. Paths are derived from catalog items, never URL paths.
use crate::{
    AppState,
    auth::Auth,
    error::{Error, Result},
    playback::media_path,
};
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::header,
    response::Response,
};
use futures_util::stream;
use serde_json::{Value, json};
use std::{
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};
use tower::ServiceExt;
use tower_http::services::ServeFile;

async fn safe_sidecar(parent: &FsPath, path: &FsPath) -> Option<PathBuf> {
    let resolved = tokio::fs::canonicalize(path).await.ok()?;
    if !resolved.starts_with(parent) || !tokio::fs::metadata(&resolved).await.ok()?.is_file() {
        return None;
    }
    Some(resolved)
}
pub async fn image(
    _auth: Auth,
    State(state): State<AppState>,
    Path(item): Path<String>,
    request: Request,
) -> Result<Response> {
    if let Some(response) = crate::remote::image(&state, &item).await? {
        return Ok(response);
    }
    // Confirm existence before constructing a server-owned artwork path.
    let kind = state
        .db
        .call({
            let id = item.clone();
            move |c| {
                use rusqlite::OptionalExtension;
                c.query_row("SELECT kind FROM items WHERE id=?1", [id], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?
                .ok_or_else(Error::missing)
            }
        })
        .await?;
    let artwork = state.data_dir.clone().join("artwork");
    for extension in ["jpg", "png", "webp"] {
        if let Some(path) =
            safe_sidecar(&artwork, &artwork.join(format!("{item}.{extension}"))).await
        {
            return Ok(ServeFile::new(path)
                .oneshot(request)
                .await
                .map_err(Error::internal)?
                .map(Body::new));
        }
    }
    if matches!(kind.as_str(), "Series" | "Season") {
        return Err(Error::missing());
    }
    let media = media_path(&state, item.clone()).await?;
    let parent = media.parent().ok_or_else(Error::missing)?;
    let stem = media
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(Error::missing)?;
    for name in [
        format!("{stem}-poster"),
        stem.into(),
        "poster".into(),
        "folder".into(),
        "cover".into(),
    ] {
        for extension in ["jpg", "jpeg", "png", "webp"] {
            if let Some(path) =
                safe_sidecar(parent, &parent.join(format!("{name}.{extension}"))).await
            {
                return Ok(ServeFile::new(path)
                    .oneshot(request)
                    .await
                    .map_err(Error::internal)?
                    .map(Body::new));
            }
        }
    }
    Err(Error::missing())
}
struct Subtitle {
    path: PathBuf,
    codec: String,
    language: Option<String>,
}

pub fn text_subtitle_codec(codec: &str) -> bool {
    matches!(
        codec.to_ascii_lowercase().as_str(),
        "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "vtt" | "mov_text" | "text"
    )
}
async fn find_subtitles(state: &AppState, item: &str) -> Result<Vec<Subtitle>> {
    let media = media_path(state, item.to_owned()).await?;
    let parent = media.parent().ok_or_else(Error::missing)?;
    let stem = media
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(Error::missing)?;
    let mut directory = tokio::fs::read_dir(parent).await?;
    let mut result = vec![];
    while let Some(entry) = directory.next_entry().await? {
        let path = entry.path();
        let Some(codec) = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        if !["srt", "vtt", "ass", "ssa"].contains(&codec.as_str()) {
            continue;
        }
        let Some(subtitle_stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let language = if subtitle_stem == stem {
            None
        } else if let Some(suffix) = subtitle_stem.strip_prefix(&format!("{stem}.")) {
            Some(suffix.to_owned())
        } else {
            continue;
        };
        if let Some(resolved) = safe_sidecar(parent, &path).await {
            result.push(Subtitle {
                path: resolved,
                codec,
                language,
            });
        }
        if result.len() > 100 {
            return Err(Error::bad("Too many external subtitles for this item"));
        }
    }
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(result)
}
pub async fn subtitle_streams(state: &AppState, item: &str) -> Result<Vec<Value>> {
    Ok(find_subtitles(state,item).await?.iter().enumerate().map(|(index,s)|json!({
        "Index":10000+index,"Type":"Subtitle","Codec":s.codec,"Language":s.language,
        "IsExternal":true,"DeliveryMethod":"External","DeliveryUrl":format!("/Items/{item}/Subtitles/{}",10000+index)
    })).collect())
}
pub async fn subtitles(
    _auth: Auth,
    State(state): State<AppState>,
    Path(item): Path<String>,
) -> Result<Json<Vec<Value>>> {
    Ok(Json(subtitle_streams(&state, &item).await?))
}
pub async fn subtitle(
    State(state): State<AppState>,
    Path((item, index)): Path<(String, usize)>,
    Query(query): Query<SubtitleQuery>,
    request: Request,
) -> Result<Response> {
    let request = crate::tickets::authorize(&state, &item, request).await?;
    if index < 10000 {
        return embedded_subtitle(&state, &item, index, query.start_seconds).await;
    }
    let index = index - 10000;
    let subtitles = find_subtitles(&state, &item).await?;
    let subtitle = subtitles.get(index).ok_or_else(Error::missing)?;
    if subtitle.codec == "srt" {
        let source = String::from_utf8_lossy(&tokio::fs::read(&subtitle.path).await?).into_owned();
        let vtt = srt_to_vtt(&source);
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
            .body(Body::from(vtt))
            .map_err(Error::internal);
    }
    if matches!(subtitle.codec.as_str(), "ass" | "ssa") {
        return ffmpeg_subtitle(&state, subtitle.path.as_os_str().to_owned(), None, 0, None).await;
    }
    Ok(ServeFile::new(&subtitle.path)
        .oneshot(request)
        .await
        .map_err(Error::internal)?
        .map(Body::new))
}

#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubtitleQuery {
    start_seconds: Option<u64>,
}

async fn embedded_subtitle(
    state: &AppState,
    item: &str,
    index: usize,
    start_seconds: Option<u64>,
) -> Result<Response> {
    let item_id = item.to_owned();
    let streams: String = state
        .db
        .call(move |c| {
            Ok(c.query_row(
                "SELECT media_streams FROM items WHERE id=?1",
                [item_id],
                |r| r.get(0),
            )?)
        })
        .await?;
    let streams: Vec<Value> = serde_json::from_str(&streams).map_err(Error::internal)?;
    let stream = streams
        .iter()
        .find(|s| s["Type"] == "Subtitle" && s["Index"].as_u64() == Some(index as u64))
        .ok_or_else(Error::missing)?;
    let codec = stream["Codec"].as_str().unwrap_or("");
    if stream["IsExternal"] == true {
        let bytes = crate::remote::external_subtitle(state, item, index, codec).await?;
        let text = String::from_utf8_lossy(&bytes);
        let vtt = if matches!(codec.to_ascii_lowercase().as_str(), "subrip" | "srt") {
            srt_to_vtt(&text)
        } else {
            text.into_owned()
        };
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
            .body(Body::from(vtt))
            .map_err(Error::internal);
    }
    if !text_subtitle_codec(codec) {
        return Err(Error::missing());
    }
    let local_ordinal = streams
        .iter()
        .filter(|stream| stream["Type"] == "Subtitle" && stream["IsExternal"] != true)
        .position(|stream| stream["Index"].as_u64() == Some(index as u64))
        .ok_or_else(Error::missing)?;
    let (source, headers, ordinal) = if let Some((url, headers, ordinal)) =
        crate::remote::embedded_subtitle_source(state, item, index, codec).await?
    {
        (url.into(), headers, ordinal)
    } else {
        (
            media_path(state, item.to_owned()).await?.into_os_string(),
            None,
            local_ordinal,
        )
    };
    // Local files are seekable without network traffic. Extract the complete
    // subtitle stream once so cues cannot disappear at window boundaries.
    let window = if headers.is_some() {
        start_seconds
    } else {
        None
    };
    ffmpeg_subtitle(state, source, headers, ordinal, window).await
}

fn srt_to_vtt(source: &str) -> String {
    let mut vtt = String::from("WEBVTT\n\n");
    for line in source.lines() {
        if line.contains(" --> ") {
            vtt.push_str(&line.replace(',', "."));
        } else {
            vtt.push_str(line);
        }
        vtt.push('\n');
    }
    vtt
}

async fn ffmpeg_subtitle(
    state: &AppState,
    source: std::ffi::OsString,
    headers: Option<String>,
    ordinal: usize,
    start_seconds: Option<u64>,
) -> Result<Response> {
    let permit = state
        .subtitle_gate
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::internal("Subtitle extraction queue is unavailable"))?;
    let mut command = Command::new(&state.ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    if let Some(start) = start_seconds {
        command.args(["-ss", &start.to_string()]);
    }
    if let Some(headers) = headers {
        command.args(["-headers", &headers]);
    }
    command.arg("-i").arg(source);
    command.args(["-map", &format!("0:s:{ordinal}")]);
    if let Some(start) = start_seconds {
        command.args(["-t", "90", "-output_ts_offset", &start.to_string()]);
    }
    command
        .args(["-flush_packets", "1", "-f", "webvtt", "pipe:1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(Error::internal)?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::internal("Subtitle output is unavailable"))?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<std::io::Result<Bytes>>(8);
    tokio::spawn(async move {
        let _permit = permit;
        let mut total = 0usize;
        let mut buffer = [0u8; 8192];
        loop {
            let read = tokio::select! {
                _ = sender.closed() => { let _ = child.kill().await; return; }
                result = tokio::time::timeout(Duration::from_secs(45), stdout.read(&mut buffer)) => result,
            };
            match read {
                Ok(Ok(0)) => break,
                Ok(Ok(count)) => {
                    total += count;
                    if total > 16 * 1024 * 1024 {
                        let _ = sender
                            .send(Err(std::io::Error::other("Subtitle track is too large")))
                            .await;
                        let _ = child.kill().await;
                        return;
                    }
                    if sender
                        .send(Ok(Bytes::copy_from_slice(&buffer[..count])))
                        .await
                        .is_err()
                    {
                        let _ = child.kill().await;
                        return;
                    }
                }
                Ok(Err(error)) => {
                    let _ = sender.send(Err(error)).await;
                    let _ = child.kill().await;
                    return;
                }
                Err(_) => {
                    let _ = sender
                        .send(Err(std::io::Error::other("Subtitle extraction timed out")))
                        .await;
                    let _ = child.kill().await;
                    return;
                }
            }
        }
        match child.wait().await {
            Ok(status) if status.success() => {}
            _ => {
                let _ = sender
                    .send(Err(std::io::Error::other(
                        "Subtitle track could not be converted",
                    )))
                    .await;
            }
        }
    });
    let chunks = stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|chunk| (chunk, receiver))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
        .body(Body::from_stream(chunks))
        .map_err(Error::internal)
}

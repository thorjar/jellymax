use crate::{
    AppState,
    auth::{Auth, id, now},
    error::{Error, Result},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, Query, Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use rusqlite::{OptionalExtension, params};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, sync::mpsc};
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub async fn info(
    auth: Auth,
    State(state): State<AppState>,
    Path(item): Path<String>,
    Query(capabilities): Query<PlaybackCapabilities>,
    body: Option<Json<PlaybackRequest>>,
) -> Result<Json<Value>> {
    let remote = crate::remote::for_item(&state, &item).await?.is_some();
    let object = crate::object_storage::is_item(&state, &item).await;
    let _ = body;
    let mut external_streams = if remote || object {
        Vec::new()
    } else {
        crate::assets::subtitle_streams(&state, &item).await?
    };
    let ticket = crate::tickets::issue(&state, &auth, &item).await?;
    let play_session = id();
    for stream in &mut external_streams {
        if let Some(url) = stream["DeliveryUrl"].as_str() {
            stream["DeliveryUrl"] = format!("{url}?PlaybackTicket={ticket}").into();
        }
    }
    let probe_item = item.clone();
    let mut info = state.db.call(move |c| {
        let (kind,container,size,runtime,streams):(String,String,i64,Option<i64>,String)=c.query_row(
            "SELECT kind,container,size,runtime_ticks,media_streams FROM items WHERE id=?1",[&item],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or_else(Error::missing)?;
        let mut streams: Vec<Value> = serde_json::from_str(&streams).unwrap_or_default();
        streams.extend(external_streams);
        for stream in &mut streams {
            if remote
                && stream["Type"] == "Subtitle"
                && stream["IsExternal"] == true
                && matches!(stream["Codec"].as_str().map(str::to_ascii_lowercase).as_deref(), Some("subrip" | "srt" | "webvtt" | "vtt"))
                && let Some(index) = stream["Index"].as_u64()
            {
                stream["DeliveryMethod"] = Value::String("External".into());
                stream["DeliveryUrl"] = Value::String(format!(
                    "/Items/{item}/Subtitles/{index}?PlaybackTicket={ticket}"
                ));
            }
            if stream["Type"] == "Subtitle"
                && stream["IsExternal"] != true
                && crate::assets::text_subtitle_codec(stream["Codec"].as_str().unwrap_or(""))
                && let Some(index) = stream["Index"].as_u64()
            {
                stream["DeliveryMethod"] = Value::String("External".into());
                stream["DeliveryUrl"] = Value::String(format!(
                    "/Items/{item}/Subtitles/{index}?PlaybackTicket={ticket}"
                ));
            }
        }
        let plan = PlaybackPlan::select(
            &kind,
            &container,
            &streams,
            capabilities.supports_hevc,
            capabilities.supports_mkv,
            capabilities.supports_ac3,
            capabilities.supports_eac3,
        );
        let route=if kind=="Audio"{"Audio"}else{"Videos"};
        let direct_url = if remote {
            format!("/RemoteItems/{item}/stream?PlaybackTicket={ticket}")
        } else if object {
            format!("/ObjectItems/{item}/stream?PlaybackTicket={ticket}")
        } else {
            format!("/{route}/{item}/stream?PlaybackTicket={ticket}")
        };
        let codec = |stream_type: &str| streams.iter().find(|stream| stream["Type"] == stream_type)
            .and_then(|stream| stream["Codec"].as_str()).unwrap_or("").to_ascii_lowercase();
        let video_codec = codec("Video");
        let audio_codec = codec("Audio");
        let start_index = (capabilities.start_time_ticks.unwrap_or(0).max(0) as u64 / 10_000_000 / crate::transcode::segment_seconds()).min(runtime.unwrap_or(0).max(0) as u64 / 10_000_000 / crate::transcode::segment_seconds());
        let hls_url = |mode: PlaybackPlan| format!("/Videos/{item}/hls/master.m3u8?PlaybackTicket={ticket}&Mode={}&PlaySessionId={play_session}&VideoCodec={video_codec}&StartIndex={start_index}", mode.query_value());
        let audio_transcoding_mode = if video_codec == "h264"
            || (video_codec == "hevc" && capabilities.supports_hevc)
        { PlaybackPlan::Audio } else { PlaybackPlan::Video };
        let needs_audio_conversion = match audio_codec.as_str() {
            "eac3" => !capabilities.supports_eac3,
            "ac3" => !capabilities.supports_ac3,
            "dts" | "truehd" => true,
            _ => false,
        };
        let audio_transcoding_url = (kind != "Audio" && needs_audio_conversion)
            .then(|| hls_url(audio_transcoding_mode));
        let audio_track_urls: serde_json::Map<String, Value> = if kind == "Audio" { serde_json::Map::new() } else {
            streams.iter().filter(|stream| stream["Type"] == "Audio").filter_map(|stream| {
                let index = stream["Index"].as_u64()?;
                let codec = stream["Codec"].as_str().unwrap_or("").to_ascii_lowercase();
                let audio_supported = match codec.as_str() {
                    "eac3" => capabilities.supports_eac3,
                    "ac3" => capabilities.supports_ac3,
                    "dts" | "truehd" => false,
                    _ => true,
                };
                let mode = if video_codec == "h264" || (video_codec == "hevc" && capabilities.supports_hevc) {
                    if audio_supported { PlaybackPlan::Remux } else { PlaybackPlan::Audio }
                } else { PlaybackPlan::Video };
                Some((index.to_string(), Value::String(format!("{}&AudioStreamIndex={index}", hls_url(mode)))))
            }).collect()
        };
        let fallback_mode = PlaybackPlan::Video;
        let fallback_url = hls_url(fallback_mode);
        let (transcoding_url, transcoding, direct_play, output_container) = match plan {
            PlaybackPlan::Direct => (None, false, true, container.clone()),
            mode if kind == "Audio" => (Some(format!("/Audio/{item}/transcode.m4a?PlaybackTicket={ticket}&Mode={}", mode.query_value())), true, false, "m4a".to_owned()),
            mode => (Some(hls_url(mode)), true, false, "hls".to_owned()),
        };
        Ok(Json(json!({"PlaySessionId":play_session,"MediaSources":[{
            "Id":item,"Protocol":"Http","Type":"Default","Container":output_container,"Size":size,"RunTimeTicks":runtime,
            "MediaStreams":streams,
            "SupportsDirectPlay":direct_play,"SupportsDirectStream":direct_play,"SupportsTranscoding":transcoding,
            "DirectStreamUrl":direct_url,"TranscodingUrl":transcoding_url,"FallbackTranscodingUrl":fallback_url,
            "AudioTranscodingUrl":audio_transcoding_url,
            "AudioTrackUrls":audio_track_urls,
            "AudioTranscodingMode":audio_transcoding_mode.query_value(),
            "TranscodingMode":if transcoding {Some(plan.query_value())} else {None}
        }]})))
    }).await?;
    let source = &mut info.0["MediaSources"][0];
    if (matches!(source["TranscodingMode"].as_str(), Some("remux" | "audio"))
        || (source["AudioTranscodingUrl"].is_string()
            && matches!(source["AudioTranscodingMode"].as_str(), Some("audio"))))
        && sparse_keyframes(
            &state,
            &probe_item,
            source["Size"].as_i64().unwrap_or(0),
            source["RunTimeTicks"].as_i64().unwrap_or(0),
        )
        .await
    {
        for field in ["TranscodingUrl", "AudioTranscodingUrl"] {
            if let Some(url) = source[field].as_str() {
                source[field] = Value::String(
                    url.replace("Mode=remux", "Mode=video")
                        .replace("Mode=audio", "Mode=video"),
                );
            }
        }
        source["TranscodingMode"] = Value::String("video".into());
        source["AudioTranscodingMode"] = Value::String("video".into());
        if let Some(tracks) = source["AudioTrackUrls"].as_object_mut() {
            for url in tracks.values_mut() {
                if let Some(value) = url.as_str() {
                    *url = Value::String(
                        value
                            .replace("Mode=remux", "Mode=video")
                            .replace("Mode=audio", "Mode=video"),
                    );
                }
            }
        }
    }
    Ok(info)
}

async fn sparse_keyframes(state: &AppState, item: &str, size: i64, runtime_ticks: i64) -> bool {
    let segment = crate::transcode::segment_seconds() as f64;
    let duration = runtime_ticks as f64 / 10_000_000.0;
    if duration <= segment {
        return false;
    }
    let key = format!("{item}:{size}:{runtime_ticks}");
    if let Some(result) = state.keyframe_safety.lock().unwrap().get(&key).copied() {
        return result;
    }
    let resolved = match crate::remote::ffmpeg_source(state, item).await {
        Ok(Some(source)) => Some(source),
        Ok(None) => match crate::object_storage::ffmpeg_source(state, item).await {
            Ok(source) => source,
            Err(_) => return true,
        },
        Err(_) => return true,
    };
    let (source, headers) = if let Some((source, headers)) = resolved {
        (source.into(), headers)
    } else {
        match media_path(state, item.to_owned()).await {
            Ok(path) => (path.into_os_string(), None),
            Err(_) => return true,
        }
    };
    let mut command = Command::new(&state.ffprobe);
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-read_intervals",
        "0%+18",
        "-show_packets",
        "-show_entries",
        "packet=pts_time,flags",
        "-of",
        "csv=p=0",
    ]);
    if let Some(headers) = headers {
        command.args(["-headers", &headers]);
    }
    command.arg(source).kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(3), command.output()).await;
    let sparse = match result {
        Ok(Ok(output)) if output.status.success() => {
            let keyframes: Vec<f64> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| {
                    let (time, flags) = line.split_once(',')?;
                    flags
                        .contains('K')
                        .then(|| time.parse::<f64>().ok())
                        .flatten()
                })
                .collect();
            (1..=2).any(|boundary| {
                let target = boundary as f64 * segment;
                target < duration - 0.1
                    && !keyframes.iter().any(|time| (time - target).abs() < 0.15)
            })
        }
        // A failed probe cannot certify the fixed-duration remux manifest.
        // Video mode forces the keyframes that the playlist advertises.
        Ok(Err(_)) => true,
        Ok(Ok(_)) => true,
        Err(_) => true, // A slow source cannot be certified safe to remux.
    };
    let mut cache = state.keyframe_safety.lock().unwrap();
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(key, sparse);
    sparse
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlaybackCapabilities {
    supports_hevc: bool,
    supports_mkv: bool,
    supports_ac3: bool,
    supports_eac3: bool,
    start_time_ticks: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PlaybackRequest {
    device_profile: Option<Value>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PlaybackPlan {
    Direct,
    Remux,
    Audio,
    AudioOnly,
    Video,
}
impl PlaybackPlan {
    fn query_value(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Remux => "remux",
            Self::Audio => "audio",
            Self::AudioOnly => "audio-only",
            Self::Video => "video",
        }
    }
    fn select(
        kind: &str,
        container: &str,
        streams: &[Value],
        supports_hevc: bool,
        supports_mkv: bool,
        supports_ac3: bool,
        supports_eac3: bool,
    ) -> Self {
        let codec = |stream_type: &str| {
            streams
                .iter()
                .find(|s| s["Type"] == stream_type)
                .and_then(|s| s["Codec"].as_str())
                .unwrap_or("")
                .to_ascii_lowercase()
        };
        let audio = codec("Audio");
        if kind == "Audio" {
            return if matches!(
                (container, audio.as_str()),
                ("mp3", "mp3")
                    | ("m4a" | "mp4", "aac")
                    | ("ogg", "opus" | "vorbis")
                    | ("flac", "flac")
            ) {
                Self::Direct
            } else {
                Self::Audio
            };
        }
        let video = codec("Video");
        let compatible_video = video == "h264" || (video == "hevc" && supports_hevc);
        let compatible_audio = matches!(audio.as_str(), "" | "aac" | "mp3")
            || (audio == "ac3" && supports_ac3)
            || (audio == "eac3" && supports_eac3);
        if (container == "mp4" || (container == "mkv" && supports_mkv))
            && compatible_video
            && compatible_audio
        {
            return Self::Direct;
        }
        if container == "webm"
            && matches!(video.as_str(), "vp8" | "vp9" | "av1")
            && matches!(audio.as_str(), "" | "opus" | "vorbis")
        {
            return Self::Direct;
        }
        if compatible_video && matches!(audio.as_str(), "" | "aac" | "mp3") {
            Self::Remux
        } else if compatible_video {
            Self::Audio
        } else {
            Self::Video
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TranscodeQuery {
    mode: String,
    start_time_ticks: Option<i64>,
}

fn playback_mode(value: &str) -> Result<PlaybackPlan> {
    match value {
        "remux" => Ok(PlaybackPlan::Remux),
        "audio" => Ok(PlaybackPlan::Audio),
        "audio-only" => Ok(PlaybackPlan::AudioOnly),
        "video" => Ok(PlaybackPlan::Video),
        _ => Err(Error::bad("Invalid transcode mode")),
    }
}

pub async fn transcode(
    State(state): State<AppState>,
    Path(item): Path<String>,
    Query(query): Query<TranscodeQuery>,
    request: Request,
) -> Result<Response> {
    let request = crate::tickets::authorize(&state, &item, request).await?;
    let mode = playback_mode(&query.mode)?;
    if query.start_time_ticks.is_some_and(|ticks| ticks < 0) {
        return Err(Error::bad("StartTimeTicks must not be negative"));
    }
    let is_audio = state
        .db
        .call({
            let item = item.clone();
            move |c| {
                Ok(
                    c.query_row("SELECT kind='Audio' FROM items WHERE id=?1", [item], |r| {
                        r.get::<_, bool>(0)
                    })?,
                )
            }
        })
        .await?;
    if request.method() == Method::HEAD {
        return Response::builder()
            .header(
                header::CONTENT_TYPE,
                if is_audio { "audio/mp4" } else { "video/mp4" },
            )
            .body(Body::empty())
            .map_err(Error::internal);
    }
    ffmpeg_stream(
        state,
        item,
        mode,
        query
            .start_time_ticks
            .map(|ticks| ticks as f64 / 10_000_000.0),
        None,
        if is_audio { "ipod" } else { "mp4" },
        if is_audio { "audio/mp4" } else { "video/mp4" },
    )
    .await
}

async fn ffmpeg_stream(
    state: AppState,
    item: String,
    mode: PlaybackPlan,
    start_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    format: &'static str,
    content_type: &'static str,
) -> Result<Response> {
    let permit = state
        .transcode_gate
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::internal("Transcoding queue is unavailable"))?;
    let resolved = if let Some(source) = crate::remote::ffmpeg_source(&state, &item).await? {
        Some(source)
    } else {
        crate::object_storage::ffmpeg_source(&state, &item).await?
    };
    let (source, source_headers) = if let Some(source) = resolved {
        (source.0.into(), source.1)
    } else {
        (
            media_path(&state, item.clone()).await?.into_os_string(),
            None,
        )
    };
    let mut command = Command::new(&state.ffmpeg);
    command.args(["-hide_banner", "-loglevel", "warning", "-nostdin"]);
    if let Some(seconds) = start_seconds.filter(|seconds| *seconds > 0.0) {
        command.args(["-ss", &seconds.to_string()]);
    }
    if let Some(headers) = source_headers {
        command.args(["-headers", &headers]);
    }
    command.arg("-i").arg(source);
    if mode == PlaybackPlan::AudioOnly {
        command.args(["-map", "0:a:0"]);
    } else {
        command.args(["-map", "0:v:0?", "-map", "0:a:0?"]);
    }
    match mode {
        PlaybackPlan::Remux => {
            command.args(["-c", "copy"]);
        }
        PlaybackPlan::Audio => {
            command.args(["-c:v", "copy", "-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
        }
        PlaybackPlan::AudioOnly => {
            command.args(["-vn", "-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
        }
        PlaybackPlan::Video => {
            command.args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-crf",
                "24",
                "-vf",
                "scale=w='min(1920,iw)':h='min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-ac",
                "2",
                "-b:a",
                "192k",
            ]);
        }
        PlaybackPlan::Direct => unreachable!(),
    }
    if let Some(seconds) = duration_seconds {
        command.args(["-t", &seconds.to_string()]);
    }
    if format == "mp4" || format == "ipod" {
        command.args(["-movflags", "frag_keyframe+empty_moov+default_base_moof"]);
    } else {
        command.args(["-reset_timestamps", "1", "-avoid_negative_ts", "make_zero"]);
    }
    command
        .args(["-f", format, "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        tracing::error!(%error, executable=%state.ffmpeg,"Unable to start FFmpeg");
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "FFmpeg could not be started".into(),
        )
    })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::internal("FFmpeg stdout unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::internal("FFmpeg stderr unavailable"))?;
    let (sender, receiver) = mpsc::channel(8);
    tokio::spawn(async move {
        let _permit = permit;
        let mut cancelled = false;
        let stderr_task = tokio::spawn(async move {
            let mut bytes = Vec::new();
            let _ = stderr.take(64 * 1024).read_to_end(&mut bytes).await;
            bytes
        });
        let mut buffer = vec![0; 64 * 1024];
        loop {
            match stdout.read(&mut buffer).await {
                Ok(0) => break,
                Ok(n) => {
                    if sender
                        .send(Ok::<_, Infallible>(axum::body::Bytes::copy_from_slice(
                            &buffer[..n],
                        )))
                        .await
                        .is_err()
                    {
                        cancelled = true;
                        let _ = child.kill().await;
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error,"Reading FFmpeg output failed");
                    let _ = child.kill().await;
                    break;
                }
            }
        }
        let status = child.wait().await;
        let stderr = stderr_task.await.unwrap_or_default();
        if !cancelled && !matches!(status,Ok(status) if status.success()) {
            tracing::warn!(status=?status, stderr=%String::from_utf8_lossy(&stderr),"FFmpeg playback ended with an error");
        }
    });
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCEPT_RANGES, "none")
        .body(Body::from_stream(futures_util::stream::unfold(
            receiver,
            |mut receiver| async move { receiver.recv().await.map(|item| (item, receiver)) },
        )))
        .map_err(Error::internal)
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct HlsQuery {
    mode: String,
    playback_ticket: String,
    play_session_id: String,
    video_codec: Option<String>,
    start_index: Option<u64>,
    audio_stream_index: Option<u64>,
}

async fn validate_audio_stream_index(
    state: &AppState,
    item: &str,
    index: Option<u64>,
) -> Result<()> {
    let Some(index) = index else {
        return Ok(());
    };
    let item = item.to_owned();
    let valid = state
        .db
        .call(move |c| {
            let json: String = c.query_row(
                "SELECT media_streams FROM items WHERE id=?1",
                [&item],
                |r| r.get(0),
            )?;
            let streams: Vec<Value> = serde_json::from_str(&json).unwrap_or_default();
            Ok(streams
                .iter()
                .any(|stream| stream["Type"] == "Audio" && stream["Index"].as_u64() == Some(index)))
        })
        .await?;
    if valid {
        Ok(())
    } else {
        Err(Error::bad("Invalid audio stream"))
    }
}

pub async fn hls_manifest(
    State(state): State<AppState>,
    Path(item): Path<String>,
    Query(query): Query<HlsQuery>,
    request: Request,
) -> Result<Response> {
    let _ = crate::tickets::authorize(&state, &item, request).await?;
    playback_mode(&query.mode)?;
    validate_audio_stream_index(&state, &item, query.audio_stream_index).await?;
    let runtime_ticks = state
        .db
        .call({
            let item = item.clone();
            move |c| {
                c.query_row("SELECT runtime_ticks FROM items WHERE id=?1", [item], |r| {
                    r.get::<_, Option<i64>>(0)
                })?
                .ok_or_else(|| Error::bad("Media duration is unavailable"))
            }
        })
        .await?;
    let segment_seconds = crate::transcode::segment_seconds() as f64;
    let duration = runtime_ticks as f64 / 10_000_000.0;
    let count = (duration / segment_seconds).ceil() as u64;
    let audio_query = query
        .audio_stream_index
        .map(|index| format!("&AudioStreamIndex={index}"))
        .unwrap_or_default();
    let mut playlist = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-TARGETDURATION:{}\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"init.mp4?PlaybackTicket={}&Mode={}&PlaySessionId={}&VideoCodec={}&StartIndex={}{}\"\n",
        segment_seconds as u64,
        query.playback_ticket,
        query.mode,
        query.play_session_id,
        query.video_codec.as_deref().unwrap_or(""),
        query.start_index.unwrap_or(0),
        audio_query
    );
    for index in 0..count {
        let remaining = duration - index as f64 * segment_seconds;
        let segment_duration = remaining.min(segment_seconds);
        playlist.push_str(&format!(
            "#EXTINF:{segment_duration:.3},\n{index}.m4s?PlaybackTicket={}&Mode={}&PlaySessionId={}&VideoCodec={}{}\n",
            query.playback_ticket, query.mode, query.play_session_id, query.video_codec.as_deref().unwrap_or(""),
            audio_query
        ));
    }
    playlist.push_str("#EXT-X-ENDLIST\n");
    Response::builder()
        .header(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(playlist))
        .map_err(Error::internal)
}

pub async fn hls_segment(
    State(state): State<AppState>,
    Path((item, segment)): Path<(String, String)>,
    Query(query): Query<HlsQuery>,
    request: Request,
) -> Result<Response> {
    let request = crate::tickets::authorize(&state, &item, request).await?;
    playback_mode(&query.mode)?;
    validate_audio_stream_index(&state, &item, query.audio_stream_index).await?;
    let resolved = if let Some(source) = crate::remote::ffmpeg_source(&state, &item).await? {
        Some(source)
    } else {
        crate::object_storage::ffmpeg_source(&state, &item).await?
    };
    let (source, source_headers) = if let Some(source) = resolved {
        (source.0.into(), source.1)
    } else {
        (
            media_path(&state, item.clone()).await?.into_os_string(),
            None,
        )
    };
    let session_request = || crate::transcode::SessionRequest {
        ffmpeg: &state.ffmpeg,
        gate: state.transcode_gate.clone(),
        session_id: &query.play_session_id,
        item: &item,
        mode: &query.mode,
        video_codec: query.video_codec.as_deref().unwrap_or(""),
        audio_stream_index: query.audio_stream_index,
        source: source.clone(),
        source_headers: source_headers.clone(),
    };
    let output = if segment == "init.mp4" {
        state
            .transcode_sessions
            .init_segment(session_request(), query.start_index.unwrap_or(0))
            .await?
    } else {
        let index = segment
            .strip_suffix(".m4s")
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| Error::bad("Invalid HLS segment"))?;
        tracing::debug!(%item, index, mode=%query.mode, "Serving managed HLS segment");
        state
            .transcode_sessions
            .segment(session_request(), index)
            .await?
    };
    let content_type = if segment == "init.mp4" {
        "video/mp4"
    } else {
        "video/iso.segment"
    };
    let mut response = ServeFile::new(output)
        .oneshot(request)
        .await
        .map_err(Error::internal)?
        .map(Body::new)
        .into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static(content_type),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}
// Resolve the catalog path again on every request so replaced symlinks cannot expose files outside the library.
pub async fn media_path(state: &AppState, item: String) -> Result<PathBuf> {
    let (path,root,root_identity)=state.db.call(move |c| {
        c.query_row("SELECT i.path,l.path,l.root_identity FROM items i JOIN libraries l ON l.id=i.library_id WHERE i.id=?1",[item],
            |r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?))).optional()?.ok_or_else(Error::missing)
    }).await?;
    let root = PathBuf::from(root);
    if tokio::fs::canonicalize(&root)
        .await
        .map_err(|_| Error::missing())?
        != root
    {
        return Err(Error::forbidden());
    }
    let metadata = tokio::fs::metadata(&root)
        .await
        .map_err(|_| Error::missing())?;
    if root_identity.is_some() && root_identity != crate::scanner::root_identity(&metadata) {
        return Err(Error::forbidden());
    }
    let resolved = tokio::fs::canonicalize(&path)
        .await
        .map_err(|_| Error::missing())?;
    if !resolved.starts_with(&root) {
        return Err(Error::forbidden());
    }
    if !tokio::fs::metadata(&resolved)
        .await
        .map_err(|_| Error::missing())?
        .is_file()
    {
        return Err(Error::missing());
    }
    Ok(resolved)
}
pub async fn stream(
    State(state): State<AppState>,
    Path(item): Path<String>,
    request: Request,
) -> Result<Response> {
    if crate::object_storage::is_item(&state, &item).await {
        return crate::object_storage::stream(State(state), Path(item), request).await;
    }
    let request = crate::tickets::authorize(&state, &item, request).await?;
    let path = media_path(&state, item).await?;
    let response = ServeFile::new(path)
        .oneshot(request)
        .await
        .map_err(Error::internal)?;
    Ok(response.map(Body::new).into_response())
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Progress {
    pub item_id: String,
    pub position_ticks: i64,
    pub play_session_id: Option<String>,
}
pub async fn progress(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<Progress>,
) -> Result<StatusCode> {
    if input.position_ticks < 0 {
        return Err(Error::bad("PositionTicks must not be negative"));
    }
    state.db.call(move |c| {
        let runtime:Option<i64>=c.query_row("SELECT runtime_ticks FROM items WHERE id=?1",[&input.item_id],|r|r.get(0)).optional()?.ok_or_else(Error::missing)?;
        let position=runtime.map_or(input.position_ticks,|r|input.position_ticks.min(r));
        c.execute("INSERT INTO user_data(user_id,item_id,position_ticks,updated_at) VALUES (?1,?2,?3,?4)
            ON CONFLICT(user_id,item_id) DO UPDATE SET position_ticks=excluded.position_ticks,updated_at=excluded.updated_at",
            params![auth.user.id,input.item_id,position,now()])?;
        Ok(())
    }).await?;
    Ok(StatusCode::NO_CONTENT)
}
pub async fn stopped(
    auth: Auth,
    State(state): State<AppState>,
    Json(input): Json<Progress>,
) -> Result<StatusCode> {
    if let Some(session) = input.play_session_id.as_deref() {
        state
            .transcode_sessions
            .stop_session(session, &input.item_id)
            .await;
    }
    progress(auth, State(state), Json(input)).await
}
async fn set_flag(
    auth: Auth,
    state: AppState,
    user: String,
    item: String,
    column: &'static str,
    value: bool,
) -> Result<Json<Value>> {
    auth.own(&user)?;
    state.db.call(move |c| {
        if !c.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=?1)",[&item],|r|r.get::<_,bool>(0))? {return Err(Error::missing());}
        // Column comes only from the four fixed handlers below, never from a request.
        c.execute(&format!("INSERT INTO user_data(user_id,item_id,{column},updated_at) VALUES (?1,?2,?3,?4)
            ON CONFLICT(user_id,item_id) DO UPDATE SET {column}=excluded.{column},updated_at=excluded.updated_at"),params![user,item,value,now()])?;
        let data=c.query_row("SELECT position_ticks,played,favorite FROM user_data WHERE user_id=?1 AND item_id=?2",params![user,item],|r|
            Ok(json!({"PlaybackPositionTicks":r.get::<_,i64>(0)?,"Played":r.get::<_,bool>(1)?,"IsFavorite":r.get::<_,bool>(2)?})))?;
        Ok(Json(data))
    }).await
}
pub async fn favorite(
    a: Auth,
    State(s): State<AppState>,
    Path((u, i)): Path<(String, String)>,
) -> Result<Json<Value>> {
    set_flag(a, s, u, i, "favorite", true).await
}
pub async fn unfavorite(
    a: Auth,
    State(s): State<AppState>,
    Path((u, i)): Path<(String, String)>,
) -> Result<Json<Value>> {
    set_flag(a, s, u, i, "favorite", false).await
}
pub async fn played(
    a: Auth,
    State(s): State<AppState>,
    Path((u, i)): Path<(String, String)>,
) -> Result<Json<Value>> {
    set_flag(a, s, u, i, "played", true).await
}
pub async fn unplayed(
    a: Auth,
    State(s): State<AppState>,
    Path((u, i)): Path<(String, String)>,
) -> Result<Json<Value>> {
    set_flag(a, s, u, i, "played", false).await
}

#[cfg(test)]
mod tests {
    use super::PlaybackPlan;
    use serde_json::json;

    fn streams(video: &str, audio: &str) -> Vec<serde_json::Value> {
        vec![
            json!({"Type":"Video","Codec":video}),
            json!({"Type":"Audio","Codec":audio}),
        ]
    }

    #[test]
    fn selects_the_least_expensive_browser_playback_plan() {
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mp4",
                &streams("h264", "aac"),
                false,
                false,
                false,
                false
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("h264", "aac"),
                false,
                false,
                false,
                false
            ),
            PlaybackPlan::Remux
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("h264", "aac"),
                false,
                true,
                false,
                false
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("h264", "eac3"),
                false,
                true,
                false,
                false
            ),
            PlaybackPlan::Audio
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("h264", "eac3"),
                false,
                true,
                false,
                true
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("hevc", "truehd"),
                false,
                true,
                false,
                false
            ),
            PlaybackPlan::Video
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mp4",
                &streams("hevc", "aac"),
                true,
                false,
                false,
                false
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("hevc", "aac"),
                true,
                false,
                false,
                false
            ),
            PlaybackPlan::Remux
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("hevc", "aac"),
                true,
                true,
                false,
                false
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Movie",
                "mkv",
                &streams("hevc", "aac"),
                false,
                true,
                false,
                false
            ),
            PlaybackPlan::Video
        );
        assert_eq!(
            PlaybackPlan::select(
                "Audio",
                "flac",
                &[json!({"Type":"Audio","Codec":"flac"})],
                false,
                false,
                false,
                false,
            ),
            PlaybackPlan::Direct
        );
        assert_eq!(
            PlaybackPlan::select(
                "Audio",
                "mka",
                &[json!({"Type":"Audio","Codec":"eac3"})],
                false,
                false,
                false,
                false,
            ),
            PlaybackPlan::Audio
        );
    }
}

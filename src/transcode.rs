use crate::error::{Error, Result};
use axum::http::StatusCode;
use std::{
    collections::HashMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant},
};
use tokio::{
    process::{Child, Command},
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::sleep,
};

const SEGMENT_SECONDS: u64 = 6;
const RESTART_GAP: u64 = 4;
const READ_AHEAD_SECONDS: u64 = 18;
const RETAIN_BEHIND_SEGMENTS: u64 = 10;
const DEFAULT_CACHE_LIMIT_MB: u64 = 512;
const MIB: u64 = 1024 * 1024;
static READRATE_BURST_SUPPORT: OnceLock<StdMutex<HashMap<String, bool>>> = OnceLock::new();

fn supports_initial_burst(ffmpeg: &str) -> bool {
    if Path::new(ffmpeg).file_name() != Some(std::ffi::OsStr::new("ffmpeg")) {
        return false;
    }
    let cache = READRATE_BURST_SUPPORT.get_or_init(|| StdMutex::new(HashMap::new()));
    if let Some(supported) = cache.lock().unwrap().get(ffmpeg).copied() {
        return supported;
    }
    let supported = std::process::Command::new(ffmpeg)
        .args(["-hide_banner", "-h", "full"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("-readrate_initial_burst")
        });
    cache.lock().unwrap().insert(ffmpeg.to_owned(), supported);
    supported
}

#[derive(Clone)]
pub struct TranscodeSessions {
    root: PathBuf,
    cache_limit: u64,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Session>>>>>,
}

struct Session {
    item: String,
    mode: String,
    video_codec: String,
    audio_stream_index: Option<u64>,
    directory: PathBuf,
    child: Option<Child>,
    permit: Option<OwnedSemaphorePermit>,
    first_segment: u64,
    generation: u64,
    last_access: Instant,
}

pub struct SessionRequest<'a> {
    pub ffmpeg: &'a str,
    pub gate: Arc<Semaphore>,
    pub session_id: &'a str,
    pub item: &'a str,
    pub mode: &'a str,
    pub video_codec: &'a str,
    pub audio_stream_index: Option<u64>,
    pub source: OsString,
    pub source_headers: Option<String>,
}

impl TranscodeSessions {
    pub fn new(data_dir: &Path) -> Self {
        let root = data_dir.join("transcodes");
        // Keep the root itself: deployments may mount a tmpfs here. The
        // instance lock is held, so no prior server can be writing to it.
        match std::fs::read_dir(&root) {
            Ok(entries) => {
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            let result = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                                std::fs::remove_dir_all(entry.path())
                            } else {
                                std::fs::remove_file(entry.path())
                            };
                            if let Err(error) = result {
                                tracing::warn!(%error, path=%entry.path().display(), "Could not clear old transcode file");
                            }
                        }
                        Err(error) => {
                            tracing::warn!(%error, "Could not inspect old transcode file")
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(%error, "Could not inspect transcode directory"),
        }
        let cache_limit = std::env::var("JELLYMAX_TRANSCODE_CACHE_MB")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value >= 64)
            .unwrap_or(DEFAULT_CACHE_LIMIT_MB)
            .saturating_mul(MIB);
        Self {
            root,
            cache_limit,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn segment(&self, request: SessionRequest<'_>, index: u64) -> Result<PathBuf> {
        self.segment_inner(request, index, false).await
    }

    async fn segment_inner(
        &self,
        request: SessionRequest<'_>,
        index: u64,
        init_only: bool,
    ) -> Result<PathBuf> {
        let SessionRequest {
            ffmpeg,
            gate,
            session_id,
            item,
            mode,
            video_codec,
            audio_stream_index,
            source,
            source_headers,
        } = request;
        validate_session_id(session_id)?;
        let session = {
            let mut sessions = self.sessions.lock().await;
            sessions
                .entry(session_id.to_owned())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(Session {
                        item: item.to_owned(),
                        mode: mode.to_owned(),
                        video_codec: video_codec.to_owned(),
                        audio_stream_index,
                        directory: self.root.join(session_id),
                        child: None,
                        permit: None,
                        first_segment: index,
                        generation: 0,
                        last_access: Instant::now(),
                    }))
                })
                .clone()
        };
        let mut current = session.lock().await;
        if current.item != item
            || current.mode != mode
            || current.video_codec != video_codec
            || current.audio_stream_index != audio_stream_index
        {
            return Err(Error::forbidden());
        }
        current.last_access = Instant::now();
        let target = if init_only {
            current.directory.join("init.mp4")
        } else {
            current.directory.join(format!("{index}.m4s"))
        };
        if output_ready(&target, &current.directory, index, init_only).await {
            reap_finished(&mut current)?;
            if !init_only {
                prune_segments(
                    &current.directory,
                    index.saturating_sub(RETAIN_BEHIND_SEGMENTS),
                )
                .await?;
            }
            return Ok(target);
        }
        let running = match current.child.as_mut() {
            Some(child) => child.try_wait().map_err(Error::internal)?.is_none(),
            None => false,
        };
        let (oldest, latest) = segment_bounds(&current.directory).await;
        let restart = !running
            || index < current.first_segment
            || oldest.is_some_and(|oldest| index < oldest)
            || index
                > latest
                    .unwrap_or(current.first_segment)
                    .saturating_add(RESTART_GAP);
        if restart {
            stop(&mut current).await;
            if tokio::fs::remove_dir_all(&current.directory).await.is_err() {
                // A missing directory is normal on the first request.
            }
            tokio::fs::create_dir_all(&current.directory)
                .await
                .map_err(Error::internal)?;
            if directory_size(&self.root).await? >= self.cache_limit {
                return Err(cache_full());
            }
            let permit = gate
                .acquire_owned()
                .await
                .map_err(|_| Error::internal("Transcoding queue is unavailable"))?;
            let child = start_ffmpeg(
                ffmpeg,
                (source, source_headers),
                &current.directory,
                mode,
                video_codec,
                audio_stream_index,
                index,
            )?;
            current.child = Some(child);
            current.permit = Some(permit);
            current.first_segment = index;
            current.generation += 1;
            schedule_cleanup(
                session.clone(),
                current.generation,
                self.root.clone(),
                self.cache_limit,
            );
        }
        let generation = current.generation;
        drop(current);

        for attempt in 0..1200 {
            if session.lock().await.generation != generation {
                return Err(Error(
                    StatusCode::CONFLICT,
                    "Playback seek superseded this segment request".into(),
                ));
            }
            if attempt % 4 == 0 && directory_size(&self.root).await? >= self.cache_limit {
                let mut current = session.lock().await;
                if current.generation != generation {
                    return Err(Error(
                        StatusCode::CONFLICT,
                        "Playback seek superseded this segment request".into(),
                    ));
                }
                stop(&mut current).await;
                let _ = tokio::fs::remove_dir_all(&current.directory).await;
                return Err(cache_full());
            }
            let directory = session.lock().await.directory.clone();
            if output_ready(&target, &directory, index, init_only).await {
                let mut current = session.lock().await;
                if current.generation != generation {
                    return Err(Error(
                        StatusCode::CONFLICT,
                        "Playback seek superseded this segment request".into(),
                    ));
                }
                current.last_access = Instant::now();
                reap_finished(&mut current)?;
                if !init_only {
                    prune_segments(
                        &current.directory,
                        index.saturating_sub(RETAIN_BEHIND_SEGMENTS),
                    )
                    .await?;
                }
                return Ok(target);
            }
            let (exited, exit_status) = {
                let mut current = session.lock().await;
                match current.child.as_mut() {
                    Some(child) => {
                        let status = child.try_wait().map_err(Error::internal)?;
                        (status.is_some(), status)
                    }
                    None => (true, None),
                }
            };
            if exited {
                let mut current = session.lock().await;
                if current.generation != generation {
                    return Err(Error(
                        StatusCode::CONFLICT,
                        "Playback seek superseded this segment request".into(),
                    ));
                }
                let log = tokio::fs::read_to_string(current.directory.join("ffmpeg.log"))
                    .await
                    .unwrap_or_default();
                let (oldest, latest) = segment_bounds(&current.directory).await;
                tracing::warn!(item=%current.item, mode=%current.mode, index, init_only, ?exit_status, ?oldest, ?latest, stderr=%log, "Managed FFmpeg session did not produce requested HLS output");
                current.child.take();
                current.permit.take();
                return Err(Error::internal(
                    "FFmpeg exited before producing the requested segment",
                ));
            }
            sleep(Duration::from_millis(50)).await;
        }
        let mut current = session.lock().await;
        if current.generation != generation {
            return Err(Error(
                StatusCode::CONFLICT,
                "Playback seek superseded this segment request".into(),
            ));
        }
        let log = tokio::fs::read_to_string(current.directory.join("ffmpeg.log"))
            .await
            .unwrap_or_default();
        let (oldest, latest) = segment_bounds(&current.directory).await;
        tracing::warn!(item=%current.item, mode=%current.mode, index, init_only, ?oldest, ?latest, stderr=%log, "Timed out waiting for FFmpeg output");
        stop(&mut current).await;
        Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Timed out waiting for FFmpeg output".into(),
        ))
    }

    pub async fn init_segment(
        &self,
        request: SessionRequest<'_>,
        requested_start: u64,
    ) -> Result<PathBuf> {
        validate_session_id(request.session_id)?;
        let session_id = request.session_id.to_owned();
        let path = self.root.join(&session_id).join("init.mp4");
        if let Some(session) = self.sessions.lock().await.get(&session_id).cloned() {
            let current = session.lock().await;
            if current.item != request.item
                || current.mode != request.mode
                || current.video_codec != request.video_codec
            {
                return Err(Error::forbidden());
            }
            drop(current);
            if tokio::fs::metadata(&path).await.is_ok() {
                return Ok(path);
            }
        }
        let start_index =
            if let Some(session) = self.sessions.lock().await.get(&session_id).cloned() {
                session.lock().await.first_segment
            } else {
                requested_start
            };
        let _ = self.segment_inner(request, start_index, true).await?;
        if tokio::fs::metadata(&path).await.is_ok() {
            Ok(path)
        } else {
            Err(Error::missing())
        }
    }

    pub async fn stop_session(&self, session_id: &str, item: &str) {
        let session = { self.sessions.lock().await.get(session_id).cloned() };
        let Some(session) = session else {
            return;
        };
        let mut current = session.lock().await;
        if current.item != item {
            return;
        }
        stop(&mut current).await;
        let _ = tokio::fs::remove_dir_all(&current.directory).await;
        drop(current);
        self.sessions.lock().await.remove(session_id);
    }
}

fn validate_session_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err(Error::bad("Invalid playback session"));
    }
    Ok(())
}

fn start_ffmpeg(
    ffmpeg: &str,
    input: (OsString, Option<String>),
    directory: &Path,
    mode: &str,
    video_codec: &str,
    audio_stream_index: Option<u64>,
    index: u64,
) -> Result<Child> {
    let (source, source_headers) = input;
    std::fs::create_dir_all(directory).map_err(Error::internal)?;
    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "warning", "-nostdin"]);
    if index > 0 {
        command.args(["-ss", &(index * SEGMENT_SECONDS).to_string()]);
    }
    if let Some(headers) = source_headers {
        command.args(["-headers", &headers]);
    }
    command.args(["-readrate", "1"]);
    if supports_initial_burst(ffmpeg) {
        command.args(["-readrate_initial_burst", &READ_AHEAD_SECONDS.to_string()]);
    }
    command.arg("-i").arg(source);
    command.args(["-map", "0:v:0?"]);
    if let Some(index) = audio_stream_index {
        command.args(["-map", &format!("0:{index}")]);
    } else {
        command.args(["-map", "0:a:0?"]);
    }
    match mode {
        "remux" => {
            command.args(["-c", "copy"]);
        }
        "audio" => {
            command.args(["-c:v", "copy", "-c:a", "aac", "-ac", "2", "-b:a", "192k"]);
        }
        "video" => {
            command.args([
            "-c:v", "libx264", "-preset", "ultrafast", "-tune", "zerolatency", "-crf", "24",
            "-vf", "scale=w='min(1920,iw)':h='min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2",
            "-pix_fmt", "yuv420p", "-c:a", "aac", "-ac", "2", "-b:a", "192k",
            "-force_key_frames", &format!("expr:gte(t,n_forced*{SEGMENT_SECONDS})"),
        ]);
        }
        _ => return Err(Error::bad("Invalid transcode mode")),
    }
    if matches!(mode, "remux" | "audio") && video_codec == "hevc" {
        command.args(["-tag:v", "hvc1"]);
    }
    let segment_pattern = directory.join("%d.m4s");
    let playlist = directory.join("main.m3u8");
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("ffmpeg.log"))
        .map_err(Error::internal)?;
    command
        .args([
            "-copyts",
            "-start_at_zero",
            "-avoid_negative_ts",
            "disabled",
            "-max_muxing_queue_size",
            "2048",
            "-f",
            "hls",
            "-hls_time",
            &SEGMENT_SECONDS.to_string(),
            "-hls_segment_type",
            "fmp4",
            "-hls_fmp4_init_filename",
            "init.mp4",
            "-start_number",
            &index.to_string(),
            "-hls_list_size",
            "0",
            "-hls_playlist_type",
            "vod",
            "-hls_flags",
            "independent_segments",
            "-hls_segment_filename",
        ])
        .arg(segment_pattern)
        .arg("-y")
        .arg(playlist)
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    command.spawn().map_err(|error| {
        tracing::error!(%error, executable=%ffmpeg, "Unable to start managed FFmpeg session");
        Error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "FFmpeg could not be started".into(),
        )
    })
}

async fn complete_segment(path: &Path, directory: &Path, index: u64) -> bool {
    let has_bytes = tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.len() > 0);
    has_bytes
        && (tokio::fs::metadata(directory.join(format!("{}.m4s", index + 1)))
            .await
            .is_ok()
            || tokio::fs::metadata(directory.join("main.m3u8"))
                .await
                .is_ok())
}

async fn output_ready(path: &Path, directory: &Path, index: u64, init_only: bool) -> bool {
    if init_only {
        tokio::fs::metadata(path)
            .await
            .is_ok_and(|metadata| metadata.len() > 0)
    } else {
        complete_segment(path, directory, index).await
    }
}

async fn segment_bounds(directory: &Path) -> (Option<u64>, Option<u64>) {
    let Ok(mut entries) = tokio::fs::read_dir(directory).await else {
        return (None, None);
    };
    let (mut oldest, mut latest) = (None, None);
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(index) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".m4s"))
            .and_then(|value| value.parse::<u64>().ok())
        {
            latest = Some(latest.map_or(index, |old: u64| old.max(index)));
            oldest = Some(oldest.map_or(index, |old: u64| old.min(index)));
        }
    }
    (oldest, latest)
}

async fn prune_segments(directory: &Path, before: u64) -> Result<()> {
    if before == 0 {
        return Ok(());
    }
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .map_err(Error::internal)?;
    while let Some(entry) = entries.next_entry().await.map_err(Error::internal)? {
        if let Some(index) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".m4s"))
            .and_then(|name| name.parse::<u64>().ok())
            && index < before
        {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }
    Ok(())
}

async fn stop(session: &mut Session) {
    if let Some(mut child) = session.child.take() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    session.permit.take();
}

fn schedule_cleanup(
    session: Arc<Mutex<Session>>,
    generation: u64,
    root: PathBuf,
    cache_limit: u64,
) {
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let mut session = session.lock().await;
            if session.generation != generation {
                return;
            }
            if reap_finished(&mut session).is_err() {
                return;
            }
            if directory_size(&root)
                .await
                .is_ok_and(|size| size >= cache_limit)
            {
                stop(&mut session).await;
                let _ = tokio::fs::remove_dir_all(&session.directory).await;
                return;
            }
            if session.last_access.elapsed() >= Duration::from_secs(60) {
                stop(&mut session).await;
                let _ = tokio::fs::remove_dir_all(&session.directory).await;
                return;
            }
        }
    });
}

fn cache_full() -> Error {
    Error(StatusCode::INSUFFICIENT_STORAGE, "Transcode cache limit reached; increase JELLYMAX_TRANSCODE_CACHE_MB or use direct playback".into())
}

async fn directory_size(root: &Path) -> Result<u64> {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let mut entries = match tokio::fs::read_dir(path).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(Error::internal(error)),
        };
        while let Some(entry) = entries.next_entry().await.map_err(Error::internal)? {
            let kind = entry.file_type().await.map_err(Error::internal)?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total =
                    total.saturating_add(entry.metadata().await.map_err(Error::internal)?.len());
            }
        }
    }
    Ok(total)
}

fn reap_finished(session: &mut Session) -> Result<()> {
    let finished = match session.child.as_mut() {
        Some(child) => child.try_wait().map_err(Error::internal)?.is_some(),
        None => false,
    };
    if finished {
        session.child.take();
        session.permit.take();
    }
    Ok(())
}

pub const fn segment_seconds() -> u64 {
    SEGMENT_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_distant_seek_replaces_an_unstarted_worker_without_waiting_for_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let fake = temp.path().join("fake-ffmpeg.sh");
        std::fs::write(
            &fake,
            r#"#!/bin/sh
previous=''
start=0
for argument in "$@"; do
  if [ "$previous" = '-start_number' ]; then start="$argument"; fi
  previous="$argument"
  last="$argument"
done
directory="${last%/*}"
printf init > "$directory/init.mp4"
sleep 0.3
printf segment > "$directory/$start.m4s"
next=$((start + 1))
printf segment > "$directory/$next.m4s"
printf '#EXTM3U\n#EXT-X-ENDLIST\n' > "$last"
"#,
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o700)).unwrap();
        let cache = TranscodeSessions::new(temp.path());
        let gate = Arc::new(Semaphore::new(1));
        let executable = fake.to_str().unwrap();
        let request = || SessionRequest {
            ffmpeg: executable,
            gate: gate.clone(),
            session_id: "seek",
            item: "movie",
            mode: "video",
            video_codec: "h264",
            audio_stream_index: None,
            source: OsString::from("ignored"),
            source_headers: None,
        };
        let init = cache.init_segment(request(), 0).await.unwrap();
        assert!(init.exists());
        let (old, new) = tokio::join!(cache.segment(request(), 0), async {
            sleep(Duration::from_millis(20)).await;
            cache.segment(request(), 7).await
        });
        assert_eq!(old.unwrap_err().0, StatusCode::CONFLICT);
        assert!(new.unwrap().ends_with("7.m4s"));
        cache.stop_session("seek", "movie").await;
    }

    #[tokio::test]
    async fn selected_audio_stream_is_used_for_hls() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("two-audio.mkv");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=size=64x64:rate=10",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=880:sample_rate=48000",
                "-t",
                "2",
                "-map",
                "0:v:0",
                "-map",
                "1:a:0",
                "-map",
                "2:a:0",
                "-c:v",
                "libx264",
                "-c:a",
                "aac",
                "-ac:a:0",
                "1",
                "-ac:a:1",
                "2",
                "-y",
            ])
            .arg(&input)
            .status()
            .unwrap();
        assert!(status.success());
        let output = temp.path().join("selected");
        let mut child = start_ffmpeg(
            "ffmpeg",
            (input.into_os_string(), None),
            &output,
            "remux",
            "h264",
            Some(2),
            0,
        )
        .unwrap();
        for _ in 0..160 {
            if output.join("0.m4s").exists() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(
            output.join("0.m4s").exists(),
            "selected soundtrack did not produce HLS output: {}",
            std::fs::read_to_string(output.join("ffmpeg.log")).unwrap_or_default()
        );
        if child.try_wait().unwrap().is_none() {
            child.kill().await.unwrap();
        }
        let _ = child.wait().await;
        let probe = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-show_entries",
                "stream=codec_type,channels",
                "-of",
                "csv=p=0",
            ])
            .arg(output.join("init.mp4"))
            .output()
            .unwrap();
        assert!(probe.status.success());
        let streams = String::from_utf8_lossy(&probe.stdout);
        assert!(
            streams
                .lines()
                .any(|line| line.contains("audio") && line.contains('2')),
            "wrong audio stream in HLS init: {streams}"
        );
        assert_eq!(
            streams
                .lines()
                .filter(|line| line.contains("audio"))
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn encoder_starts_fast_and_old_segments_are_pruned() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("source.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x90:rate=10",
                "-t",
                "45",
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-y",
            ])
            .arg(&input)
            .status()
            .unwrap();
        assert!(status.success());
        let output = temp.path().join("segments");
        let mut child = start_ffmpeg(
            "ffmpeg",
            (input.into_os_string(), None),
            &output,
            "video",
            "h264",
            None,
            0,
        )
        .unwrap();
        for _ in 0..100 {
            if output.join("1.m4s").exists() {
                break;
            }
            sleep(Duration::from_millis(50)).await;
        }
        assert!(
            output.join("1.m4s").exists(),
            "first segments were not available quickly"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "encoder stopped before playback advanced"
        );
        child.kill().await.unwrap();
        let _ = child.wait().await;
        for index in 0..15 {
            std::fs::write(output.join(format!("{index}.m4s")), b"segment").unwrap();
        }
        prune_segments(&output, 5).await.unwrap();
        assert!(!output.join("4.m4s").exists());
        assert!(output.join("5.m4s").exists());
        let stale = temp.path().join("transcodes/old.m4s");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, b"stale").unwrap();
        let _cache = TranscodeSessions::new(temp.path());
        assert!(!stale.exists());

        let cache = TranscodeSessions::new(temp.path());
        let source = temp.path().join("source.mp4").into_os_string();
        let segment = cache
            .segment(
                SessionRequest {
                    ffmpeg: "ffmpeg",
                    gate: Arc::new(Semaphore::new(1)),
                    session_id: "session",
                    item: "movie",
                    mode: "video",
                    video_codec: "h264",
                    audio_stream_index: None,
                    source,
                    source_headers: None,
                },
                0,
            )
            .await
            .unwrap();
        assert!(segment.exists());
        std::fs::remove_file(&segment).unwrap();
        let init = cache
            .init_segment(
                SessionRequest {
                    ffmpeg: "ffmpeg",
                    gate: Arc::new(Semaphore::new(1)),
                    session_id: "session",
                    item: "movie",
                    mode: "video",
                    video_codec: "h264",
                    audio_stream_index: None,
                    source: temp.path().join("source.mp4").into_os_string(),
                    source_headers: None,
                },
                0,
            )
            .await
            .unwrap();
        assert!(init.exists());
        assert!(
            !segment.exists(),
            "requesting the init file restarted encoding from zero"
        );
        cache.stop_session("session", "movie").await;
        assert!(!segment.exists());
    }
}

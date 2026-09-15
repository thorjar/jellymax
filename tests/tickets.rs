use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use jellymax::{AppState, auth::now, db::Database, router, tmdb::TmdbConfig};
use rusqlite::params;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tower::ServiceExt;

const TOKEN: &str = "first-user-account-token";
const OTHER_TOKEN: &str = "second-user-account-token";
const SUBTITLE: &[u8] = b"WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello\n";

struct Server {
    app: Router,
    state: AppState,
    _dir: TempDir,
}
impl Server {
    async fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let media = dir.path().join("media");
        std::fs::create_dir(&media).unwrap();
        let media = std::fs::canonicalize(media).unwrap();
        std::fs::write(media.join("movie.mp4"), b"0123456789").unwrap();
        std::fs::write(media.join("other.mp4"), b"other movie").unwrap();
        std::fs::write(media.join("song.mp3"), b"audio bytes").unwrap();
        std::fs::write(media.join("movie.en.vtt"), SUBTITLE).unwrap();
        let db = Database::open(&dir.path().join("test.db")).unwrap();
        db.call(move |c| {
            for (user, token) in [("first", TOKEN), ("second", OTHER_TOKEN)] {
                c.execute(
                    "INSERT INTO users(id,name,password_hash,is_admin) VALUES (?1,?1,'unused',0)",
                    [user],
                )?;
                c.execute(
                    "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?1,?2,?3)",
                    params![hex::encode(Sha256::digest(token)), user, now() + 86400],
                )?;
            }
            c.execute(
                "INSERT INTO libraries(id,name,path,kind) VALUES ('library','Movies',?1,'movies')",
                [media.to_string_lossy().as_ref()],
            )?;
            for (id, file, kind, container) in [
                ("movie", "movie.mp4", "Movie", "mp4"),
                ("other", "other.mp4", "Movie", "mp4"),
                ("song", "song.mp3", "Audio", "mp3"),
            ] {
                let streams = if kind == "Audio" {
                    r#"[{"Type":"Audio","Codec":"mp3"}]"#
                } else {
                    r#"[{"Type":"Video","Codec":"h264"},{"Type":"Audio","Codec":"aac"}]"#
                };
                c.execute(
                    "INSERT INTO items(id,library_id,path,name,kind,container,size,modified,media_streams,scan_id)
                     VALUES (?1,'library',?2,?1,?3,?4,10,0,?5,'scan')",
                    params![id, media.join(file).to_string_lossy().as_ref(), kind, container, streams],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
        let mut state = AppState::new(
            db,
            "Tickets".into(),
            "/nonexistent/ffprobe".into(),
            dir.path().to_path_buf(),
            TmdbConfig {
                api_key: None,
                language: "en-US".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        state.ffmpeg = "ffmpeg".into();
        Self {
            app: router(state.clone()),
            state,
            _dir: dir,
        }
    }

    async fn request(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        range: Option<&str>,
    ) -> Response {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header("X-Emby-Token", token);
        }
        if let Some(range) = range {
            request = request.header("Range", range);
        }
        self.app
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn info(&self, item: &str, token: &str) -> Value {
        let response = self
            .request(
                "POST",
                &format!("/Items/{item}/PlaybackInfo"),
                Some(token),
                None,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
    }

    async fn stream_url(&self, item: &str, token: &str) -> String {
        self.info(item, token).await["MediaSources"][0]["DirectStreamUrl"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    async fn first_hls_segment(&self, item: &str, manifest_url: &str) -> Vec<u8> {
        let response = self.request("GET", manifest_url, None, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        let manifest = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        assert!(manifest.contains("#EXT-X-PLAYLIST-TYPE:VOD"));
        assert!(manifest.contains("#EXT-X-MAP"));
        let segment = manifest
            .lines()
            .find(|line| line.contains(".m4s?"))
            .unwrap();
        let query = segment.split_once('?').unwrap().1;
        let init = self
            .request(
                "GET",
                &format!("/Videos/{item}/hls/init.mp4?{query}"),
                None,
                None,
            )
            .await;
        assert_eq!(init.status(), StatusCode::OK);
        let mut output = init
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        let response = self
            .request("GET", &format!("/Videos/{item}/hls/{segment}"), None, None)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        output.extend_from_slice(&response.into_body().collect().await.unwrap().to_bytes());
        output
    }
}

#[tokio::test]
async fn only_authenticated_accounts_can_issue_tickets() {
    let s = Server::new().await;
    for path in [
        "/Items/movie/PlaybackInfo",
        "/Videos/movie/stream",
        "/Items/movie/Download",
        "/Items/movie/Subtitles/10000",
    ] {
        assert_eq!(
            s.request("GET", path, None, None).await.status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        s.request(
            "GET",
            &format!("/Videos/movie/stream?api_key={TOKEN}"),
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let info = s.info("movie", TOKEN).await;
    assert!(!info.to_string().contains(TOKEN));
    let url = info["MediaSources"][0]["DirectStreamUrl"].as_str().unwrap();
    let ticket = url.split_once("PlaybackTicket=").unwrap().1;
    assert_eq!(ticket.len(), 64);
    let plaintext_ticket = ticket.to_owned();
    s.state
        .db
        .call(move |c| {
            let (hash, session, expiry): (String, String, i64) = c.query_row(
                "SELECT ticket_hash,session_hash,expires_at FROM playback_tickets",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_ne!(hash, plaintext_ticket);
            assert_eq!(hash, hex::encode(Sha256::digest(plaintext_ticket)));
            assert_eq!(session, hex::encode(Sha256::digest(TOKEN)));
            assert!(expiry > now() && expiry <= now() + 4 * 3600);
            Ok(())
        })
        .await
        .unwrap();
    for (method, path) in [
        ("GET", "/Items"),
        ("GET", "/Users/Me"),
        ("GET", "/Items/movie/PlaybackInfo"),
        ("GET", "/Items/movie/Subtitles"),
        ("GET", "/Items/movie/Images/Primary"),
        ("POST", "/Sessions/Logout"),
        ("POST", "/Users/first/FavoriteItems/movie"),
    ] {
        assert_eq!(
            s.request(
                method,
                &format!("{path}?PlaybackTicket={ticket}"),
                None,
                None
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED,
            "ticket unexpectedly authorized {method} {path}"
        );
    }
    assert_eq!(
        s.request("GET", "/Users/Me", Some(ticket), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn browser_tickets_support_ranges_head_audio_and_download() {
    let s = Server::new().await;
    let url = s.stream_url("movie", TOKEN).await;
    let response = s.request("GET", &url, None, Some("bytes=2-5")).await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(
        &response.into_body().collect().await.unwrap().to_bytes()[..],
        b"2345"
    );
    let response = s.request("HEAD", &url, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], "10");
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
    let query = url.split_once('?').unwrap().1;
    assert_eq!(
        s.request("GET", &format!("/Items/movie/Download?{query}"), None, None)
            .await
            .status(),
        StatusCode::OK
    );
    let song = s.stream_url("song", TOKEN).await;
    assert!(song.starts_with("/Audio/song/stream?PlaybackTicket="));
    assert_eq!(
        s.request("GET", &song, None, None).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        s.request("GET", &url, None, Some("bytes=100-200"))
            .await
            .status(),
        StatusCode::RANGE_NOT_SATISFIABLE
    );
}

#[tokio::test]
async fn tickets_are_item_scoped_and_invalid_values_are_rejected() {
    let s = Server::new().await;
    let url = s.stream_url("movie", TOKEN).await;
    let query = url.split_once('?').unwrap().1;
    for path in [
        "/Videos/other/stream",
        "/Items/other/Download",
        "/Items/other/Subtitles/10000",
        "/Videos/missing/stream",
    ] {
        assert_eq!(
            s.request("GET", &format!("{path}?{query}"), None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    for query in [
        "PlaybackTicket=".to_owned(),
        "PlaybackTicket=invalid".to_owned(),
        format!("PlaybackTicket={}", "a".repeat(64)),
        format!("{query}&{query}"),
    ] {
        assert_eq!(
            s.request("GET", &format!("/Videos/movie/stream?{query}"), None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[tokio::test]
async fn ticket_expiry_and_shortened_session_expiry_are_enforced() {
    let s = Server::new().await;
    let expired = s.stream_url("movie", TOKEN).await;
    s.state
        .db
        .call(|c| {
            c.execute("UPDATE playback_tickets SET expires_at=?1", [now()])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        s.request("GET", &expired, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let current = s.stream_url("movie", TOKEN).await;
    s.state
        .db
        .call(|c| {
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM playback_tickets", [], |r| r.get(0))?;
            assert_eq!(count, 1, "issuance must clean expired tickets");
            c.execute("UPDATE sessions SET expires_at=?1", [now()])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        s.request("GET", &current, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn ticket_lifetime_is_limited_to_issuing_session() {
    let s = Server::new().await;
    let session_expiry = now() + 60;
    s.state
        .db
        .call(move |c| {
            c.execute("UPDATE sessions SET expires_at=?1", [session_expiry])?;
            Ok(())
        })
        .await
        .unwrap();
    s.stream_url("movie", TOKEN).await;
    s.state
        .db
        .call(move |c| {
            let expiry: i64 =
                c.query_row("SELECT expires_at FROM playback_tickets", [], |r| r.get(0))?;
            assert_eq!(expiry, session_expiry);
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn logout_revokes_only_its_sessions_tickets() {
    let s = Server::new().await;
    let first = s.stream_url("movie", TOKEN).await;
    let second = s.stream_url("movie", OTHER_TOKEN).await;
    assert_ne!(first, second);
    assert_eq!(
        s.request("POST", "/Sessions/Logout", Some(TOKEN), None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        s.request("GET", &first, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.request("GET", &second, None, None).await.status(),
        StatusCode::OK
    );
    s.state
        .db
        .call(|c| {
            let count: i64 =
                c.query_row("SELECT COUNT(*) FROM playback_tickets", [], |r| r.get(0))?;
            assert_eq!(
                count, 1,
                "logout should delete ticket rows through the foreign key"
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn repeated_issuance_bounds_session_storage_and_evicts_oldest() {
    let s = Server::new().await;
    let oldest = s.stream_url("movie", TOKEN).await;
    let other_user = s.stream_url("movie", OTHER_TOKEN).await;
    let mut latest = String::new();
    for _ in 0..35 {
        latest = s.stream_url("movie", TOKEN).await;
    }
    s.state
        .db
        .call(|c| {
            let count: i64 = c.query_row(
                "SELECT COUNT(*) FROM playback_tickets WHERE session_hash=?1",
                [hex::encode(Sha256::digest(TOKEN))],
                |r| r.get(0),
            )?;
            assert_eq!(count, 32);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        s.request("GET", &oldest, None, None).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.request("GET", &latest, None, None).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        s.request("GET", &other_user, None, None).await.status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn eac3_video_transcodes_only_audio_to_browser_compatible_aac() {
    let s = Server::new().await;
    let input = s._dir.path().join("media/eac3.mkv");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=blue:s=160x90:r=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000",
            "-t",
            "0.5",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "eac3",
            "-ac",
            "6",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(generated.success());
    let input_string = input.to_string_lossy().into_owned();
    s.state.db.call(move |c| {
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id)
            VALUES ('eac3','library',?1,'E-AC-3','Movie','mkv',1,0,5000000,?2,'scan')",
            params![input_string, r#"[{"Type":"Video","Codec":"h264"},{"Type":"Audio","Codec":"eac3","Channels":6}]"#])?;
        Ok(())
    }).await.unwrap();

    let info = s.info("eac3", TOKEN).await;
    let source = &info["MediaSources"][0];
    assert_eq!(source["SupportsTranscoding"], true);
    assert_eq!(source["TranscodingMode"], "audio");
    let url = source["TranscodingUrl"].as_str().unwrap();
    let output = s.first_hls_segment("eac3", url).await;
    assert!(output.len() > 1000);
    let destination = s._dir.path().join("converted.ts");
    std::fs::write(&destination, output).unwrap();
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,codec_name,channels",
            "-of",
            "json",
        ])
        .arg(destination)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let result: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = result["streams"].as_array().unwrap();
    assert!(
        streams
            .iter()
            .any(|stream| stream["codec_type"] == "video" && stream["codec_name"] == "h264")
    );
    assert!(streams.iter().any(|stream| stream["codec_type"] == "audio"
        && stream["codec_name"] == "aac"
        && stream["channels"] == 2));
}

#[tokio::test]
async fn matroska_h264_aac_is_remuxed_without_reencoding() {
    let s = Server::new().await;
    let input = s._dir.path().join("media/remux.mkv");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=green:s=160x90:r=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=660:sample_rate=48000",
            "-t",
            "0.5",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(generated.success());
    let input_string = input.to_string_lossy().into_owned();
    s.state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id)
                 VALUES ('remux','library',?1,'Remux','Movie','mkv',1,0,5000000,?2,'scan')",
                params![input_string, r#"[{"Type":"Video","Codec":"h264"},{"Type":"Audio","Codec":"aac"}]"#],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let info = s.info("remux", TOKEN).await;
    let source = &info["MediaSources"][0];
    assert_eq!(source["TranscodingMode"], "remux");
    let output = s
        .first_hls_segment("remux", source["TranscodingUrl"].as_str().unwrap())
        .await;
    let destination = s._dir.path().join("remuxed.ts");
    std::fs::write(&destination, output).unwrap();
    assert_codecs(&destination, "h264", "aac");
}

#[tokio::test]
async fn sparse_keyframes_use_seekable_video_segments() {
    let mut s = Server::new().await;
    s.state.ffprobe = "ffprobe".into();
    s.app = router(s.state.clone());
    let input = s._dir.path().join("media/sparse.mkv");
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
            "-g",
            "100",
            "-keyint_min",
            "100",
            "-sc_threshold",
            "0",
            "-y",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    let input_string = input.to_string_lossy().into_owned();
    s.state.db.call(move |c| {
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id)
            VALUES ('sparse','library',?1,'Sparse','Movie','mkv',1,0,450000000,'[{\"Type\":\"Video\",\"Codec\":\"h264\"}]','scan')", [input_string])?;
        Ok(())
    }).await.unwrap();
    let info = s.info("sparse", TOKEN).await;
    assert_eq!(info["MediaSources"][0]["TranscodingMode"], "video");
    let safe = s._dir.path().join("media/short-gop.mkv");
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
            "20",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-g",
            "20",
            "-keyint_min",
            "20",
            "-sc_threshold",
            "0",
            "-y",
        ])
        .arg(&safe)
        .status()
        .unwrap();
    assert!(status.success());
    let safe_path = safe.to_string_lossy().into_owned();
    s.state.db.call(move |c| {
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id)
            VALUES ('short-gop','library',?1,'Short GOP','Movie','mkv',1,0,200000000,'[{\"Type\":\"Video\",\"Codec\":\"h264\"}]','scan')", [safe_path])?;
        Ok(())
    }).await.unwrap();
    assert_eq!(
        s.info("short-gop", TOKEN).await["MediaSources"][0]["TranscodingMode"],
        "remux"
    );
    let manifest_url = info["MediaSources"][0]["TranscodingUrl"].as_str().unwrap();
    let response = s.request("GET", manifest_url, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let manifest = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(manifest.contains("#EXT-X-INDEPENDENT-SEGMENTS"));
    let last = manifest
        .lines()
        .rfind(|line| line.contains(".m4s?"))
        .unwrap();
    assert!(last.starts_with("7.m4s?"));
    let response = s
        .request("GET", &format!("/Videos/sparse/hls/{last}"), None, None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len()
            > 1000
    );
}

#[tokio::test]
async fn unsupported_video_is_transcoded_to_h264_and_aac() {
    let s = Server::new().await;
    let input = s._dir.path().join("media/mpeg4.mkv");
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=red:s=160x90:r=10",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:sample_rate=48000",
            "-t",
            "0.5",
            "-c:v",
            "mpeg4",
            "-c:a",
            "aac",
        ])
        .arg(&input)
        .status()
        .unwrap();
    assert!(generated.success());
    let input_string = input.to_string_lossy().into_owned();
    s.state
        .db
        .call(move |c| {
            c.execute(
                "INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id)
                 VALUES ('mpeg4','library',?1,'MPEG-4','Movie','mkv',1,0,5000000,?2,'scan')",
                params![input_string, r#"[{"Type":"Video","Codec":"mpeg4"},{"Type":"Audio","Codec":"aac"}]"#],
            )?;
            Ok(())
        })
        .await
        .unwrap();

    let info = s.info("mpeg4", TOKEN).await;
    let source = &info["MediaSources"][0];
    assert_eq!(source["TranscodingMode"], "video");
    let output = s
        .first_hls_segment("mpeg4", source["TranscodingUrl"].as_str().unwrap())
        .await;
    let destination = s._dir.path().join("transcoded.ts");
    std::fs::write(&destination, output).unwrap();
    assert_codecs(&destination, "h264", "aac");
}

fn assert_codecs(path: &std::path::Path, video: &str, audio: &str) {
    let probe = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,codec_name",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        probe.status.success(),
        "{}",
        String::from_utf8_lossy(&probe.stderr)
    );
    let result: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = result["streams"].as_array().unwrap();
    assert!(
        streams
            .iter()
            .any(|stream| stream["codec_type"] == "video" && stream["codec_name"] == video)
    );
    assert!(
        streams
            .iter()
            .any(|stream| stream["codec_type"] == "audio" && stream["codec_name"] == audio)
    );
}

#[tokio::test]
async fn subtitle_delivery_uses_tickets_and_header_auth_still_works() {
    let s = Server::new().await;
    let info = s.info("movie", TOKEN).await;
    let url = info["MediaSources"][0]["MediaStreams"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stream| stream["IsExternal"] == true)
        .unwrap()["DeliveryUrl"]
        .as_str()
        .unwrap();
    assert!(url.starts_with("/Items/movie/Subtitles/10000?PlaybackTicket="));
    let response = s.request("GET", url, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &response.into_body().collect().await.unwrap().to_bytes()[..],
        SUBTITLE
    );
    for path in [
        "/Videos/movie/stream",
        "/Items/movie/Download",
        "/Items/movie/Subtitles/10000",
    ] {
        assert_eq!(
            s.request("GET", path, Some(TOKEN), None).await.status(),
            StatusCode::OK
        );
    }
    assert_eq!(
        s.request("GET", url, Some("invalid-account-token"), None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        s.request(
            "GET",
            "/Videos/movie/stream?PlaybackTicket=invalid",
            Some(TOKEN),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
}

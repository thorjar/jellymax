use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use jellymax::{AppState, auth, db::Database, router, tmdb::TmdbConfig};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct TestServer {
    app: Router,
    state: AppState,
    dir: TempDir,
    token: String,
    user: String,
}

#[tokio::test]
async fn first_run_setup_is_single_use_and_enables_login() {
    let dir = TempDir::new().unwrap();
    let db = Database::open(&dir.path().join("test.db")).unwrap();
    let state = AppState::new(
        db,
        "First run".into(),
        "ffprobe".into(),
        dir.path().to_path_buf(),
        TmdbConfig::default(),
    )
    .await
    .unwrap();
    let app = router(state);
    let (status, info) = send(&app, "GET", "/System/Info/Public", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(info["StartupWizardCompleted"], false);
    assert_eq!(
        send(
            &app,
            "POST",
            "/System/Setup",
            None,
            Some(json!({"Name":"admin","Password":"short"}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );

    let first = send(
        &app,
        "POST",
        "/System/Setup",
        None,
        Some(json!({"Name":"first","Password":"first-admin-password"})),
    );
    let second = send(
        &app,
        "POST",
        "/System/Setup",
        None,
        Some(json!({"Name":"second","Password":"second-admin-password"})),
    );
    let (first, second) = tokio::join!(first, second);
    assert!(matches!(
        (first.0, second.0),
        (StatusCode::CREATED, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::CREATED)
    ));
    let (name, password) = if first.0 == StatusCode::CREATED {
        ("first", "first-admin-password")
    } else {
        ("second", "second-admin-password")
    };
    let (_, info) = send(&app, "GET", "/System/Info/Public", None, None).await;
    assert_eq!(info["StartupWizardCompleted"], true);
    let (status, session) = send(
        &app,
        "POST",
        "/Users/AuthenticateByName",
        None,
        Some(json!({"Username":name,"Pw":password})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(session["User"]["Policy"]["IsAdministrator"], true);
    assert_eq!(
        send(
            &app,
            "POST",
            "/System/Setup",
            None,
            Some(json!({"Name":"third","Password":"third-admin-password"}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn remote_playback_uses_local_hls_and_static_upstream_input() {
    let s = TestServer::new().await;
    s.state.db.call(|c| {
        c.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id) VALUES ('remote','Remote','http://127.0.0.1:1/jellyfin','upstream','remote-user','secret','device')", [])?;
        c.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES ('remote-library','Remote','remote://library','movies','remote','view')", [])?;
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,remote_server_id,remote_item_id) VALUES ('remote-movie','remote-library','remote://movie','Movie','Movie','mkv',100,0,600000000,'[{\"Type\":\"Video\",\"Codec\":\"hevc\"},{\"Type\":\"Audio\",\"Codec\":\"aac\"}]','scan','remote','upstream-movie')", [])?;
        Ok(())
    }).await.unwrap();
    let (status, info) = s
        .call(
            "POST",
            "/Items/remote-movie/PlaybackInfo?SupportsHevc=false",
            Some(json!({})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let source = &info["MediaSources"][0];
    assert_eq!(source["SupportsDirectPlay"], false);
    assert!(
        source["TranscodingUrl"]
            .as_str()
            .unwrap()
            .starts_with("/Videos/remote-movie/hls/master.m3u8")
    );
    assert!(
        source["DirectStreamUrl"]
            .as_str()
            .unwrap()
            .starts_with("/RemoteItems/remote-movie/stream")
    );
}

#[tokio::test]
async fn remote_external_srt_is_fetched_in_original_format_and_served_as_vtt() {
    let upstream = Router::new()
        .route("/Items/abcd/PlaybackInfo", axum::routing::get(|| async {
            axum::Json(json!({"MediaSources":[{"Id":"source","MediaStreams":[{"Type":"Subtitle","Index":2,"IsExternal":true}]}]}))
        }))
        .route("/Videos/abcd/source/Subtitles/2/Stream.srt", axum::routing::get(|| async {
            "1\n00:00:00,000 --> 00:00:01,000\nRemote subtitle\n"
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let s = TestServer::new().await;
    s.state.db.call(move |c| {
        c.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id) VALUES ('remote','Remote',?1,'upstream','remote-user','secret','device')", [format!("http://{address}")])?;
        c.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES ('remote-library','Remote','remote://library','movies','remote','view')", [])?;
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,remote_server_id,remote_item_id) VALUES ('remote-movie','remote-library','remote://movie','Movie','Movie','mp4',100,0,10000000,'[{\"Type\":\"Video\",\"Codec\":\"h264\"},{\"Type\":\"Subtitle\",\"Index\":2,\"Codec\":\"SubRip\",\"IsExternal\":true}]','scan','remote','abcd')", [])?;
        Ok(())
    }).await.unwrap();
    let (_, info) = s
        .call("POST", "/Items/remote-movie/PlaybackInfo", Some(json!({})))
        .await;
    let url = info["MediaSources"][0]["MediaStreams"][1]["DeliveryUrl"]
        .as_str()
        .unwrap();
    let request = Request::builder()
        .uri(url)
        .header("X-Emby-Token", &s.token)
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("WEBVTT\n\n1\n00:00:00.000 --> 00:00:01.000")
    );
    server.abort();
}

#[tokio::test]
async fn remote_embedded_subrip_uses_the_media_source_that_advertised_it() {
    let dir = TempDir::new().unwrap();
    let subtitle = dir.path().join("track.srt");
    let media = dir.path().join("movie.mkv");
    std::fs::write(
        &subtitle,
        "1\n00:00:01,000 --> 00:00:03,000\nSelected source caption\n",
    )
    .unwrap();
    let generated = std::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=32x32:r=1",
            "-i",
        ])
        .arg(&subtitle)
        .args(["-t", "4", "-c:v", "mpeg4", "-c:s", "srt"])
        .arg(&media)
        .status()
        .unwrap();
    assert!(generated.success());
    let bytes = std::fs::read(media).unwrap();
    let upstream = Router::new()
        .route("/Items/abcd/PlaybackInfo", axum::routing::get(|| async {
            axum::Json(json!({"MediaSources":[
                {"Id":"wrong-source","MediaStreams":[{"Type":"Subtitle","Index":4,"Codec":"ass","IsExternal":false}]},
                {"Id":"selected-source","MediaStreams":[{"Type":"Subtitle","Index":2,"Codec":"subrip","IsExternal":true},{"Type":"Subtitle","Index":4,"Codec":"subrip","IsExternal":false}]}
            ]}))
        }))
        .route("/Videos/abcd/stream", axum::routing::get(move |axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>| {
            let bytes = bytes.clone();
            async move {
                if query.get("MediaSourceId").map(String::as_str) == Some("selected-source") {
                    Ok::<_, StatusCode>(bytes)
                } else {
                    Err(StatusCode::NOT_FOUND)
                }
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let mut s = TestServer::new().await;
    s.state.ffmpeg = "ffmpeg".into();
    s.app = router(s.state.clone());
    s.state.db.call(move |c| {
        c.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id) VALUES ('remote','Remote',?1,'upstream','remote-user','secret','device')", [format!("http://{address}")])?;
        c.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES ('remote-library','Remote','remote://library','movies','remote','view')", [])?;
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,remote_server_id,remote_item_id) VALUES ('remote-movie','remote-library','remote://movie','Movie','Movie','mkv',100,0,40000000,'[{\"Type\":\"Video\",\"Index\":0,\"Codec\":\"mpeg4\"},{\"Type\":\"Subtitle\",\"Index\":2,\"Codec\":\"SubRip\",\"IsExternal\":true},{\"Type\":\"Subtitle\",\"Index\":4,\"Codec\":\"SubRip\",\"IsExternal\":false}]','scan','remote','abcd')", [])?;
        Ok(())
    }).await.unwrap();
    let request = Request::builder()
        .uri("/Items/remote-movie/Subtitles/4?StartSeconds=0")
        .header("X-Emby-Token", &s.token)
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let output = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&output)
    );
    assert!(
        std::str::from_utf8(&output)
            .unwrap()
            .contains("Selected source caption")
    );
    server.abort();
}

#[cfg(unix)]
#[tokio::test]
async fn embedded_subtitle_response_sends_first_cue_before_extraction_finishes() {
    use std::os::unix::fs::PermissionsExt;
    let upstream = Router::new()
        .route("/Items/abcd/PlaybackInfo", axum::routing::get(|| async {
            axum::Json(json!({"MediaSources":[{"Id":"source","MediaStreams":[{"Type":"Subtitle","Index":2,"Codec":"subrip","IsExternal":false}]}]}))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let mut s = TestServer::new().await;
    let script = s.dir.path().join("slow-ffmpeg.sh");
    std::fs::write(&script, "#!/bin/sh\nprintf 'WEBVTT\\n\\n00:00:01.000 --> 00:00:04.000\\nFirst caption\\n\\n'\nsleep 2\nprintf '00:00:05.000 --> 00:00:08.000\\nSecond caption\\n\\n'\n").unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    s.state.ffmpeg = script.to_string_lossy().into_owned();
    s.app = router(s.state.clone());
    s.state.db.call(move |c| {
        c.execute("INSERT INTO remote_servers(id,name,base_url,server_id,user_id,access_token,device_id) VALUES ('remote','Remote',?1,'upstream','remote-user','secret','device')", [format!("http://{address}")])?;
        c.execute("INSERT INTO libraries(id,name,path,kind,remote_server_id,remote_item_id) VALUES ('remote-library','Remote','remote://library','movies','remote','view')", [])?;
        c.execute("INSERT INTO items(id,library_id,path,name,kind,container,size,modified,runtime_ticks,media_streams,scan_id,remote_server_id,remote_item_id) VALUES ('remote-movie','remote-library','remote://movie','Movie','Movie','mkv',100,0,100000000,'[{\"Type\":\"Subtitle\",\"Index\":2,\"Codec\":\"subrip\",\"IsExternal\":false}]','scan','remote','abcd')", [])?;
        Ok(())
    }).await.unwrap();
    let request = Request::builder()
        .uri("/Items/remote-movie/Subtitles/2?StartSeconds=0")
        .header("X-Emby-Token", &s.token)
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut chunks = response.into_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_millis(800), chunks.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        std::str::from_utf8(&first)
            .unwrap()
            .contains("First caption")
    );
    let second = tokio::time::timeout(std::time::Duration::from_secs(4), chunks.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        std::str::from_utf8(&second)
            .unwrap()
            .contains("Second caption")
    );
    server.abort();
}
impl TestServer {
    async fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let db = Database::open(&dir.path().join("test.db")).unwrap();
        let user = auth::add_user(&db, "admin".into(), "long-test-password".into(), true)
            .await
            .unwrap();
        let state = AppState::new(
            db,
            "Test".into(),
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
        let app = router(state.clone());
        let (_, body) = send(
            &app,
            "POST",
            "/Users/AuthenticateByName",
            None,
            Some(json!({"Username":"ADMIN","Pw":"long-test-password"})),
        )
        .await;
        Self {
            app,
            state,
            dir,
            token: body["AccessToken"].as_str().unwrap().into(),
            user: user.id,
        }
    }
    async fn call(&self, method: &str, path: &str, body: Option<Value>) -> (StatusCode, Value) {
        send(&self.app, method, path, Some(&self.token), body).await
    }
    async fn library(&self) -> String {
        let media = self.dir.path().join("movies");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("Example.Movie.mp4"), b"0123456789").unwrap();
        let (status, result) = self
            .call(
                "POST",
                "/Library/VirtualFolders",
                Some(json!({"Name":"Movies","CollectionType":"movies","Locations":[media]})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        result["Id"].as_str().unwrap().into()
    }
    async fn scan(&self) -> Value {
        assert_eq!(
            self.call("POST", "/Library/Refresh", None).await.0,
            StatusCode::ACCEPTED
        );
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let status = self.call("GET", "/ScheduledTasks", None).await.1;
                if status[0]["State"] != "Running" {
                    return status[0].clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    async fn item(&self) -> String {
        self.library().await;
        assert_eq!(self.scan().await["State"], "Completed");
        self.call("GET", "/Items", None).await.1["Items"][0]["Id"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn other_user(&self) -> (String, String) {
        let (status, user) = self
            .call(
                "POST",
                "/Users/New",
                Some(json!({"Name":"viewer","Password":"viewer-password-123"})),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED);
        let (status, session) = send(
            &self.app,
            "POST",
            "/Users/AuthenticateByName",
            None,
            Some(json!({"Username":"viewer","Pw":"viewer-password-123"})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        (
            user["Id"].as_str().unwrap().into(),
            session["AccessToken"].as_str().unwrap().into(),
        )
    }
}
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        req = req.header("X-Emby-Token", token);
    }
    let body = if let Some(body) = body {
        req = req.header("Content-Type", "application/json");
        Body::from(body.to_string())
    } else {
        Body::empty()
    };
    let response = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn authentication_logout_and_persistent_identity() {
    let s = TestServer::new().await;
    assert_eq!(
        send(&s.app, "GET", "/Items", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &s.app,
            "POST",
            "/Users/AuthenticateByName",
            None,
            Some(json!({"Username":"admin","Pw":"wrong"}))
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(s.call("GET", "/Users/Me", None).await.1["Id"], s.user);
    let reopened = Database::open(&s.dir.path().join("test.db")).unwrap();
    let state = AppState::new(
        reopened,
        "Again".into(),
        "ffprobe".into(),
        s.dir.path().to_path_buf(),
        TmdbConfig {
            api_key: None,
            language: "en-US".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(state.server_id, s.state.server_id);
    assert_eq!(
        send(&router(state), "GET", "/Users/Me", Some(&s.token), None)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        s.call("POST", "/Sessions/Logout", None).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        s.call("GET", "/Users/Me", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn expired_sessions_are_rejected_and_tokens_are_hashed() {
    let s = TestServer::new().await;
    let token = s.token.clone();
    s.state
        .db
        .call(move |c| {
            let hash: String = c.query_row("SELECT token_hash FROM sessions", [], |r| r.get(0))?;
            assert_ne!(hash, token);
            c.execute("UPDATE sessions SET expires_at=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        s.call("GET", "/Items", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn scan_search_pagination_and_rescan_keep_ids() {
    let s = TestServer::new().await;
    let library = s.library().await;
    assert_eq!(s.scan().await["Scanned"], 1);
    let (_, items) = s
        .call(
            "GET",
            &format!("/Items?ParentId={library}&SearchTerm=movie&Limit=1"),
            None,
        )
        .await;
    assert_eq!(items["TotalRecordCount"], 1);
    assert_eq!(items["Items"][0]["Name"], "Example Movie");
    assert!(items["Items"][0].get("Path").is_none());
    let item = items["Items"][0]["Id"].clone();
    s.scan().await;
    assert_eq!(
        s.call("GET", "/Items", None).await.1["Items"][0]["Id"],
        item
    );
    let (_, page) = s.call("GET", "/Items?StartIndex=1&Limit=1", None).await;
    assert_eq!(page["TotalRecordCount"], 1);
    assert_eq!(page["Items"].as_array().unwrap().len(), 0);
    assert_eq!(
        s.call("GET", "/Items?SearchTerm=%27%20OR%201%3D1--", None)
            .await
            .1["TotalRecordCount"],
        0
    );
    assert_eq!(
        s.call("GET", "/Items?Limit=-1", None).await.0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn missing_root_does_not_prune_but_deleted_files_do() {
    let s = TestServer::new().await;
    s.item().await;
    let media = s.dir.path().join("movies");
    let offline = s.dir.path().join("offline");
    std::fs::rename(&media, &offline).unwrap();
    assert_eq!(s.scan().await["State"], "Failed");
    assert_eq!(s.call("GET", "/Items", None).await.1["TotalRecordCount"], 1);
    std::fs::rename(&offline, &media).unwrap();
    std::fs::remove_file(media.join("Example.Movie.mp4")).unwrap();
    assert_eq!(s.scan().await["Removed"], 1);
    assert_eq!(s.call("GET", "/Items", None).await.1["TotalRecordCount"], 0);
}
#[tokio::test]
async fn streaming_supports_ranges_head_and_authentication() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let path = format!("/Videos/{item}/stream");
    assert_eq!(
        send(&s.app, "GET", &path, None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let request = Request::builder()
        .uri(&path)
        .header("X-Emby-Token", &s.token)
        .header("Range", "bytes=2-5")
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()["content-range"], "bytes 2-5/10");
    assert_eq!(
        &response.into_body().collect().await.unwrap().to_bytes()[..],
        b"2345"
    );
    let request = Request::builder()
        .method("HEAD")
        .uri(&path)
        .header("Authorization", format!("Bearer {}", s.token))
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
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
    let request = Request::builder()
        .uri(&path)
        .header("X-Emby-Token", &s.token)
        .header("Range", "bytes=100-200")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        s.app.clone().oneshot(request).await.unwrap().status(),
        StatusCode::RANGE_NOT_SATISFIABLE
    );
}
#[tokio::test]
async fn user_data_is_private_and_survives_rescanning() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let (other, token) = s.other_user().await;
    assert_eq!(
        s.call(
            "POST",
            "/Sessions/Playing/Progress",
            Some(json!({"ItemId":item,"PositionTicks":12345}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    s.call(
        "POST",
        &format!("/Users/{}/FavoriteItems/{item}", s.user),
        None,
    )
    .await;
    s.scan().await;
    let (_, data) = s.call("GET", &format!("/Items/{item}"), None).await;
    assert_eq!(data["UserData"]["PlaybackPositionTicks"], 12345);
    assert_eq!(data["UserData"]["IsFavorite"], true);
    assert_eq!(
        s.call("GET", &format!("/Users/{}/Items/Resume", s.user), None)
            .await
            .1["TotalRecordCount"],
        1
    );
    let (_, data) = send(&s.app, "GET", &format!("/Items/{item}"), Some(&token), None).await;
    assert_eq!(data["UserData"]["PlaybackPositionTicks"], 0);
    assert_eq!(
        s.call("GET", &format!("/Items?UserId={other}"), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(
            &s.app,
            "POST",
            &format!("/Users/{}/FavoriteItems/{item}", s.user),
            Some(&token),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    s.call(
        "POST",
        &format!("/Users/{}/PlayedItems/{item}", s.user),
        None,
    )
    .await;
    assert_eq!(
        s.call("GET", &format!("/Users/{}/Items/Resume", s.user), None)
            .await
            .1["TotalRecordCount"],
        0
    );
    assert_eq!(
        s.call(
            "POST",
            "/Sessions/Playing/Progress",
            Some(json!({"ItemId":item,"PositionTicks":-1}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}
#[tokio::test]
async fn created_users_are_immediately_listed_on_both_users_routes() {
    let s = TestServer::new().await;
    let (status, created) = s
        .call(
            "POST",
            "/Users/New",
            Some(json!({"Name":"listed-viewer","Password":"viewer-password-123"})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);

    for route in ["/Users", "/Users/"] {
        let (status, users) = s.call("GET", route, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(users.as_array().unwrap().iter().any(|user| {
            user["Id"] == created["Id"] && user["Name"] == "listed-viewer"
        }));
    }
}

#[tokio::test]
async fn administration_is_restricted_and_libraries_cannot_overlap() {
    let s = TestServer::new().await;
    let library = s.library().await;
    let (_, token) = s.other_user().await;
    assert_eq!(
        send(&s.app, "POST", "/Library/Refresh", Some(&token), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&s.app, "GET", "/Users", Some(&token), None).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&s.app, "GET", "/Library/VirtualFolders", Some(&token), None)
            .await
            .1[0]["Locations"],
        json!([])
    );
    assert_eq!(
        s.call(
            "POST",
            "/Library/VirtualFolders",
            Some(json!({"Name":"Duplicate","CollectionType":"movies","Locations":[s.dir.path()]}))
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &s.app,
            "DELETE",
            &format!("/Library/VirtualFolders/{library}"),
            Some(&token),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
#[tokio::test]
async fn playlists_are_ordered_private_and_transactional() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let (_, token) = s.other_user().await;
    let (status, body) = s
        .call(
            "POST",
            "/Playlists",
            Some(json!({"Name":"Queue","Ids":[item,item]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let playlist = body["Id"].as_str().unwrap();
    let path = format!("/Playlists/{playlist}/Items");
    assert_eq!(s.call("GET", &path, None).await.1["TotalRecordCount"], 2);
    assert_eq!(
        send(&s.app, "GET", &path, Some(&token), None).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        s.call("POST", &path, Some(json!({"Ids":[item,"missing"]})))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(s.call("GET", &path, None).await.1["TotalRecordCount"], 2);
    assert_eq!(
        s.call(
            "POST",
            "/Playlists",
            Some(json!({"Name":"Invalid","Ids":["missing"]}))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        s.call("GET", "/Playlists", None).await.1["TotalRecordCount"],
        1
    );
}
#[tokio::test]
async fn concurrent_scans_are_rejected() {
    let s = TestServer::new().await;
    let _permit = s.state.scan_gate.clone().acquire_owned().await.unwrap();
    assert_eq!(
        s.call("POST", "/Library/Refresh", None).await.0,
        StatusCode::CONFLICT
    );
}
#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_expose_media_outside_the_library() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let outside = s.dir.path().join("private.mp4");
    std::fs::write(&outside, b"secret").unwrap();
    let media = s.dir.path().join("movies");
    std::os::unix::fs::symlink(&outside, media.join("link.mp4")).unwrap();
    s.scan().await;
    assert_eq!(s.call("GET", "/Items", None).await.1["TotalRecordCount"], 1);
    std::fs::remove_file(media.join("Example.Movie.mp4")).unwrap();
    std::os::unix::fs::symlink(&outside, media.join("Example.Movie.mp4")).unwrap();
    assert_eq!(
        s.call("GET", &format!("/Videos/{item}/stream"), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn local_artwork_and_subtitles_are_authenticated_and_discovered() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let media = s.dir.path().join("movies");
    std::fs::write(media.join("poster.jpg"), b"test-image").unwrap();
    let subtitle = b"WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nHello\n";
    std::fs::write(media.join("Example.Movie.en.vtt"), subtitle).unwrap();
    std::fs::write(media.join("Unrelated.en.vtt"), b"unrelated").unwrap();
    assert_eq!(
        send(
            &s.app,
            "GET",
            &format!("/Items/{item}/Images/Primary"),
            None,
            None
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let request = Request::builder()
        .uri(format!("/Items/{item}/Images/Primary"))
        .header(
            "X-Emby-Authorization",
            format!("MediaBrowser Client=\"Test\", Token=\"{}\"", s.token),
        )
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &response.into_body().collect().await.unwrap().to_bytes()[..],
        b"test-image"
    );
    let (_, subtitles) = s
        .call("GET", &format!("/Items/{item}/Subtitles"), None)
        .await;
    assert_eq!(subtitles.as_array().unwrap().len(), 1);
    assert_eq!(subtitles[0]["Language"], "en");
    let delivery = subtitles[0]["DeliveryUrl"].as_str().unwrap();
    let request = Request::builder()
        .uri(delivery)
        .header("X-Emby-Token", &s.token)
        .body(Body::empty())
        .unwrap();
    let response = s.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        &response.into_body().collect().await.unwrap().to_bytes()[..],
        subtitle
    );
    let (_, playback) = s
        .call(
            "POST",
            &format!("/Items/{item}/PlaybackInfo"),
            Some(json!({})),
        )
        .await;
    // The deliberately invalid media fixture cannot be probed, so playback
    // correctly advertises the safe full-transcode fallback.
    assert_eq!(playback["MediaSources"][0]["SupportsTranscoding"], true);
    assert_eq!(
        playback["MediaSources"][0]["MediaStreams"][0]["IsExternal"],
        true
    );
    assert_eq!(
        s.call("GET", &format!("/Items/{item}/Subtitles/9999"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[cfg(unix)]
#[tokio::test]
async fn sidecar_symlinks_cannot_read_private_files() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let secret = s.dir.path().join("secret");
    std::fs::write(&secret, b"private").unwrap();
    let media = s.dir.path().join("movies");
    std::os::unix::fs::symlink(&secret, media.join("poster.jpg")).unwrap();
    std::os::unix::fs::symlink(&secret, media.join("Example.Movie.en.vtt")).unwrap();
    assert_eq!(
        s.call("GET", &format!("/Items/{item}/Images/Primary"), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        s.call("GET", &format!("/Items/{item}/Subtitles"), None)
            .await
            .1,
        json!([])
    );
}

#[tokio::test]
async fn progress_is_clamped_to_known_duration() {
    let s = TestServer::new().await;
    let item = s.item().await;
    let item_id = item.clone();
    s.state
        .db
        .call(move |c| {
            c.execute("UPDATE items SET runtime_ticks=100 WHERE id=?1", [item_id])?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        s.call(
            "POST",
            "/Sessions/Playing/Stopped",
            Some(json!({"ItemId":item,"PositionTicks":1000}))
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        s.call("GET", &format!("/Items/{item}"), None).await.1["UserData"]["PlaybackPositionTicks"],
        100
    );
}

#[tokio::test]
async fn login_is_throttled_before_password_work() {
    let s = TestServer::new().await;
    s.state.login_window.lock().unwrap().1 = 30;
    assert_eq!(
        send(
            &s.app,
            "POST",
            "/Users/AuthenticateByName",
            None,
            Some(json!({"Username":"admin","Pw":"long-test-password"}))
        )
        .await
        .0,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[test]
fn future_database_versions_are_not_overwritten() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("future.db");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=999").unwrap();
    assert!(Database::open(&path).is_err());
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 999);
}
#[tokio::test]
async fn directory_browser_is_admin_only_and_lists_subdirectories() {
    let s = TestServer::new().await;
    let (viewer, viewer_token) = s.other_user().await;
    let base = s.dir.path().display().to_string();
    assert_eq!(
        send(
            &s.app,
            "GET",
            &format!("/Library/Paths?Path={base}"),
            Some(&viewer_token),
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let _ = viewer;
    std::fs::create_dir_all(s.dir.path().join("a_dir")).unwrap();
    std::fs::create_dir_all(s.dir.path().join("b_dir")).unwrap();
    std::fs::write(s.dir.path().join("ignored.txt"), b"x").unwrap();
    let (status, listing) = s
        .call("GET", &format!("/Library/Paths?Path={base}"), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listing["Directories"][0]["Name"], "a_dir");
    assert_eq!(listing["Directories"][1]["Name"], "b_dir");
    assert_eq!(
        listing["Directories"].as_array().unwrap().len(),
        2,
        "files are not directories and must be excluded"
    );
    assert_eq!(
        s.call("GET", "/Library/Paths?Path=%2Fmissing%2Fdirectory", None)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (status, listing) = s.call("GET", "/Library/Paths", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(listing["Path"].is_string());
}

#[tokio::test]
async fn series_seasons_and_episodes_are_persistent_and_numerically_ordered() {
    let s = TestServer::new().await;
    let root = s.dir.path().join("series");
    for file in [
        "Example Show/Season 01/S01E10.mp4",
        "Example Show/Season 01/S01E02.mp4",
        "Example Show/Season 02/Example.Show.S02E01.mp4",
        "Example Show/Specials/Example.Show.S00E01.mp4",
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"media").unwrap();
    }
    let (status, library) = s
        .call(
            "POST",
            "/Library/VirtualFolders",
            Some(json!({"Name":"Series","CollectionType":"tvshows","Locations":[root]})),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(s.scan().await["State"], "Completed");
    let top = s
        .call(
            "GET",
            &format!("/Items?ParentId={}", library["Id"].as_str().unwrap()),
            None,
        )
        .await
        .1;
    assert_eq!(top["TotalRecordCount"], 1);
    let series = &top["Items"][0];
    assert_eq!(series["Type"], "Series");
    assert_eq!(series["IsFolder"], true);
    let seasons = s
        .call(
            "GET",
            &format!("/Items?ParentId={}", series["Id"].as_str().unwrap()),
            None,
        )
        .await
        .1;
    assert_eq!(
        seasons["Items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["IndexNumber"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let episodes = s
        .call(
            "GET",
            &format!(
                "/Items?ParentId={}",
                seasons["Items"][1]["Id"].as_str().unwrap()
            ),
            None,
        )
        .await
        .1;
    assert_eq!(episodes["Items"][0]["IndexNumber"], 2);
    assert_eq!(episodes["Items"][1]["IndexNumber"], 10);
    let episode = episodes["Items"][0]["Id"].as_str().unwrap();
    assert_eq!(episodes["Items"][0]["SeriesId"], series["Id"]);
    s.call(
        "POST",
        "/Sessions/Playing/Progress",
        Some(json!({"ItemId":episode,"PositionTicks":123})),
    )
    .await;
    s.scan().await;
    assert_eq!(
        s.call("GET", &format!("/Items/{episode}"), None).await.1["UserData"]["PlaybackPositionTicks"],
        123
    );
    assert_eq!(
        s.call(
            "GET",
            &format!("/Items/{}", series["Id"].as_str().unwrap()),
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    std::fs::remove_dir_all(root.join("Example Show/Season 02")).unwrap();
    s.scan().await;
    assert_eq!(
        s.call(
            "GET",
            &format!("/Items?ParentId={}", series["Id"].as_str().unwrap()),
            None
        )
        .await
        .1["TotalRecordCount"],
        2
    );
}

#[test]
fn upgrades_old_databases_before_creating_new_indexes() {
    for version in [1, 2, 3] {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("old.db");
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch("CREATE TABLE libraries(id TEXT PRIMARY KEY,name TEXT,path TEXT,kind TEXT,root_identity TEXT);
          CREATE TABLE items(id TEXT PRIMARY KEY,library_id TEXT,path TEXT UNIQUE,name TEXT,kind TEXT,container TEXT,size INTEGER,modified INTEGER,runtime_ticks INTEGER,media_streams TEXT,scan_id TEXT);
          INSERT INTO items VALUES ('existing','library','/movie.mp4','Keep me','Movie','mp4',1,1,NULL,'[]','old');").unwrap();
        c.pragma_update(None, "user_version", version).unwrap();
        drop(c);
        Database::open(&path).unwrap();
        Database::open(&path).unwrap();
        let c = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        assert_eq!(
            c.query_row("SELECT name FROM items WHERE id='existing'", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "Keep me"
        );
        c.prepare("SELECT parent_id,index_number,parent_index_number,tmdb_id FROM items")
            .unwrap();
    }
}

#[tokio::test]
async fn tmdb_metadata_and_artwork_survive_rescans_restart_and_refresh() {
    use axum::{Json, extract::Request};
    let mock = axum::Router::new().fallback(|request: Request| async move {
        let path=request.uri().path();
        if path.starts_with("/images/") {return axum::response::IntoResponse::into_response(([("content-type","image/jpeg")],b"poster".to_vec()));}
        let data=match path {
            "/search/movie"=>json!({"results":[{"id":42,"title":"Example Movie","release_date":"2020-01-01","overview":"Movie description","genre_ids":[18],"vote_average":8.0,"poster_path":"/movie.jpg"}]}),
            "/genre/movie/list"=>json!({"genres":[{"id":18,"name":"Drama"}]}),
            "/search/tv"=>json!({"results":[{"id":100,"name":"Example Show","first_air_date":"2021-01-01"}]}),
            "/tv/100"=>json!({"id":100,"name":"Example Show","first_air_date":"2021-01-01","overview":"Series description","genres":[{"id":18,"name":"Drama"}],"poster_path":"/show.jpg"}),
            "/tv/100/season/1"=>json!({"id":101,"name":"Season One","air_date":"2021-01-01","overview":"Season description","poster_path":"/season.jpg"}),
            "/tv/100/season/1/episode/2"=>json!({"id":102,"name":"The Second Chapter","air_date":"2021-01-08","overview":"Episode description","vote_average":9.0,"still_path":"/still.jpg"}),
            _=>return axum::response::IntoResponse::into_response(StatusCode::NOT_FOUND),
        };
        axum::response::IntoResponse::into_response(Json(data))
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    let mut s = TestServer::new().await;
    // Scan once with metadata disabled: enrichment must later target the existing item ID.
    let movie = s.item().await;
    let root = s.dir.path().join("tv");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Example.Show.S01E02.mp4"), b"episode").unwrap();
    s.call(
        "POST",
        "/Library/VirtualFolders",
        Some(json!({"Name":"TV","CollectionType":"tvshows","Locations":[root]})),
    )
    .await;
    s.state.tmdb = TmdbConfig {
        api_key: Some("test-key".into()),
        api_base: format!("http://{address}"),
        image_base: format!("http://{address}/images"),
        ..Default::default()
    };
    s.app = router(s.state.clone());
    // A second local file which resolves to the same TMDb movie is an
    // alternate copy, not another library tile. Keep the larger playable
    // candidate and clean up the duplicate after metadata enrichment.
    std::fs::write(
        s.dir
            .path()
            .join("movies/Example.Movie.2020.2160p.x265.mkv"),
        b"x",
    )
    .unwrap();
    let status = s.scan().await;
    assert_eq!(status["MetadataFailures"], 0, "{status}");
    assert_eq!(status["Removed"], 1, "{status}");
    let movies = s.call("GET", "/Items?IncludeItemTypes=Movie", None).await.1;
    assert_eq!(movies["TotalRecordCount"], 1);
    assert_eq!(movies["Items"][0]["Id"], movie);
    let movie_data = s.call("GET", &format!("/Items/{movie}"), None).await.1;
    assert_eq!(movie_data["TmdbId"], "42");
    assert_eq!(movie_data["Genres"], json!(["Drama"]));
    let episode = s
        .call("GET", "/Items?IncludeItemTypes=Episode", None)
        .await
        .1["Items"][0]
        .clone();
    assert_eq!(episode["Name"], "The Second Chapter");
    assert_eq!(episode["TmdbId"], "102");
    assert_eq!(episode["SeriesTmdbId"], "100");
    let series = episode["SeriesId"].as_str().unwrap();
    let season = episode["ParentId"].as_str().unwrap();
    let eid = episode["Id"].as_str().unwrap();
    for (id, overview) in [
        (series, "Series description"),
        (season, "Season description"),
        (eid, "Episode description"),
    ] {
        let item = s.call("GET", &format!("/Items/{id}"), None).await.1;
        assert_eq!(item["Overview"], overview);
        assert_eq!(
            s.call("GET", &format!("/Items/{id}/Images/Primary"), None)
                .await
                .0,
            StatusCode::OK
        );
        assert!(
            s.state
                .data_dir
                .join("artwork")
                .join(format!("{id}.jpg"))
                .exists()
        );
    }
    s.scan().await;
    assert_eq!(
        s.call("GET", &format!("/Items/{eid}"), None).await.1["Name"],
        "The Second Chapter"
    );
    assert_eq!(
        s.call(
            "POST",
            &format!("/Items/{eid}/Metadata/Refresh"),
            Some(json!({}))
        )
        .await
        .1["Matched"],
        true
    );
    let restarted = AppState::new(
        Database::open(&s.dir.path().join("test.db")).unwrap(),
        "Restarted".into(),
        "ffprobe".into(),
        s.dir.path().into(),
        TmdbConfig::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        send(
            &router(restarted),
            "GET",
            &format!("/Items/{eid}"),
            Some(&s.token),
            None
        )
        .await
        .1["TmdbId"],
        "102"
    );
    server.abort();
}

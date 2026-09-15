pub mod assets;
pub mod auth;
pub mod catalog;
pub mod db;
pub mod error;
pub mod instance;
pub mod object_storage;
pub mod playback;
pub mod playlists;
pub mod remote;
pub mod scanner;
pub mod subtitle_provider;
pub mod tickets;
pub mod tmdb;
pub mod transcode;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    routing::{delete, get, post},
};
use db::Database;
use error::Result;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::{RwLock, Semaphore};

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub server_id: String,
    pub name: String,
    pub ffprobe: String,
    pub ffmpeg: String,
    pub data_dir: PathBuf,
    pub tmdb: tmdb::TmdbConfig,
    pub scan_gate: Arc<Semaphore>,
    pub transcode_gate: Arc<Semaphore>,
    pub subtitle_gate: Arc<Semaphore>,
    pub transcode_sessions: transcode::TranscodeSessions,
    pub http: reqwest::Client,
    pub scan_status: Arc<RwLock<scanner::ScanStatus>>,
    pub login_gate: Arc<Semaphore>,
    pub login_window: Arc<Mutex<(Instant, u32)>>,
    pub keyframe_safety: Arc<Mutex<HashMap<String, bool>>>,
    pub internal_origin: Arc<RwLock<String>>,
    pub internal_token: String,
}
impl AppState {
    pub async fn new(
        db: Database,
        name: String,
        ffprobe: String,
        data_dir: PathBuf,
        tmdb: tmdb::TmdbConfig,
    ) -> Result<Self> {
        let data_dir = data_dir
            .canonicalize()
            .map_err(|_| error::Error::internal("Cannot resolve data directory"))?;
        let server_id = db
            .call(|c| {
                Ok(c.query_row(
                    "SELECT value FROM settings WHERE key='server_id'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await?;
        let ffmpeg = if std::path::Path::new(&ffprobe)
            .file_name()
            .is_some_and(|name| name == "ffprobe")
        {
            std::path::Path::new(&ffprobe)
                .with_file_name("ffmpeg")
                .to_string_lossy()
                .into_owned()
        } else {
            "ffmpeg".to_owned()
        };
        let transcode_sessions = transcode::TranscodeSessions::new(&data_dir);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(8))
            .user_agent("Jellymax/0.1")
            .build()
            .map_err(error::Error::internal)?;
        Ok(Self {
            db,
            server_id,
            name,
            ffprobe,
            ffmpeg,
            data_dir,
            tmdb,
            scan_gate: Arc::new(Semaphore::new(1)),
            transcode_gate: Arc::new(Semaphore::new(2)),
            subtitle_gate: Arc::new(Semaphore::new(2)),
            transcode_sessions,
            http,
            scan_status: Arc::new(RwLock::new(scanner::ScanStatus::default())),
            login_gate: Arc::new(Semaphore::new(2)),
            login_window: Arc::new(Mutex::new((Instant::now(), 0))),
            keyframe_safety: Arc::new(Mutex::new(HashMap::new())),
            internal_origin: Arc::new(RwLock::new(String::new())),
            internal_token: uuid::Uuid::new_v4().simple().to_string(),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route("/System/Info/Public", get(public_info))
        .route("/System/Setup", post(auth::setup))
        .route("/Users/AuthenticateByName", post(auth::login))
        .route("/Users/Me", get(auth::me))
        .route("/Users", get(auth::users))
        .route("/Users/", get(auth::users))
        .route("/Users/New", post(auth::create_user))
        .route("/Sessions/Logout", post(auth::logout))
        .route(
            "/Library/VirtualFolders",
            get(catalog::libraries).post(catalog::create_library),
        )
        .route(
            "/Library/VirtualFolders/{id}",
            delete(catalog::delete_library),
        )
        .route("/Library/Paths", get(catalog::directories))
        .route("/Library/Refresh", post(scanner::refresh))
        .route("/RemoteServers", get(remote::list).post(remote::connect))
        .route(
            "/ObjectStores",
            get(object_storage::list).post(object_storage::connect),
        )
        .route("/ObjectStores/{id}", delete(object_storage::remove))
        .route("/ObjectStores/{id}/Sync", post(object_storage::sync))
        .route("/ObjectItems/{id}/stream", get(object_storage::stream))
        .route("/RemoteServers/{id}", delete(remote::remove))
        .route("/RemoteServers/{id}/Sync", post(remote::sync))
        .route("/RemoteItems/{id}/stream", get(remote::stream))
        .route("/ScheduledTasks", get(scanner::status))
        .route("/Items", get(catalog::items))
        .route("/Recommendations", get(catalog::recommendations))
        .route("/Items/{id}", get(catalog::item))
        .route(
            "/Items/{id}/AdjacentEpisodes",
            get(catalog::adjacent_episodes),
        )
        .route("/Items/{id}/Metadata/Refresh", post(tmdb::refresh))
        .route("/Users/{user}/Items", get(catalog::user_items))
        .route("/Users/{user}/Items/{id}", get(catalog::user_item))
        .route("/Users/{user}/Items/Resume", get(catalog::resume))
        .route(
            "/Users/{user}/FavoriteItems/{id}",
            post(playback::favorite).delete(playback::unfavorite),
        )
        .route(
            "/Users/{user}/PlayedItems/{id}",
            post(playback::played).delete(playback::unplayed),
        )
        .route(
            "/Items/{id}/PlaybackInfo",
            get(playback::info).post(playback::info),
        )
        .route("/Videos/{id}/stream", get(playback::stream))
        .route("/Audio/{id}/stream", get(playback::stream))
        .route("/Videos/{id}/transcode.mp4", get(playback::transcode))
        .route("/Audio/{id}/transcode.m4a", get(playback::transcode))
        .route("/Videos/{id}/hls/master.m3u8", get(playback::hls_manifest))
        .route("/Videos/{id}/hls/{segment}", get(playback::hls_segment))
        .route("/Items/{id}/Download", get(playback::stream))
        .route("/Items/{id}/Images/Primary", get(assets::image))
        .route("/Items/{id}/Subtitles", get(assets::subtitles))
        .route("/Items/{id}/Subtitles/{index}", get(assets::subtitle))
        .route("/Items/{id}/SubtitleSearch", get(subtitle_provider::search))
        .route(
            "/Items/{id}/SubtitleDownload",
            post(subtitle_provider::download),
        )
        .route("/Sessions/Playing/Progress", post(playback::progress))
        .route("/Sessions/Playing/Stopped", post(playback::stopped))
        .route("/Playlists", get(playlists::list).post(playlists::create))
        .route("/Playlists/{id}", delete(playlists::remove))
        .route(
            "/Playlists/{id}/Items",
            get(playlists::items).post(playlists::append),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(axum::middleware::from_fn(private_response))
        .with_state(state)
}
async fn public_info(State(state): State<AppState>) -> Result<Json<Value>> {
    let completed = auth::has_administrator(&state.db).await?;
    Ok(Json(
        json!({"ServerName":state.name,"Id":state.server_id,"Version":env!("CARGO_PKG_VERSION"),
        "ProductName":"Jellymax","StartupWizardCompleted":completed}),
    ))
}

async fn private_response(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let artwork = request.uri().path().ends_with("/Images/Primary");
    let mut response = next.run(request).await;
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static(if artwork {
            "private, max-age=86400"
        } else {
            "private, no-store"
        }),
    );
    response.headers_mut().insert(
        axum::http::header::REFERRER_POLICY,
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    response
}

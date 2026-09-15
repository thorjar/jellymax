use axum::http::{HeaderValue, Method, header};
use clap::{Parser, Subcommand};
use jellymax::{AppState, auth, db::Database, instance::ServerInstance, router};
use std::{net::SocketAddr, path::PathBuf};
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    services::{ServeDir, ServeFile},
};

#[derive(Parser)]
#[command(version, about)]
struct Options {
    #[arg(long, env = "JELLYMAX_DATA_DIR", default_value = "data")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Start the HTTP server. By default only local connections are accepted.
    Serve {
        #[arg(long, env = "JELLYMAX_BIND", default_value = "127.0.0.1:8097")]
        bind: SocketAddr,
        #[arg(long, env = "JELLYMAX_NAME", default_value = "Jellymax")]
        name: String,
        #[arg(long, env = "JELLYMAX_FFPROBE", default_value = "ffprobe")]
        ffprobe: String,
        #[arg(long, env = "JELLYMAX_FFMPEG")]
        ffmpeg: Option<String>,
        /// Explicit browser frontend origin, e.g. http://localhost:5173.
        #[arg(long, env = "JELLYMAX_CORS_ORIGIN")]
        cors_origin: Option<String>,
        /// Optional compiled web frontend directory served from this same port.
        #[arg(long, env = "JELLYMAX_WEB_DIR")]
        web_dir: Option<PathBuf>,
        /// TMDb API v3 key; enables movie metadata enrichment during scans.
        #[arg(long, env = "JELLYMAX_TMDB_API_KEY")]
        tmdb_api_key: Option<String>,
        #[arg(long, env = "JELLYMAX_TMDB_LANGUAGE", default_value = "en-US")]
        tmdb_language: String,
    },
    /// Create an administrator. Reads the password from JELLYMAX_ADMIN_PASSWORD.
    CreateAdmin {
        #[arg(long)]
        username: String,
    },
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "jellymax=info".into()),
        )
        .init();
    let options = Options::parse();
    std::fs::create_dir_all(&options.data_dir)?;
    let _instance = if matches!(&options.command, Command::Serve { .. }) {
        Some(ServerInstance::acquire(&options.data_dir)?)
    } else {
        None
    };
    let db = Database::open(&options.data_dir.join("jellyfin.db"))?;
    match options.command {
        Command::CreateAdmin { username } => {
            let password = std::env::var("JELLYMAX_ADMIN_PASSWORD").map_err(
                |_| "Set JELLYMAX_ADMIN_PASSWORD to a password of at least 12 characters",
            )?;
            let user = auth::add_user(&db, username, password, true).await?;
            println!("Created administrator {} ({})", user.name, user.id);
        }
        Command::Serve {
            bind,
            name,
            ffprobe,
            ffmpeg,
            cors_origin,
            web_dir,
            tmdb_api_key,
            tmdb_language,
        } => {
            let mut state = AppState::new(
                db,
                name,
                ffprobe,
                options.data_dir.clone(),
                jellymax::tmdb::TmdbConfig {
                    api_key: tmdb_api_key,
                    language: tmdb_language,
                    ..Default::default()
                },
            )
            .await?;
            if let Some(ffmpeg) = ffmpeg {
                state.ffmpeg = ffmpeg;
            }
            let mut app = router(state);
            if let Some(web_dir) = web_dir.filter(|path| path.is_dir()) {
                let index = web_dir.join("index.html");
                app = app.fallback_service(ServeDir::new(web_dir).fallback(ServeFile::new(index)));
            }
            if let Some(origin) = cors_origin {
                app = app.layer(
                    CorsLayer::new()
                        .allow_origin(AllowOrigin::exact(origin.parse::<HeaderValue>()?))
                        .allow_methods([
                            Method::GET,
                            Method::HEAD,
                            Method::POST,
                            Method::DELETE,
                            Method::OPTIONS,
                        ])
                        .allow_headers([
                            header::AUTHORIZATION,
                            header::CONTENT_TYPE,
                            header::RANGE,
                            header::HeaderName::from_static("x-emby-token"),
                            header::HeaderName::from_static("x-emby-authorization"),
                        ])
                        .expose_headers([
                            header::CONTENT_RANGE,
                            header::ACCEPT_RANGES,
                            header::CONTENT_LENGTH,
                        ]),
                );
            }
            let listener = tokio::net::TcpListener::bind(bind).await?;
            tracing::info!(address=%listener.local_addr()?,"Media server listening");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown())
                .await?;
        }
    }
    Ok(())
}
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {_=ctrl_c=>{},_=terminate=>{}}
}

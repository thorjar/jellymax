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
    #[arg(long, env = "JELLYMAX_DATA_DIR")]
    data_dir: Option<PathBuf>,
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
    /// Open the locally running Jellymax web interface in the default browser.
    Open {
        #[arg(long, default_value = "http://localhost:8097")]
        url: String,
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
    if let Command::Open { url } = &options.command {
        open_browser(url)?;
        return Ok(());
    }
    let data_dir = options.data_dir.unwrap_or_else(default_data_dir);
    std::fs::create_dir_all(&data_dir)?;
    let _instance = if matches!(&options.command, Command::Serve { .. }) {
        Some(ServerInstance::acquire(&data_dir)?)
    } else {
        None
    };
    let db = Database::open(&data_dir.join("jellyfin.db"))?;
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
                data_dir.clone(),
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
            let listener = tokio::net::TcpListener::bind(bind).await?;
            *state.internal_origin.write().await =
                format!("http://127.0.0.1:{}", listener.local_addr()?.port());
            let mut app = router(state);
            if let Some(web_dir) = resolve_web_dir(web_dir)? {
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
            tracing::info!(address=%listener.local_addr()?,"Media server listening");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown())
                .await?;
        }
        Command::Open { .. } => unreachable!(),
    }
    Ok(())
}

fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Jellymax");
    }
    #[cfg(target_os = "macos")]
    if let Some(path) = std::env::var_os("HOME") {
        return PathBuf::from(path)
            .join("Library")
            .join("Application Support")
            .join("Jellymax");
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(path).join("jellymax");
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".local").join("share").join("jellymax"))
        .unwrap_or_else(|| PathBuf::from("data"))
}

fn resolve_web_dir(
    explicit: Option<PathBuf>,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    if let Some(path) = explicit {
        if path.join("index.html").is_file() {
            return Ok(Some(path));
        }
        return Err(format!("Web frontend was not found at {}", path.display()).into());
    }
    let executable = std::env::current_exe()?;
    let executable_dir = executable
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let candidates = [
        executable_dir.join("web"),
        executable_dir.join("..").join("Resources").join("web"),
        PathBuf::from("frontend").join("dist"),
    ];
    Ok(candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file()))
}

fn open_browser(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = if cfg!(target_os = "windows") {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .status()?
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()?
    } else {
        std::process::Command::new("xdg-open").arg(url).status()?
    };
    if !status.success() {
        return Err("Could not open the default browser".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::resolve_web_dir;

    #[test]
    fn explicit_web_directory_must_contain_an_index() {
        let directory = tempfile::tempdir().unwrap();
        assert!(resolve_web_dir(Some(directory.path().to_path_buf())).is_err());
        std::fs::write(directory.path().join("index.html"), "<!doctype html>").unwrap();
        assert_eq!(
            resolve_web_dir(Some(directory.path().to_path_buf()))
                .unwrap()
                .unwrap(),
            directory.path()
        );
    }
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

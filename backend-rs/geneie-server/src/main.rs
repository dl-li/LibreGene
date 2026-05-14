use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::http::HeaderValue;
use axum::Router;
use clap::Parser;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::compression::CompressionLayer;
use tracing_subscriber::{fmt, EnvFilter};

use geneie_core::project::ProjectManager;

mod routes;
mod ws;

#[derive(Parser, Debug)]
#[command(name = "geneie-server")]
struct Args {
    #[arg(long, default_value = "8765")]
    port: u16,
    /// Base directory for file open/save operations.
    /// Defaults to the current directory (or its parent if inside a backend-rs dir).
    #[arg(long)]
    data_dir: Option<String>,
}

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();

    let base_dir = if let Some(ref dir) = args.data_dir {
        PathBuf::from(dir)
    } else {
        let cwd = std::env::current_dir().expect("cannot get current dir");
        // If the server is started from backend-rs, use the parent (project root)
        if cwd.file_name().map_or(false, |n| n == "backend-rs") {
            cwd.parent().map(PathBuf::from).unwrap_or(cwd)
        } else {
            cwd
        }
    };

    let pm = Arc::new(RwLock::new(ProjectManager::new()));
    let (ws_tx, _) = broadcast::channel::<String>(256);

    let state = Arc::new(routes::AppState { pm, ws_tx, base_dir });

    let cors = CorsLayer::new()
        .allow_origin(HeaderValue::from_static("http://localhost:5173"))
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .merge(routes::build_router())
        .layer(CompressionLayer::new())
        .layer(cors)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    let listener = TcpListener::bind(addr).await.expect("failed to bind");

    tracing::info!("Geneie server running on http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
    tracing::info!("Shutting down...");
}

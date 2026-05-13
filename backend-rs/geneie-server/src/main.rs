use std::net::SocketAddr;
use std::sync::Arc;

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
}

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let args = Args::parse();

    let pm = Arc::new(RwLock::new(ProjectManager::new()));
    let (ws_tx, _) = broadcast::channel::<String>(256);

    let state = Arc::new(routes::AppState { pm, ws_tx });

    let cors = CorsLayer::new()
        .allow_origin(Any)
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

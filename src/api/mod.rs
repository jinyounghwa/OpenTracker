pub mod routes;

use crate::config::Config;
use anyhow::{Context, Result};
use axum::Router;
use rust_embed::RustEmbed;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct FrontendAssets;

pub async fn run_server(config: Arc<Config>) -> Result<()> {
    let port = config.api_port;
    let state = routes::ApiState { config };
    let app: Router = routes::router(state);

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("Failed to bind API server: {addr}"))?;

    info!(address = %addr, "OpenTracker API server started");

    axum::serve(listener, app)
        .await
        .context("API server failed")?;

    Ok(())
}

pub fn get_embedded_asset(path: &str) -> Option<(Vec<u8>, String)> {
    let normalized = path.trim_start_matches('/');
    let requested = if normalized.is_empty() {
        "index.html"
    } else {
        normalized
    };

    FrontendAssets::get(requested)
        .or_else(|| FrontendAssets::get("index.html"))
        .map(|content| {
            let mime = mime_guess::from_path(requested)
                .first_or_octet_stream()
                .to_string();
            (content.data.into_owned(), mime)
        })
}

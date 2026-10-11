//! The HTTP API used by the SBRW launcher and the NFS World game client.
//!
//! The game addresses everything relative to the server URL the launcher hands
//! it, which ends in `/Engine.svc`. Everything under it goes through
//! [`engine::dispatch`], which routes like upstream's RESTEasy did; mod files
//! and the health check live outside it.

mod capture;
mod engine;
mod launcher;
mod mods;

use std::io;
use std::sync::Arc;

use axum::Router;
use axum::http::HeaderValue;
use axum::http::header::CONNECTION;
use axum::middleware;
use axum::routing::{any, get};
use sbrw_common::{CaptureSink, Config};
use sbrw_core::WorldHandle;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::{DefaultOnFailure, TraceLayer};

#[derive(Clone)]
pub struct AppState {
    capture: CaptureSink,
    mods: Arc<mods::ModsDir>,
    world: WorldHandle,
}

impl AppState {
    /// The launcher and game read the server's settings from the world's
    /// parameters (see `Config::upstream_parameters`); the config here only
    /// says where the mods are.
    pub fn new(config: Arc<Config>, capture: CaptureSink, world: WorldHandle) -> Self {
        let mods = Arc::new(mods::ModsDir::new(config.mods_dir()));
        Self { capture, mods, world }
    }

    pub fn world(&self) -> &WorldHandle {
        &self.world
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/Engine.svc", any(engine::dispatch))
        .route("/Engine.svc/", any(engine::dispatch))
        .route("/Engine.svc/{*path}", any(engine::dispatch))
        .route("/mods/{name}", get(mods::file))
        .route("/health", get(health))
        // Innermost, so captured bodies are the uncompressed ones.
        .layer(middleware::from_fn_with_state(state.clone(), capture::record))
        .layer(CompressionLayer::new())
        .layer(RequestDecompressionLayer::new())
        // Upstream answers failed logins with 500 and game errors with 503; those
        // are protocol, not server failures, so don't log them as errors.
        .layer(TraceLayer::new_for_http().on_failure(DefaultOnFailure::new().level(tracing::Level::DEBUG)))
        // port: api/util/HttpHeaderFilter#filter(ContainerRequestContext,ContainerResponseContext)
        // Upstream closes every connection; the launcher's proxy expects it.
        .layer(SetResponseHeaderLayer::overriding(CONNECTION, HeaderValue::from_static("close")))
        .with_state(state)
}

pub async fn serve(listener: TcpListener, state: AppState, shutdown: CancellationToken) -> io::Result<()> {
    // Handlers see the client address, as upstream's did through the servlet request.
    axum::serve(listener, router(state).into_make_service_with_connect_info::<std::net::SocketAddr>())
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

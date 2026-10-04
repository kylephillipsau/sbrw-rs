//! The HTTP API used by the SBRW launcher and the NFS World game client.
//!
//! The game addresses everything relative to the server URL the launcher hands
//! it, which ends in `/Engine.svc`. Two quirks shape the router:
//! - any `/Engine.svc` path we don't implement must answer 200 with an empty XML
//!   body, because the game treats a 404 as fatal (upstream's `Generic.java`);
//! - `/Engine.svc/Modding/GetModInfo` must be a real route, because the launcher
//!   reads a 404 there as "this server has no ModNet" and a 200 as JSON.

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
use axum::routing::get;
use sbrw_common::{CaptureSink, Config, EffectLog, Env};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower_http::compression::CompressionLayer;
use tower_http::decompression::RequestDecompressionLayer;
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    config: Arc<Config>,
    capture: CaptureSink,
    mods: Arc<mods::ModsDir>,
    env: Env,
    effects: EffectLog,
}

impl AppState {
    /// Production state: system clock and randomness, no effect recording.
    pub fn new(config: Arc<Config>, capture: CaptureSink) -> Self {
        let mods = Arc::new(mods::ModsDir::new(config.mods_dir()));
        Self { config, capture, mods, env: Env::system(), effects: EffectLog::disabled() }
    }

    /// Replace the clock, randomness and ids (replays use a deterministic [`Env`]).
    pub fn with_env(mut self, env: Env) -> Self {
        self.env = env;
        self
    }

    /// Record side effects into `effects` (replays compare them with recordings).
    pub fn with_effects(mut self, effects: EffectLog) -> Self {
        self.effects = effects;
        self
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    pub fn effects(&self) -> &EffectLog {
        &self.effects
    }
}

pub fn router(state: AppState) -> Router {
    let engine = Router::new()
        .route("/GetServerInformation", get(launcher::server_information))
        .route("/Modding/GetModInfo", get(launcher::mod_info))
        .fallback(engine::not_implemented);

    Router::new()
        .nest("/Engine.svc", engine)
        .route("/mods/{name}", get(mods::file))
        .route("/health", get(health))
        // Innermost, so captured bodies are the uncompressed ones.
        .layer(middleware::from_fn_with_state(state.clone(), capture::record))
        .layer(CompressionLayer::new())
        .layer(RequestDecompressionLayer::new())
        .layer(TraceLayer::new_for_http())
        // Upstream closes every connection; the launcher's proxy expects it.
        .layer(SetResponseHeaderLayer::overriding(CONNECTION, HeaderValue::from_static("close")))
        .with_state(state)
}

pub async fn serve(listener: TcpListener, state: AppState, shutdown: CancellationToken) -> io::Result<()> {
    axum::serve(listener, router(state)).with_graceful_shutdown(shutdown.cancelled_owned()).await
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

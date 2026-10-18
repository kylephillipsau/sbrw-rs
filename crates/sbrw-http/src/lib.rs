//! The HTTP API used by the SBRW launcher and the NFS World game client.
//!
//! The game addresses everything relative to the server URL the launcher hands
//! it, which ends in `/Engine.svc`. Everything under it goes through
//! [`engine::dispatch`], which routes like upstream's RESTEasy did; mod files
//! and the health check live outside it.

mod admin;
mod capture;
mod engine;
mod launcher;
mod limits;
mod mail;
mod mods;
mod pages;

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
    mailer: Option<Arc<mail::Mailer>>,
    /// The operator API's token; without one the API is off.
    admin_token: Option<Arc<str>>,
    /// The database file `sbrw admin backup` copies.
    world_db: Option<Arc<std::path::Path>>,
    auth_limiter: Arc<limits::AuthLimiter>,
    world: WorldHandle,
}

impl AppState {
    /// The launcher and game read the server's settings from the world's
    /// parameters (see `Config::upstream_parameters`); the config here only
    /// says where the mods are and how to send mail.
    pub fn new(config: Arc<Config>, capture: CaptureSink, world: WorldHandle) -> Self {
        let mods = Arc::new(mods::ModsDir::new(config.mods_dir()));
        let mailer = mail::Mailer::from_url(config.mail.smtp_url.as_deref()).map(Arc::new);
        let auth_limiter = Arc::new(limits::AuthLimiter::new(
            config.http.auth_failures,
            std::time::Duration::from_secs(config.http.auth_window_secs),
        ));
        Self { capture, mods, mailer, admin_token: None, world_db: None, auth_limiter, world }
    }

    /// Let `sbrw admin backup` copy the database file at `world_db`.
    pub fn with_backups(mut self, world_db: std::path::PathBuf) -> Self {
        self.world_db = Some(world_db.into());
        self
    }

    /// Turn on the operator API (`sbrw admin`) for loopback clients with this token.
    pub fn with_admin_token(mut self, token: &str) -> Self {
        self.admin_token = Some(token.into());
        self
    }

    pub fn world(&self) -> &WorldHandle {
        &self.world
    }

    fn mailer(&self) -> Option<&mail::Mailer> {
        self.mailer.as_deref()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/Engine.svc", any(engine::dispatch))
        .route("/Engine.svc/", any(engine::dispatch))
        .route("/Engine.svc/{*path}", any(engine::dispatch))
        .route("/mods/{name}", get(mods::file))
        .route("/health", get(health))
        .route("/admin/{command}", any(admin::handle))
        .route("/", get(pages::index))
        .route("/{page}", get(pages::page))
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

/// 200 once the world answers (within 5 seconds), with who is on; 503 when
/// it doesn't, which fails `sbrw healthcheck` and so Docker's health check.
async fn health(axum::extract::State(state): axum::extract::State<AppState>) -> axum::response::Response {
    use axum::response::IntoResponse;
    let counts = state.world.call(|w| {
        let online = w.openfire.online_users.load(std::sync::atomic::Ordering::Relaxed);
        Ok((online, sbrw_core::dao::user::count_users(w.db())?))
    });
    match tokio::time::timeout(std::time::Duration::from_secs(5), counts).await {
        Ok(Ok((online, registered))) => axum::Json(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "online": online,
            "registered": registered,
        }))
        .into_response(),
        Ok(Err(fault)) => {
            tracing::warn!("health check: {fault:#}");
            (axum::http::StatusCode::SERVICE_UNAVAILABLE, axum::Json(serde_json::json!({ "status": "error" })))
                .into_response()
        }
        Err(_) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({ "status": "world not answering" })),
        )
            .into_response(),
    }
}

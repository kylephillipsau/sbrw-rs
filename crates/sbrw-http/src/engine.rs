//! `/Engine.svc` endpoints that don't belong to a specific feature.

use axum::extract::OriginalUri;
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method, StatusCode};
use axum::response::{IntoResponse, Response};

/// The content type of every XML response upstream sends (RESTEasy adds the charset).
pub const XML: &str = "application/xml;charset=UTF-8";

/// Answers every `/Engine.svc` path without a handler with 200 and an empty XML
/// body, as upstream's `Generic.java` does; the game treats a 404 as fatal.
/// Logged at info so captures and logs show which endpoints are still missing.
pub async fn not_implemented(method: Method, OriginalUri(uri): OriginalUri) -> Response {
    if !matches!(method, Method::GET | Method::POST | Method::PUT) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    tracing::info!(target: "sbrw::unimplemented", %method, path = uri.path(), "no handler; answered 200 with an empty body");
    ([(CONTENT_TYPE, XML)], "").into_response()
}

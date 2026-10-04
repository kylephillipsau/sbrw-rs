//! Records HTTP exchanges into the capture sink when `http` capture is on.

use axum::body::{Body, Bytes};
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use http_body_util::BodyExt;
use sbrw_common::CaptureStream;
use sbrw_common::capture::HttpExchange;

use crate::AppState;

pub async fn record(State(state): State<AppState>, request: Request, next: Next) -> Response {
    // Mod packages can be large and are not protocol traffic.
    let path = request.uri().path();
    let is_mod_package = path.starts_with("/mods/") && path != "/mods/index.json";
    if !state.capture.enabled(CaptureStream::Http) || is_mod_package {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();
    let request_body = collect(body).await;
    let method = parts.method.to_string();
    let uri = parts.uri.to_string();
    let request_headers = header_pairs(&parts.headers);

    let response = next.run(Request::from_parts(parts, Body::from(request_body.clone()))).await;
    let (parts, body) = response.into_parts();
    let response_body = collect(body).await;

    state.capture.http(HttpExchange {
        method,
        uri,
        request_headers,
        request_body: String::from_utf8_lossy(&request_body).into_owned(),
        status: parts.status.as_u16(),
        response_headers: header_pairs(&parts.headers),
        response_body: String::from_utf8_lossy(&response_body).into_owned(),
    });
    Response::from_parts(parts, Body::from(response_body))
}

async fn collect(body: Body) -> Bytes {
    body.collect().await.map(|collected| collected.to_bytes()).unwrap_or_default()
}

fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| (name.to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
        .collect()
}

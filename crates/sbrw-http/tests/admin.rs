//! The operator API behind `sbrw admin`: only for this machine, with the token.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use sbrw_common::{CaptureSink, Config, EffectLog, Env};
use sbrw_core::{BuildInfo, World, WorldHandle, WorldOptions};
use serde_json::{Value, json};
use tower::ServiceExt;

fn server(token: Option<&str>) -> Router {
    let config = Config::default();
    let world = World::boot(WorldOptions {
        db_path: None,
        parameters: config.upstream_parameters(),
        env: Env::system(),
        effects: EffectLog::disabled(),
        openfire: Default::default(),
        build_info: BuildInfo::of_this_build(),
        relay: sbrw_common::RelayRegistry::default(),
    })
    .unwrap();
    let state = sbrw_http::AppState::new(Arc::new(config), CaptureSink::disabled(), WorldHandle::spawn(world));
    sbrw_http::router(match token {
        Some(token) => state.with_admin_token(token),
        None => state,
    })
}

async fn admin(router: &Router, from: &str, token: Option<&str>, command: &str, body: Value) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(Method::POST)
        .uri(format!("/admin/{command}"))
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let mut request = request.body(Body::from(body.to_string())).unwrap();
    request.extensions_mut().insert(ConnectInfo(from.parse::<SocketAddr>().unwrap()));
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

const HERE: &str = "127.0.0.1:50000";

#[tokio::test]
async fn only_this_machine_with_the_token_gets_in() {
    let router = server(Some("secret"));
    assert_eq!(admin(&router, HERE, None, "tokens", json!({})).await.0, StatusCode::UNAUTHORIZED);
    for wrong in ["wrong", "secre", "secretX", ""] {
        assert_eq!(admin(&router, HERE, Some(wrong), "tokens", json!({})).await.0, StatusCode::UNAUTHORIZED, "{wrong}");
    }
    assert_eq!(admin(&router, "172.17.0.1:50000", Some("secret"), "tokens", json!({})).await.0, StatusCode::NOT_FOUND);
    assert_eq!(admin(&router, "[::1]:50000", Some("secret"), "tokens", json!({})).await.0, StatusCode::OK);
    let (status, tokens) = admin(&router, HERE, Some("secret"), "tokens", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(tokens["ADMIN_AUTH"].as_str().map(str::len), Some(32), "{tokens}");
    assert_eq!(tokens["TICKET_TOKEN"], Value::Null);
    // Without a token the API is off altogether.
    assert_eq!(admin(&server(None), HERE, Some("secret"), "tokens", json!({})).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn commands_answer_with_a_message_or_an_error() {
    let router = server(Some("t"));
    let register = Request::builder()
        .uri("/Engine.svc/User/createUser?email=p%40example.com&password=5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8")
        .header("X-HWID", "H")
        .body(Body::empty())
        .unwrap();
    assert_eq!(router.clone().oneshot(register).await.unwrap().status(), StatusCode::OK);

    let ban = json!({ "target": "p@example.com", "length": "1d", "reason": "cheating" });
    let (status, reply) = admin(&router, HERE, Some("t"), "ban", ban.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert!(reply["message"].as_str().unwrap().starts_with("Banned user! The ban ends at "), "{reply}");
    assert_eq!(
        admin(&router, HERE, Some("t"), "ban", ban).await,
        (StatusCode::BAD_REQUEST, json!({ "error": "User is already banned!" }))
    );
    assert_eq!(
        admin(&router, HERE, Some("t"), "unban", json!({ "target": "p@example.com" })).await,
        (StatusCode::OK, json!({ "message": "Unbanned user!" }))
    );
    assert_eq!(
        admin(&router, HERE, Some("t"), "ban", json!({})).await,
        (StatusCode::BAD_REQUEST, json!({ "error": "target is required" }))
    );
    assert_eq!(
        admin(&router, HERE, Some("t"), "ban", json!({ "who": "x" })).await.0,
        StatusCode::BAD_REQUEST,
        "unknown fields are refused"
    );
    assert_eq!(
        admin(&router, HERE, Some("t"), "reboot", json!({})).await,
        (StatusCode::BAD_REQUEST, json!({ "error": "unknown command reboot" }))
    );
}

//! The contracts the SBRW launcher and the game's catch-all behaviour rely on.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use sbrw_common::{CaptureSink, Config, EffectLog, Env};
use sbrw_core::{BuildInfo, World, WorldHandle, WorldOptions};
use serde_json::Value;
use tower::ServiceExt;

struct TestServer {
    router: Router,
    data_dir: tempfile::TempDir,
}

fn server_with(edit: impl FnOnce(&mut Config)) -> TestServer {
    let data_dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.server.data_dir = data_dir.path().to_path_buf();
    edit(&mut config);
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
    TestServer { router: sbrw_http::router(state), data_dir }
}

fn server() -> TestServer {
    server_with(|_| {})
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("JSON body")
    }
}

async fn send(server: &TestServer, method: Method, uri: &str) -> Reply {
    let request = Request::builder().method(method).uri(uri).body(Body::empty()).unwrap();
    let response = server.router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Reply { status, headers, body }
}

async fn get(server: &TestServer, uri: &str) -> Reply {
    send(server, Method::GET, uri).await
}

#[tokio::test]
async fn server_information_matches_the_launcher_model() {
    let server = server_with(|config| {
        config.launcher.server_name = "Test Server".into();
        config.launcher.discord_url = Some("https://discord.example".into());
    });
    let reply = get(&server, "/Engine.svc/GetServerInformation").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
    let info = reply.json();

    assert_eq!(info["serverName"], "Test Server");
    assert_eq!(info["discordUrl"], "https://discord.example");
    // The launcher deserialises these into non-nullable ints.
    for key in ["timezone", "numberOfRegistered", "secondsToShutDown", "onlineNumber"] {
        assert!(info[key].is_i64(), "{key} must be an integer, got {}", info[key]);
    }
    assert_eq!(info["secondsToShutDown"], 7200);
    assert_eq!(info["requireTicket"], false);
    // Upstream never sends it; the launcher's bool then defaults to false, which
    // keeps it on the SHA-1 login upstream implements.
    assert!(info.get("modernAuthSupport").is_none_or(|v| v == false));
    assert!(info["activatedHolidaySceneryGroups"].is_array());
    assert!(info["disactivatedHolidaySceneryGroups"].is_array());
    assert!(info["serverVersion"].as_str().unwrap().starts_with("sbrw-rs "));
    // Unset optional strings are omitted rather than null.
    assert!(info.get("bannerUrl").is_none());
    assert!(info.as_object().unwrap().values().all(|v| !v.is_null()));
}

#[tokio::test]
async fn unknown_engine_paths_answer_200_with_empty_xml() {
    let server = server();
    for method in [Method::GET, Method::POST, Method::PUT] {
        let reply = send(&server, method.clone(), "/Engine.svc/NotAnEndpoint/x").await;
        assert_eq!(reply.status, StatusCode::OK, "{method}");
        assert_eq!(reply.headers[header::CONTENT_TYPE], "application/xml;charset=UTF-8");
        assert!(reply.body.is_empty());
    }
    // Upstream maps the 405 to its generic error: 503 with code -500.
    let reply = send(&server, Method::DELETE, "/Engine.svc/anything").await;
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(String::from_utf8(reply.body).unwrap().starts_with("<EngineExceptionTrans><ErrorCode>-500</ErrorCode>"));
}

#[tokio::test]
async fn secured_endpoints_need_a_session_even_before_they_are_ported() {
    let reply = get(&server(), "/Engine.svc/events/instancedaccolades").await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
    assert!(reply.headers.get(header::CONTENT_TYPE).is_none());
}

#[tokio::test]
async fn paths_outside_the_engine_are_404() {
    let reply = get(&server(), "/not-the-engine").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn every_response_closes_the_connection() {
    let server = server();
    for uri in ["/Engine.svc/GetServerInformation", "/Engine.svc/unknown", "/nope", "/health"] {
        assert_eq!(get(&server, uri).await.headers[header::CONNECTION], "close", "{uri}");
    }
}

#[tokio::test]
async fn mod_info_points_the_launcher_at_our_mods() {
    let server = server_with(|config| {
        config.server.public_host = "10.1.2.3".into();
        config.modding.server_id = "my-server".into();
    });
    let reply = get(&server, "/Engine.svc/Modding/GetModInfo").await;
    assert_eq!(reply.status, StatusCode::OK);
    let info = reply.json();
    assert_eq!(info["basePath"], "http://10.1.2.3:8680/mods");
    assert_eq!(info["serverID"], "my-server");
    assert_eq!(info["features"], Value::Array(vec![]));
}

#[tokio::test]
async fn mod_info_is_404_when_modding_is_off() {
    // A 200 here, even empty, would make the launcher try to parse JSON and fail.
    let server = server_with(|config| config.modding.enabled = false);
    let reply = get(&server, "/Engine.svc/Modding/GetModInfo").await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mod_index_without_a_mods_directory_is_empty() {
    let reply = get(&server(), "/mods/index.json").await;
    assert_eq!(reply.status, StatusCode::OK);
    let index = reply.json();
    assert_eq!(index["entries"], Value::Array(vec![]));
    assert_eq!(index["built_at"], "1970-01-01T00:00:00Z");
}

#[tokio::test]
async fn mod_index_lists_packages_with_sha1() {
    let server = server();
    let mods = server.data_dir.path().join("mods");
    std::fs::create_dir_all(&mods).unwrap();
    std::fs::write(mods.join("b.mods"), b"hello").unwrap();
    std::fs::write(mods.join("a.mods"), b"").unwrap();
    std::fs::write(mods.join("cars.json"), b"[]").unwrap();
    std::fs::write(mods.join(".DS_Store"), b"x").unwrap();

    let index = get(&server, "/mods/index.json").await.json();
    let entries = index["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "reserved and hidden files are not packages: {entries:?}");
    assert_eq!(entries[0]["Name"], "a.mods");
    assert_eq!(entries[0]["Checksum"], "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(entries[1]["Name"], "b.mods");
    assert_eq!(entries[1]["Checksum"], "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d");
    assert_ne!(index["built_at"], "1970-01-01T00:00:00Z");

    let package = get(&server, "/mods/b.mods").await;
    assert_eq!(package.status, StatusCode::OK);
    assert_eq!(package.body, b"hello");
    // Present, so served, even though it isn't listed.
    assert_eq!(get(&server, "/mods/cars.json").await.status, StatusCode::OK);
}

#[tokio::test]
async fn discord_lists_404_unless_provided() {
    // The launcher falls back to its built-in car and event names on 404.
    let server = server();
    assert_eq!(get(&server, "/mods/cars.json").await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&server, "/mods/events.json").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mod_files_cannot_escape_the_mods_directory() {
    let server = server();
    std::fs::write(server.data_dir.path().join("sbrw.toml"), b"secret").unwrap();
    for uri in ["/mods/..%2Fsbrw.toml", "/mods/.hidden", "/mods/%2E%2E%2Fsbrw.toml"] {
        assert_eq!(get(&server, uri).await.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn responses_are_gzipped_when_accepted() {
    let server = server();
    let request = Request::builder()
        .uri("/Engine.svc/GetServerInformation")
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap();
    let response = server.router.clone().oneshot(request).await.unwrap();
    assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
}

#[tokio::test]
async fn the_freeroam_json_file_is_served_as_is() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"{\"spawns\": [1, 2]}\n").unwrap();
    let path = file.path().to_str().unwrap().to_owned();
    let server = server_with(|config| {
        config.parameters.insert("FREEROAM_JSON_PATH".into(), path);
    });
    let reply = get(&server, "/Engine.svc/GetFreeroamJson").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "application/json");
    assert_eq!(reply.body, b"{\"spawns\": [1, 2]}\n");
}

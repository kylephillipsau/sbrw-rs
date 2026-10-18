//! Password recovery's mail, which the reference rig can't send: a fake SMTP
//! server takes the link, and the key in it resets the password.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use sbrw_common::{CaptureSink, Config, EffectLog, Env};
use sbrw_core::{BuildInfo, World, WorldHandle, WorldOptions};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tower::ServiceExt;

/// Accepts one mail and hands back its envelope and data.
async fn fake_smtp_server() -> (u16, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        let mut seen = Vec::new();
        write.write_all(b"220 fake ESMTP\r\n").await.unwrap();
        let mut in_data = false;
        while let Some(line) = lines.next_line().await.unwrap() {
            if in_data {
                if line == "." {
                    in_data = false;
                    write.write_all(b"250 queued\r\n").await.unwrap();
                } else {
                    seen.push(line);
                }
                continue;
            }
            seen.push(line.clone());
            let reply: &[u8] = match line.split(' ').next().unwrap_or("") {
                "EHLO" | "HELO" => b"250 fake\r\n",
                "DATA" => {
                    in_data = true;
                    b"354 go ahead\r\n"
                }
                "QUIT" => {
                    write.write_all(b"221 bye\r\n").await.unwrap();
                    break;
                }
                _ => b"250 ok\r\n",
            };
            write.write_all(reply).await.unwrap();
        }
        seen
    });
    (port, server)
}

fn server(smtp_port: u16) -> Router {
    let mut config = Config::default();
    config.mail.smtp_url = Some(format!("smtp://127.0.0.1:{smtp_port}"));
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
    sbrw_http::router(state)
}

async fn send(router: &Router, request: Request<Body>) -> (StatusCode, String) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn post_form(router: &Router, uri: &str, form: &str) -> String {
    let request = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form.to_owned()))
        .unwrap();
    let (status, body) = send(router, request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body
}

async fn get(router: &Router, uri: &str) -> StatusCode {
    send(router, Request::builder().uri(uri).header("X-HWID", "HWID-1").body(Body::empty()).unwrap()).await.0
}

const PASSWORD: &str = "5baa61e4c9b93f3f0682250b6cf8331b7ee68fd8"; // sha1("password")
const HUNTER2: &str = "f3bbbd66a63d4bf1747940578ec3d0103530e21d"; // sha1("hunter2")

#[tokio::test]
async fn a_mailed_link_resets_the_password() {
    let (smtp_port, smtp) = fake_smtp_server().await;
    let router = server(smtp_port);
    let register = format!("/Engine.svc/User/createUser?email=player%40example.com&password={PASSWORD}");
    assert_eq!(get(&router, &register).await, StatusCode::OK);

    let reply = post_form(&router, "/Engine.svc/RecoveryPassword/forgotPassword", "email=player%40example.com").await;
    assert_eq!(reply, "Link to reset password sent to: [player@example.com]");
    let mail = smtp.await.unwrap();
    assert!(mail.iter().any(|l| l.starts_with("MAIL FROM:<sbrwserver@sbrw.com>")), "{mail:?}");
    assert!(mail.iter().any(|l| l == "RCPT TO:<player@example.com>"), "{mail:?}");
    assert!(mail.iter().any(|l| l == "Subject: Recovery Password Email"), "{mail:?}");
    let link = mail.iter().find(|l| l.contains("?randomKey=")).expect("a link");
    let (page, key) = link.split_once("?randomKey=").unwrap();
    assert_eq!(page, "http://127.0.0.1:8680/password.jsp");

    let reply = post_form(&router, "/Engine.svc/RecoveryPassword/forgotPassword", "email=player%40example.com").await;
    assert!(reply.starts_with("ERROR: Recovery password link already sent"), "{reply}");

    let reset = format!("password=hunter2&passwordconf=hunter2&randomKey={key}");
    assert_eq!(post_form(&router, "/Engine.svc/RecoveryPassword/resetPassword", &reset).await, "Password changed!");
    let login =
        |password: &str| format!("/Engine.svc/User/authenticateUser?email=player%40example.com&password={password}");
    assert_eq!(get(&router, &login(PASSWORD)).await, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(get(&router, &login(HUNTER2)).await, StatusCode::OK);
}

#[tokio::test]
async fn an_unreachable_mail_server_cannot_send() {
    // Nothing listens on the port once the listener is dropped.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let router = server(port);
    let register = format!("/Engine.svc/User/createUser?email=player%40example.com&password={PASSWORD}");
    assert_eq!(get(&router, &register).await, StatusCode::OK);
    let reply = post_form(&router, "/Engine.svc/RecoveryPassword/forgotPassword", "email=player%40example.com").await;
    assert_eq!(reply, "ERROR: Can't send email!");
    // No key was stored, so asking again tries again.
    let reply = post_form(&router, "/Engine.svc/RecoveryPassword/forgotPassword", "email=player%40example.com").await;
    assert_eq!(reply, "ERROR: Can't send email!");
}

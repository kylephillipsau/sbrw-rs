//! The operator's API behind `sbrw admin`: JSON over the HTTP listener, only
//! for connections from this machine that present the token `sbrw serve`
//! wrote into the data directory. Players connect from elsewhere (through
//! Docker, from the bridge's address), so they never reach it.

use std::net::SocketAddr;

use axum::Json;
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::response::{IntoResponse, Response};
use sbrw_core::ops;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;

/// Every command's arguments; each reads the ones it needs.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdminRequest {
    /// A driver's name, or an account's email.
    pub target: Option<String>,
    /// How long a ban lasts, as `/ban` reads it (`1d2h30m`); none is for good.
    pub length: Option<String>,
    pub reason: Option<String>,
    pub message: Option<String>,
    pub discord_name: Option<String>,
    pub email: Option<String>,
    pub admin: Option<bool>,
    pub password: Option<String>,
}

pub async fn handle(State(state): State<AppState>, Path(command): Path<String>, request: Request) -> Response {
    let Some(token) = state.admin_token.clone() else { return StatusCode::NOT_FOUND.into_response() };
    let local =
        request.extensions().get::<ConnectInfo<SocketAddr>>().is_some_and(|ConnectInfo(a)| a.ip().is_loopback());
    if !local {
        return StatusCode::NOT_FOUND.into_response();
    }
    let presented =
        request.headers().get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer "));
    if !presented.is_some_and(|p| constant_time_eq(p.as_bytes(), token.as_bytes())) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "wrong admin token; is this the server's data directory?" })),
        )
            .into_response();
    }
    let body = match axum::body::to_bytes(request.into_body(), 64 * 1024).await {
        Ok(body) => body,
        Err(err) => return error(err.to_string()),
    };
    let args: AdminRequest = if body.is_empty() {
        AdminRequest::default()
    } else {
        match serde_json::from_slice(&body) {
            Ok(args) => args,
            Err(err) => return error(err.to_string()),
        }
    };
    tracing::info!(command, target = ?args.target.as_ref().or(args.email.as_ref()), "admin command");
    match run(&state, &command, args).await {
        Ok(reply) => Json(reply).into_response(),
        Err(err) => error(err),
    }
}

fn error(message: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
}

fn required(value: Option<String>, name: &str) -> Result<String, String> {
    value.filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} is required"))
}

/// A command's work in the world: its reply, or why it couldn't be done.
type Operation = Box<dyn FnOnce(&mut sbrw_core::World) -> Result<String, String> + Send>;

async fn run(state: &AppState, command: &str, args: AdminRequest) -> Result<Value, String> {
    let world = state.world();
    let message = |reply: Result<String, String>| reply.map(|m| json!({ "message": m }));
    let call = |f: Operation| async move { world.call(move |w| Ok(f(w))).await.map_err(|fault| format!("{fault:#}"))? };
    match command {
        "ban" => {
            let target = required(args.target, "target")?;
            message(call(Box::new(move |w| ops::ban(w, &target, args.length.as_deref(), args.reason))).await)
        }
        "unban" => {
            let target = required(args.target, "target")?;
            message(call(Box::new(move |w| ops::unban(w, &target))).await)
        }
        "kick" => {
            let target = required(args.target, "target")?;
            message(call(Box::new(move |w| ops::kick(w, &target))).await)
        }
        "announce" => {
            let text = required(args.message, "message")?;
            message(call(Box::new(move |w| ops::announce(w, &text))).await)
        }
        "ticket" => {
            let name = required(args.discord_name, "discord_name")?;
            message(call(Box::new(move |w| ops::ticket(w, &name))).await)
        }
        "set-admin" => {
            let email = required(args.email, "email")?;
            let admin = args.admin.unwrap_or(true);
            message(call(Box::new(move |w| ops::set_admin(w, &email, admin))).await)
        }
        "set-password" => {
            let email = required(args.email, "email")?;
            let password = required(args.password, "password")?;
            message(call(Box::new(move |w| ops::set_password(w, &email, &password))).await)
        }
        "tokens" => {
            let tokens = world.call(|w| Ok(ops::tokens(w))).await.map_err(|fault| format!("{fault:#}"))?;
            Ok(Value::Object(tokens.into_iter().map(|(name, value)| (name.to_owned(), json!(value))).collect()))
        }
        "backup" => {
            let db = state.world_db.clone().ok_or("this server has no database file to back up")?;
            let copy = tokio::task::spawn_blocking(move || sbrw_core::store::backup_file(&db, "manual"))
                .await
                .map_err(|err| err.to_string())?
                .map_err(|err| format!("{err:#}"))?;
            Ok(json!({ "message": format!("Backed up to {}", copy.display()) }))
        }
        _ => Err(format!("unknown command {command}")),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

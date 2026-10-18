//! `api/RecoveryPassword`: asking for a password reset link by email, and
//! resetting with the link's key.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::recovery_password::{self, Recovery};

use super::{Call, entity};
use crate::AppState;

/// Asking or resetting refused because the address has failed too often lately.
fn too_many_failures(state: &AppState, call: &Call) -> Option<Response> {
    state.auth_limiter.blocked(call.remote_addr, std::time::Instant::now()).then(|| {
        let minutes = state.auth_limiter.wait().as_secs().div_ceil(60);
        entity(
            call,
            StatusCode::OK,
            format!("ERROR: Too many failed attempts from your address. Try again in {minutes} minutes."),
        )
    })
}

fn form(call: &Call, name: &str) -> Option<String> {
    call.form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

// port: api/RecoveryPassword#resetPassord(String,String,String)
pub(super) async fn reset_password(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (password, passwordconf, random_key) =
        (form(call, "password"), form(call, "passwordconf"), form(call, "randomKey"));
    let (Some(password), Some(passwordconf), Some(random_key)) = (password, passwordconf, random_key) else {
        return Ok(entity(call, StatusCode::OK, "ERROR: empty values!".into()));
    };
    if password.is_empty() || passwordconf.is_empty() || random_key.is_empty() {
        return Ok(entity(call, StatusCode::OK, "ERROR: empty values!".into()));
    }
    if password != passwordconf {
        return Ok(entity(call, StatusCode::OK, "ERROR: Passwords not match!".into()));
    }
    if let Some(refused) = too_many_failures(state, call) {
        return Ok(refused);
    }
    let body = state.world().call(move |w| recovery_password::reset_password(w, &password, &random_key)).await?;
    if body != "Password changed!" {
        state.auth_limiter.failed(call.remote_addr, std::time::Instant::now());
    }
    Ok(entity(call, StatusCode::OK, body.to_owned()))
}

// port: api/RecoveryPassword#forgotPassword(String)
pub(super) async fn forgot_password(state: &AppState, call: &Call) -> Result<Response, Fault> {
    if let Some(refused) = too_many_failures(state, call) {
        return Ok(refused);
    }
    let email = form(call, "email");
    let world = state.world();
    let body = match world.call(move |w| recovery_password::forgot_password(w, email.as_deref())).await? {
        Recovery::Done(body) => {
            if body == "ERROR: Invalid email!" {
                state.auth_limiter.failed(call.remote_addr, std::time::Instant::now());
            }
            body
        }
        Recovery::Send(mail) => match state.mailer() {
            Some(mailer) if mailer.send(&mail).await => {
                world.call(move |w| recovery_password::recovery_mail_sent(w, &mail)).await?
            }
            _ => "ERROR: Can't send email!".to_owned(),
        },
    };
    Ok(entity(call, StatusCode::OK, body))
}

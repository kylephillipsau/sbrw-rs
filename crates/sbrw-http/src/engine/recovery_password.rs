//! `api/RecoveryPassword`: asking for a password reset link by email, and
//! resetting with the link's key.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::recovery_password::{self, Recovery};

use super::{Call, entity};
use crate::AppState;

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
    let body = state.world().call(move |w| recovery_password::reset_password(w, &password, &random_key)).await?;
    Ok(entity(call, StatusCode::OK, body.to_owned()))
}

// port: api/RecoveryPassword#forgotPassword(String)
pub(super) async fn forgot_password(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let email = form(call, "email");
    let world = state.world();
    let body = match world.call(move |w| recovery_password::forgot_password(w, email.as_deref())).await? {
        Recovery::Done(body) => body,
        Recovery::Send(mail) => match state.mailer() {
            Some(mailer) if mailer.send(&mail).await => {
                world.call(move |w| recovery_password::recovery_mail_sent(w, &mail)).await?
            }
            _ => "ERROR: Can't send email!".to_owned(),
        },
    };
    Ok(entity(call, StatusCode::OK, body))
}

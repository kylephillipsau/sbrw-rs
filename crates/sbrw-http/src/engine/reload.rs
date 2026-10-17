//! `api/ReloadParameters` and `api/ReloadLoginAnnouncements`: reading the
//! parameters again and forgetting cached login announcements, for whoever
//! has the admin token.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::bo::login_announcement;
use sbrw_core::params::Parameters;
use sbrw_core::{Fault, World};

use super::{Call, entity};
use crate::AppState;

/// Upstream's check: no `ADMIN_AUTH`, the wrong token, or the action's
/// success message.
fn with_admin_token(
    w: &mut World,
    token: Option<String>,
    action: impl FnOnce(&mut World) -> Result<&'static str, Fault>,
) -> Result<&'static str, Fault> {
    let Some(admin_token) = w.params.get_str_param("ADMIN_AUTH") else {
        return Ok("ERROR! no admin token set in DB");
    };
    if token.as_deref() == Some(admin_token.as_str()) { action(w) } else { Ok("ERROR! invalid admin token") }
}

fn admin_auth(call: &Call) -> Option<String> {
    call.form.iter().find(|(k, _)| k == "adminAuth").map(|(_, v)| v.clone())
}

// port: api/ReloadParameters#reloadParameters(String,String)
pub(super) async fn reload_parameters(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let token = admin_auth(call);
    let body = state
        .world()
        .call(move |w| {
            with_admin_token(w, token, |w| {
                w.params = Parameters::load(w.db())?;
                tracing::info!("Loaded parameters from database");
                Ok("SUCCESS! reloaded parameters")
            })
        })
        .await?;
    Ok(entity(call, StatusCode::OK, body.to_owned()))
}

// port: api/ReloadLoginAnnouncements#reloadLoginAnnouncements(String,String)
pub(super) async fn reload_login_announcements(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let token = admin_auth(call);
    let body = state
        .world()
        .call(move |w| {
            with_admin_token(w, token, |w| {
                login_announcement::reset_cached_announcements(w);
                Ok("SUCCESS! cleared login announcements cache")
            })
        })
        .await?;
    Ok(entity(call, StatusCode::OK, body.to_owned()))
}

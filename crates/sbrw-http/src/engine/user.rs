//! `api/User`: the launcher's login and registration. The game's session calls
//! (`GetPermanentSession`, `SecureLoginPersona`, ...) come next.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{token_session, user};
use sbrw_xml::ToXml;

use super::{Call, entity};
use crate::AppState;

// port: api/User#authenticateUser(String,String)
pub(super) async fn authenticate_user(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (email, password) = (call.query("email"), call.query("password"));
    let remote_host = call.remote_addr.to_string();
    let status = state
        .world()
        .call(move |w| token_session::login(w, email.as_deref(), password.as_deref(), &remote_host))
        .await?;
    let code = if status.login_ok { StatusCode::OK } else { StatusCode::INTERNAL_SERVER_ERROR };
    Ok(entity(call, code, status.to_xml()))
}

// port: api/User#createUser(String,String,String)
pub(super) async fn create_user(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (email, password, ticket) = (call.query("email"), call.query("password"), call.query("inviteTicket"));
    let ip = call.remote_addr.to_string();
    let world = state.world();
    let status = {
        let (email, password, ip) = (email.clone(), password.clone(), ip.clone());
        world
            .call(move |w| {
                user::create_user_with_ticket(w, email.as_deref(), password.as_deref(), &ip, ticket.as_deref())
            })
            .await?
    };
    if status.login_ok {
        // Registration logs the new account in, in a second transaction.
        let status = world.call(move |w| token_session::login(w, email.as_deref(), password.as_deref(), &ip)).await?;
        return Ok(entity(call, StatusCode::OK, status.to_xml()));
    }
    Ok(entity(call, StatusCode::INTERNAL_SERVER_ERROR, status.to_xml()))
}

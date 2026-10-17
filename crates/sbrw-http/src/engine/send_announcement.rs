//! `api/SendAnnouncement`: a message to every player, for whoever has the
//! announcement token.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;

use super::{Call, entity};
use crate::AppState;

// port: api/SendAnnouncement#sendAnnouncement(String,String)
pub(super) async fn send_announcement(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let form = |name: &str| call.form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let (message, token) = (form("message"), form("announcementAuth"));
    let body = state
        .world()
        .call(move |w| {
            let Some(announcement_token) = w.params.get_str_param("ANNOUNCEMENT_AUTH") else {
                return Ok("ERROR! no announcement token set in DB");
            };
            if token.as_deref() == Some(announcement_token.as_str()) {
                w.openfire.send_chat_announcement(message.as_deref(), &w.effects);
                Ok("SUCCESS! sent announcement")
            } else {
                Ok("ERROR! invalid admin token")
            }
        })
        .await?;
    Ok(entity(call, StatusCode::OK, body.to_owned()))
}

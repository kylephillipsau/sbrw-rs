//! `api/MatchMaking`: launching an event. As upstream, each call into a BO
//! is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{event, token_session};
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{SecurityChallenge, SessionInfo};

use super::{Call, entity};
use crate::AppState;

// port: api/MatchMaking#launchEvent(int)
/// A single-player session of the event, with a fixed security challenge.
pub(super) async fn launch_event(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_id = call.path_int("eventId")?.unwrap_or_default();
    let session = call.session().clone();
    let world = state.world();
    let token = session.security_token.clone();
    let create_event_session = world.call(move |w| event::create_event_session(w, &session, event_id)).await?;

    let session_info = SessionInfo {
        challenge: Some(SecurityChallenge {
            challenge_id: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
            left_size: 14,
            pattern: Some("FFFFFFFFFFFFFFFF".into()),
            right_size: 50,
        }),
        event_id,
        session_id: create_event_session.ok_or_else(|| Fault::null_pointer("EventSessionEntity"))?.id,
    };
    world
        .call(move |w| {
            token_session::set_active_lobby_id(w, &token, Some(0));
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, session_info.to_xml()))
}

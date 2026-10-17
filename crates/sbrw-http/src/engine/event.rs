//! `api/Event`: starting an event session, leaving it, and finishing it.
//! As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{event, matchmaking, token_session};

use super::{Call, entity};
use crate::AppState;

// port: api/Event#abort(Long)
pub(super) async fn abort(state: &AppState, call: &Call) -> Result<Response, Fault> {
    call.query_long("eventSessionId")?;
    let token = call.session().security_token.clone();
    state
        .world()
        .call(move |w| {
            token_session::set_event_session_id(w, &token, None);
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/Event#launched(Long)
/// The driver starts the session: off the race-now queue, with their data
/// for the session created.
pub(super) async fn launched(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_session_id = call.query_long("eventSessionId")?;
    let session = call.session();
    let active_persona_id =
        session.active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
    let token = session.security_token.clone();
    let world = state.world();
    world
        .call(move |w| {
            matchmaking::remove_player_from_queue(w, active_persona_id);
            Ok(())
        })
        .await?;
    world.call(move |w| event::create_event_data_session(w, active_persona_id, event_session_id)).await?;
    world
        .call(move |w| {
            token_session::set_event_session_id(w, &token, event_session_id);
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

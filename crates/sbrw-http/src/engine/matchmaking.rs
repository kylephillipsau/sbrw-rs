//! `api/MatchMaking`: the lobby queues and invitations, and launching an
//! event. As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{event, lobby, matchmaking, persona, token_session};
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{SecurityChallenge, SessionInfo};

use super::{Call, entity};
use crate::AppState;

/// `RequestSessionInfo.getActivePersonaId()`, which BOs dereference.
fn active_persona_id(call: &Call) -> Result<i64, Fault> {
    call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))
}

// port: api/MatchMaking#joinQueueRaceNow()
/// Race now, in the default car's class. Without a car there's no class to
/// read.
pub(super) async fn join_queue_race_now(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = active_persona_id(call)?;
    let world = state.world();
    let default_car = world.call(move |w| persona::get_default_car(w, active_persona_id)).await?;
    let car_class_hash =
        default_car.custom_car.ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))?.car_class_hash;
    world.call(move |w| lobby::join_fast_lobby(w, active_persona_id, car_class_hash)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/MatchMaking#joinQueueEvent(int)
pub(super) async fn join_queue_event(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_id = call.path_int("eventId")?.unwrap_or_default();
    let active_persona_id = active_persona_id(call)?;
    state.world().call(move |w| lobby::join_queue_event(w, active_persona_id, event_id)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/MatchMaking#leaveQueue()
pub(super) async fn leave_queue(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = active_persona_id(call)?;
    state
        .world()
        .call(move |w| {
            matchmaking::remove_player_from_queue(w, active_persona_id);
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/MatchMaking#leavelobby()
/// Out of the lobby the session last accepted, if any.
pub(super) async fn leavelobby(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = call.session().active_persona_id;
    if let Some(active_lobby_id) = call.session().active_lobby_id.filter(|&id| id != 0) {
        let active_persona_id =
            active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
        state.world().call(move |w| lobby::remove_entrant_from_lobby(w, active_persona_id, active_lobby_id)).await?;
    }
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/MatchMaking#makePrivateLobby(int)
pub(super) async fn make_private_lobby(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_id = call.path_int("eventId")?.unwrap_or_default();
    let active_persona_id = active_persona_id(call)?;
    state.world().call(move |w| lobby::create_private_lobby(w, active_persona_id, event_id)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/MatchMaking#acceptInvite(Long)
/// The session's lobby becomes the invitation's, before it's accepted and
/// whether or not it can be.
pub(super) async fn accept_invite(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let lobby_invite_id = call.query_long("lobbyInviteId")?;
    let token = call.session().security_token.clone();
    let world = state.world();
    world
        .call(move |w| {
            token_session::set_active_lobby_id(w, &token, lobby_invite_id);
            Ok(())
        })
        .await?;
    let active_persona_id = active_persona_id(call)?;
    let lobby_info = world.call(move |w| lobby::acceptinvite(w, active_persona_id, lobby_invite_id)).await?;
    Ok(entity(call, StatusCode::OK, lobby_info.to_xml()))
}

// port: api/MatchMaking#declineInvite(Long)
pub(super) async fn decline_invite(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let lobby_invite_id = call.query_long("lobbyInviteId")?;
    let active_persona_id = active_persona_id(call)?;
    state.world().call(move |w| lobby::declineinvite(w, active_persona_id, lobby_invite_id)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

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

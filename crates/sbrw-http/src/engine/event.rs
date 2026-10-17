//! `api/Event`: starting an event session, leaving it, and finishing it.
//! As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{
    event, event_result, event_result_drag, event_result_pursuit, event_result_route, event_result_team_escape,
    matchmaking, token_session,
};
use sbrw_core::dao::event::EventMode;
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{
    DragArbitrationPacket, PursuitArbitrationPacket, RouteArbitrationPacket, TeamEscapeArbitrationPacket,
};

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

/// `RequestSessionInfo.getActivePersonaId()`, which BOs dereference.
fn active_persona_id(call: &Call) -> Result<i64, Fault> {
    call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))
}

// port: api/Event#arbitration(InputStream,Long)
/// The game's report of a finished event, read as the event's mode
/// expects, and the result; a meeting place has none. The session's event is
/// then cleared.
pub(super) async fn arbitration(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_session_id = call.query_long("eventSessionId")?;
    let world = state.world();
    let event_session_entity = world.call(move |w| event::find_event_session_by_id(w, event_session_id)).await?;
    let event_mode = EventMode::from_id(event_session_entity.event.event_mode_id)?;
    let active_persona_id = active_persona_id(call)?;

    let event_result = match event_mode {
        EventMode::Circuit | EventMode::Sprint => {
            let route_arbitration_packet: RouteArbitrationPacket = call.unmarshal()?;
            Some(
                world
                    .call(move |w| {
                        event_result::handle(
                            w,
                            event_session_entity,
                            active_persona_id,
                            route_arbitration_packet,
                            event_result_route::handle_internal,
                        )
                    })
                    .await?
                    .to_xml(),
            )
        }
        EventMode::Drag => {
            let drag_arbitration_packet: DragArbitrationPacket = call.unmarshal()?;
            Some(
                world
                    .call(move |w| {
                        event_result::handle(
                            w,
                            event_session_entity,
                            active_persona_id,
                            drag_arbitration_packet,
                            event_result_drag::handle_internal,
                        )
                    })
                    .await?
                    .to_xml(),
            )
        }
        EventMode::PursuitMp => {
            let team_escape_arbitration_packet: TeamEscapeArbitrationPacket = call.unmarshal()?;
            Some(
                world
                    .call(move |w| {
                        event_result::handle(
                            w,
                            event_session_entity,
                            active_persona_id,
                            team_escape_arbitration_packet,
                            event_result_team_escape::handle_internal,
                        )
                    })
                    .await?
                    .to_xml(),
            )
        }
        EventMode::PursuitSp => {
            let pursuit_arbitration_packet: PursuitArbitrationPacket = call.unmarshal()?;
            Some(
                world
                    .call(move |w| {
                        event_result::handle(
                            w,
                            event_session_entity,
                            active_persona_id,
                            pursuit_arbitration_packet,
                            event_result_pursuit::handle_internal,
                        )
                    })
                    .await?
                    .to_xml(),
            )
        }
        EventMode::Meetingplace => None,
    };

    let token = call.session().security_token.clone();
    world
        .call(move |w| {
            token_session::set_event_session_id(w, &token, None);
            Ok(())
        })
        .await?;

    Ok(entity(call, StatusCode::OK, event_result.unwrap_or_default()))
}

// port: api/Event#bust(InputStream,String,Long)
/// A pursuit the driver was busted in, reported apart from arbitration. The
/// session's event isn't cleared.
pub(super) async fn bust(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let event_session_id = call.query_long("eventSessionId")?;
    let world = state.world();
    let event_session_entity = world.call(move |w| event::find_event_session_by_id(w, event_session_id)).await?;
    let pursuit_arbitration_packet: PursuitArbitrationPacket = call.unmarshal()?;
    let active_persona_id = active_persona_id(call)?;
    let pursuit_event_result = world
        .call(move |w| {
            event_result::handle(
                w,
                event_session_entity,
                active_persona_id,
                pursuit_arbitration_packet,
                event_result_pursuit::handle_internal,
            )
        })
        .await?;
    Ok(entity(call, StatusCode::OK, pursuit_event_result.to_xml()))
}

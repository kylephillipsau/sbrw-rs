//! `EventResultRouteBO`: finishing a sprint or circuit.

use sbrw_xml::dto::http::{ArrayOfRouteEntrantResult, RouteArbitrationPacket, RouteEntrantResult, RouteEventResult};
use sbrw_xml::dto::xmpp::{XMPP_ResponseTypeRouteEntrantResult, XMPP_RouteEntrantResultType};

use crate::bo::{achievement, car_damage, dnf_timer, event_result, matchmaking, reward_route};
use crate::dao::event_data::{self, EventDataEntity};
use crate::dao::event_session::{self, EventSessionEntity};
use crate::dao::persona;
use crate::engine::{EngineException, EngineExceptionCode};
use crate::xmpp_event::XmppEvent;
use crate::{Fault, World, datetime};

/// The driver's data for the session, which must not be finished yet.
pub(crate) fn unfinished_event_data(
    w: &World,
    active_persona_id: i64,
    event_session_id: i64,
) -> Result<EventDataEntity, Fault> {
    let event_data_entity =
        event_data::find_by_persona_and_event_session_id(w.db(), active_persona_id, event_session_id)?
            .ok_or_else(|| Fault::null_pointer("EventDataEntity"))?;

    if event_data_entity.finish_reason != 0 {
        return Err(Fault::Engine(EngineException::with_message(
            "Session already completed.",
            EngineExceptionCode::SecurityKickedArbitration,
            true,
        )));
    }
    Ok(event_data_entity)
}

// port: bo/EventResultRouteBO#handleInternal(EventSessionEntity,Long,RouteArbitrationPacket)
/// The packet into the driver's event data, every racer's result, the
/// rewards, the damage, the other racers told (and given their DNF
/// countdown when the driver won), and the EVENT achievements.
pub fn handle_internal(
    w: &mut World,
    mut event_session_entity: EventSessionEntity,
    active_persona_id: i64,
    route_arbitration_packet: RouteArbitrationPacket,
) -> Result<RouteEventResult, Fault> {
    let event_session_id = event_session_entity.id;

    let mut event_data_entity = unfinished_event_data(w, active_persona_id, event_session_id)?;

    event_result::prepare_basic_event_data(w, &mut event_data_entity, active_persona_id, &route_arbitration_packet);

    let p = &route_arbitration_packet;
    event_data_entity.best_lap_duration_in_milliseconds = p.best_lap_duration_in_milliseconds;
    event_data_entity.fraction_completed = p.fraction_completed;
    event_data_entity.longest_jump_duration_in_milliseconds = p.longest_jump_duration_in_milliseconds;
    event_data_entity.number_of_collisions = p.number_of_collisions;
    event_data_entity.perfect_start = p.perfect_start;
    event_data_entity.sum_of_jumps_duration_in_milliseconds = p.sum_of_jumps_duration_in_milliseconds;
    event_data_entity.top_speed = p.top_speed;
    event_session_entity.ended = Some(datetime::current_time_millis(&w.env));

    let mut array_of_route_entrant_result = ArrayOfRouteEntrantResult::default();
    for racer in event_result::get_racers(w, &event_data_entity)? {
        array_of_route_entrant_result.route_entrant_result.push(RouteEntrantResult {
            best_lap_duration_in_milliseconds: racer.best_lap_duration_in_milliseconds,
            event_duration_in_milliseconds: racer.event_duration_in_milliseconds,
            event_session_id,
            finish_reason: racer.finish_reason,
            persona_id: racer.persona_id.ok_or_else(|| Fault::null_pointer("EventDataEntity.personaId"))?,
            ranking: racer.rank,
            top_speed: racer.top_speed,
        });
    }

    let mut persona_entity =
        persona::find(w.db(), active_persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut transaction = achievement::create_transaction(active_persona_id);
    let accolades = reward_route::get_accolades(
        w,
        active_persona_id,
        &route_arbitration_packet,
        &mut event_data_entity,
        &event_session_entity,
        &mut transaction,
        &mut persona_entity,
    )?;
    let durability =
        car_damage::induce_car_damage(w, active_persona_id, &route_arbitration_packet, &event_data_entity.event)?;
    let race_again = event_result::prepare_race_again(w, &mut event_session_entity)?;
    let route_event_result = RouteEventResult {
        accolades: Some(accolades),
        durability,
        entrants: Some(array_of_route_entrant_result),
        event_id: event_data_entity.event.id,
        event_session_id,
        persona_id: active_persona_id,
        exit_path: Some(race_again.exit_path),
        lobby_invite_id: race_again.lobby_invite_id,
        invite_lifetime_in_milliseconds: race_again.invite_lifetime_in_milliseconds,
    };
    send_xmpp_packet(w, &event_session_entity, active_persona_id, &route_arbitration_packet)?;

    event_result::update_event_achievements(
        w,
        &event_data_entity,
        &event_session_entity,
        &mut persona_entity,
        &route_arbitration_packet,
        &mut transaction,
    )?;
    achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;

    event_session::update(w.db(), &event_session_entity)?;
    event_data::update(w.db(), &event_data_entity)?;

    reset_ignored_events(w, &event_session_entity, active_persona_id)?;

    Ok(route_event_result)
}

/// After a public lobby's race, the driver's ignored events are forgotten.
pub(crate) fn reset_ignored_events(
    w: &mut World,
    event_session_entity: &EventSessionEntity,
    active_persona_id: i64,
) -> Result<(), Fault> {
    if let Some(lobby) = &event_session_entity.lobby
        && !lobby.is_private.ok_or_else(|| Fault::null_pointer("LobbyEntity.isPrivate"))?
    {
        matchmaking::reset_ignored_events(w, active_persona_id);
    }
    Ok(())
}

// port: bo/EventResultRouteBO#sendXmppPacket(EventSessionEntity,Long,RouteArbitrationPacket)
/// Each other racer hears the driver's result; when the driver won, they
/// also hear the DNF countdown has started, if the event has one.
fn send_xmpp_packet(
    w: &mut World,
    event_session_entity: &EventSessionEntity,
    active_persona_id: i64,
    route_arbitration_packet: &RouteArbitrationPacket,
) -> Result<(), Fault> {
    let p = route_arbitration_packet;
    let route_entrant_result_response = XMPP_ResponseTypeRouteEntrantResult {
        route_entrant_result: Some(XMPP_RouteEntrantResultType {
            best_lap_duration_in_milliseconds: Some(p.best_lap_duration_in_milliseconds),
            event_duration_in_milliseconds: Some(p.event_duration_in_milliseconds),
            event_session_id: Some(event_session_entity.id),
            finish_reason: Some(p.finish_reason),
            persona_id: Some(active_persona_id),
            ranking: Some(p.rank),
            top_speed: Some(p.top_speed),
        }),
        ..Default::default()
    };

    for racer in event_data::get_racers(w.db(), event_session_entity.id)? {
        let racer_persona_id = racer.persona_id.ok_or_else(|| Fault::null_pointer("EventDataEntity.personaId"))?;
        if racer_persona_id != active_persona_id {
            let xmpp_event = XmppEvent::new(racer_persona_id);
            xmpp_event.send_race_end(w, &route_entrant_result_response);
            if p.finish_reason == 22 && p.rank == 1 && event_session_entity.event.is_dnf_enabled {
                xmpp_event.send_event_timing_out(w, event_session_entity);
                dnf_timer::schedule_dnf(w, event_session_entity, racer_persona_id);
            }
        }
    }
    Ok(())
}

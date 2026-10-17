//! `EventResultDragBO`: finishing a drag.

use sbrw_xml::dto::http::{ArrayOfDragEntrantResult, DragArbitrationPacket, DragEntrantResult, DragEventResult};
use sbrw_xml::dto::xmpp::{XMPP_DragEntrantResultType, XMPP_ResponseTypeDragEntrantResult};

use crate::bo::event_result_route::{reset_ignored_events, unfinished_event_data};
use crate::bo::{achievement, car_damage, dnf_timer, event_result, reward_drag};
use crate::dao::event_data;
use crate::dao::event_session::{self, EventSessionEntity};
use crate::dao::persona;
use crate::xmpp_event::XmppEvent;
use crate::{Fault, World, datetime};

// port: bo/EventResultDragBO#handleInternal(EventSessionEntity,Long,DragArbitrationPacket)
/// As a sprint's result, but the other racers are told as their results are
/// listed, before the rewards.
pub fn handle_internal(
    w: &mut World,
    mut event_session_entity: EventSessionEntity,
    active_persona_id: i64,
    drag_arbitration_packet: DragArbitrationPacket,
) -> Result<DragEventResult, Fault> {
    let event_session_id = event_session_entity.id;
    let p = &drag_arbitration_packet;

    let drag_entrant_result_response = XMPP_ResponseTypeDragEntrantResult {
        drag_entrant_result: Some(XMPP_DragEntrantResultType {
            event_duration_in_milliseconds: Some(p.event_duration_in_milliseconds),
            event_session_id: Some(event_session_id),
            finish_reason: Some(p.finish_reason),
            persona_id: Some(active_persona_id),
            ranking: Some(p.rank),
            top_speed: Some(p.top_speed),
        }),
        ..Default::default()
    };

    let mut event_data_entity = unfinished_event_data(w, active_persona_id, event_session_id)?;

    event_result::prepare_basic_event_data(w, &mut event_data_entity, active_persona_id, p);
    event_data_entity.fraction_completed = p.fraction_completed;
    event_data_entity.longest_jump_duration_in_milliseconds = p.longest_jump_duration_in_milliseconds;
    event_data_entity.number_of_collisions = p.number_of_collisions;
    event_data_entity.perfect_start = p.perfect_start;
    event_data_entity.sum_of_jumps_duration_in_milliseconds = p.sum_of_jumps_duration_in_milliseconds;
    event_data_entity.top_speed = p.top_speed;
    event_session_entity.ended = Some(datetime::current_time_millis(&w.env));

    let mut array_of_drag_entrant_result = ArrayOfDragEntrantResult::default();
    for racer in event_result::get_racers(w, &event_data_entity)? {
        let racer_persona_id = racer.persona_id.ok_or_else(|| Fault::null_pointer("EventDataEntity.personaId"))?;
        array_of_drag_entrant_result.drag_entrant_result.push(DragEntrantResult {
            event_duration_in_milliseconds: racer.event_duration_in_milliseconds,
            event_session_id,
            finish_reason: racer.finish_reason,
            persona_id: racer_persona_id,
            ranking: racer.rank,
            top_speed: racer.top_speed,
        });

        if racer_persona_id != active_persona_id {
            let xmpp_event = XmppEvent::new(racer_persona_id);
            xmpp_event.send_drag_end(w, &drag_entrant_result_response);
            if p.finish_reason == 22 && p.rank == 1 && event_session_entity.event.is_dnf_enabled {
                xmpp_event.send_event_timing_out(w, &event_session_entity);
                dnf_timer::schedule_dnf(w, &event_session_entity, racer_persona_id);
            }
        }
    }

    let mut persona_entity =
        persona::find(w.db(), active_persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut transaction = achievement::create_transaction(active_persona_id);
    let accolades = reward_drag::get_accolades(
        w,
        active_persona_id,
        p,
        &mut event_data_entity,
        &event_session_entity,
        &mut transaction,
        &mut persona_entity,
    )?;
    let durability = car_damage::induce_car_damage(w, active_persona_id, p, &event_data_entity.event)?;
    let drag_event_result = DragEventResult {
        accolades: Some(accolades),
        durability,
        entrants: Some(array_of_drag_entrant_result),
        event_id: event_data_entity.event.id,
        event_session_id,
        persona_id: active_persona_id,
        exit_path: Some(event_result::prepare_race_again(&event_session_entity)?),
        ..Default::default()
    };
    event_result::update_event_achievements(
        w,
        &event_data_entity,
        &event_session_entity,
        &mut persona_entity,
        p,
        &mut transaction,
    )?;
    achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;

    event_session::update(w.db(), &event_session_entity)?;
    event_data::update(w.db(), &event_data_entity)?;

    reset_ignored_events(w, &event_session_entity, active_persona_id)?;

    Ok(drag_event_result)
}

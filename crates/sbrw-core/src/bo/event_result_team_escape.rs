//! `EventResultTeamEscapeBO`: finishing a team escape.

use sbrw_xml::dto::http::{
    ArrayOfTeamEscapeEntrantResult, TeamEscapeArbitrationPacket, TeamEscapeEntrantResult, TeamEscapeEventResult,
};
use sbrw_xml::dto::xmpp::{XMPP_ResponseTypeTeamEscapeEntrantResult, XMPP_TeamEscapeEntrantResultType};

use crate::bo::event_result_route::{reset_ignored_events, unfinished_event_data};
use crate::bo::{achievement, car_damage, dnf_timer, event_result, reward_team_escape};
use crate::dao::event_data;
use crate::dao::event_session::{self, EventSessionEntity};
use crate::dao::persona;
use crate::xmpp_event::XmppEvent;
use crate::{Fault, World, datetime};

// port: bo/EventResultTeamEscapeBO#handleInternal(EventSessionEntity,Long,TeamEscapeArbitrationPacket)
/// As a sprint's result, with the other racers told as their results are
/// listed (the DNF countdown starts when the driver is first, escaped with
/// reason 518 or 22). Only an escape without being busted counts for EVENT
/// achievements.
pub fn handle_internal(
    w: &mut World,
    mut event_session_entity: EventSessionEntity,
    active_persona_id: i64,
    mut team_escape_arbitration_packet: TeamEscapeArbitrationPacket,
) -> Result<TeamEscapeEventResult, Fault> {
    let event_session_id = event_session_entity.id;
    let p = &team_escape_arbitration_packet;

    let team_escape_entrant_result_response = XMPP_ResponseTypeTeamEscapeEntrantResult {
        team_escape_entrant_result: Some(XMPP_TeamEscapeEntrantResultType {
            event_duration_in_milliseconds: Some(p.event_duration_in_milliseconds),
            event_session_id: Some(event_session_id),
            finish_reason: Some(p.finish_reason),
            persona_id: Some(active_persona_id),
            ..Default::default()
        }),
        ..Default::default()
    };

    let mut event_data_entity = unfinished_event_data(w, active_persona_id, event_session_id)?;

    event_result::prepare_basic_event_data(w, &mut event_data_entity, active_persona_id, p);
    event_data_entity.busted_count = p.busted_count;
    event_data_entity.cops_deployed = p.cops_deployed;
    event_data_entity.cops_disabled = p.cops_disabled;
    event_data_entity.cops_rammed = p.cops_rammed;
    event_data_entity.cost_to_state = p.cost_to_state;
    event_data_entity.distance_to_finish = p.distance_to_finish;
    event_data_entity.fraction_completed = p.fraction_completed;
    event_data_entity.infractions = p.infractions;
    event_data_entity.longest_jump_duration_in_milliseconds = p.longest_jump_duration_in_milliseconds;
    event_data_entity.number_of_collisions = p.number_of_collisions;
    event_data_entity.perfect_start = p.perfect_start;
    event_data_entity.road_blocks_dodged = p.road_blocks_dodged;
    event_data_entity.spike_strips_dodged = p.spike_strips_dodged;
    event_data_entity.sum_of_jumps_duration_in_milliseconds = p.sum_of_jumps_duration_in_milliseconds;
    event_data_entity.top_speed = p.top_speed;
    event_session_entity.ended = Some(datetime::current_time_millis(&w.env));

    let mut array_of_team_escape_entrant_result = ArrayOfTeamEscapeEntrantResult::default();
    for racer in event_result::get_racers(w, &event_data_entity)? {
        let racer_persona_id = racer.persona_id.ok_or_else(|| Fault::null_pointer("EventDataEntity.personaId"))?;
        array_of_team_escape_entrant_result.team_escape_entrant_result.push(TeamEscapeEntrantResult {
            distance_to_finish: racer.distance_to_finish,
            event_duration_in_milliseconds: racer.event_duration_in_milliseconds,
            event_session_id,
            finish_reason: racer.finish_reason,
            fraction_completed: racer.fraction_completed,
            persona_id: racer_persona_id,
            ranking: racer.rank,
        });

        if racer_persona_id != active_persona_id {
            let xmpp_event = XmppEvent::new(racer_persona_id);
            xmpp_event.send_team_escape_end(w, &team_escape_entrant_result_response);
            if (p.finish_reason == 518 || p.finish_reason == 22)
                && p.rank == 1
                && event_session_entity.event.is_dnf_enabled
            {
                xmpp_event.send_event_timing_out(w, &event_session_entity);
                dnf_timer::schedule_dnf(w, &event_session_entity, racer_persona_id);
            }
        }
    }

    let mut persona_entity =
        persona::find(w.db(), active_persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut transaction = achievement::create_transaction(active_persona_id);
    let accolades = reward_team_escape::get_accolades(
        w,
        active_persona_id,
        &mut team_escape_arbitration_packet,
        &mut event_data_entity,
        &event_session_entity,
        &mut transaction,
        &mut persona_entity,
    )?;
    let p = &team_escape_arbitration_packet;
    let durability = car_damage::induce_car_damage(w, active_persona_id, p, &event_data_entity.event)?;
    let race_again = event_result::prepare_race_again(w, &mut event_session_entity)?;
    let team_escape_event_result = TeamEscapeEventResult {
        accolades: Some(accolades),
        durability,
        entrants: Some(array_of_team_escape_entrant_result),
        event_id: event_data_entity.event.id,
        event_session_id,
        persona_id: active_persona_id,
        exit_path: Some(race_again.exit_path),
        lobby_invite_id: race_again.lobby_invite_id,
        invite_lifetime_in_milliseconds: race_again.invite_lifetime_in_milliseconds,
    };

    if p.busted_count == 0 {
        event_result::update_event_achievements(
            w,
            &event_data_entity,
            &event_session_entity,
            &mut persona_entity,
            p,
            &mut transaction,
        )?;
    }

    achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;

    event_session::update(w.db(), &event_session_entity)?;
    event_data::update(w.db(), &event_data_entity)?;

    reset_ignored_events(w, &event_session_entity, active_persona_id)?;

    Ok(team_escape_event_result)
}

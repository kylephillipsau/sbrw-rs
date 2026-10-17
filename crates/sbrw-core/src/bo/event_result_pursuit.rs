//! `EventResultPursuitBO`: finishing, or being busted in, a single-player
//! pursuit.

use sbrw_xml::dto::http::{ExitPath, PursuitArbitrationPacket, PursuitEventResult};

use crate::bo::event_result_route::unfinished_event_data;
use crate::bo::{achievement, car_damage, event_result, reward, reward_pursuit};
use crate::dao::car;
use crate::dao::event_data;
use crate::dao::event_session::{self, EventSessionEntity};
use crate::dao::persona;
use crate::{Fault, World, datetime};

// port: bo/EventResultPursuitBO#handleInternal(EventSessionEntity,Long,PursuitArbitrationPacket)
/// The packet into the driver's event data, the rewards and damage, and the
/// heat left on the default car (1 when busted, reason 266). A busted
/// pursuit doesn't count for EVENT achievements. The game reports rank 0;
/// the event data keeps it, and the rewards see 1.
pub fn handle_internal(
    w: &mut World,
    mut event_session_entity: EventSessionEntity,
    active_persona_id: i64,
    mut pursuit_arbitration_packet: PursuitArbitrationPacket,
) -> Result<PursuitEventResult, Fault> {
    let event_session_id = event_session_entity.id;

    let mut event_data_entity = unfinished_event_data(w, active_persona_id, event_session_id)?;

    event_result::prepare_basic_event_data(w, &mut event_data_entity, active_persona_id, &pursuit_arbitration_packet);
    let p = &pursuit_arbitration_packet;
    event_data_entity.cops_deployed = p.cops_deployed;
    event_data_entity.cops_disabled = p.cops_disabled;
    event_data_entity.cops_rammed = p.cops_rammed;
    event_data_entity.cost_to_state = p.cost_to_state;
    event_data_entity.heat = p.heat;
    event_data_entity.infractions = p.infractions;
    event_data_entity.longest_jump_duration_in_milliseconds = p.longest_jump_duration_in_milliseconds;
    event_data_entity.road_blocks_dodged = p.road_blocks_dodged;
    event_data_entity.spike_strips_dodged = p.spike_strips_dodged;
    event_data_entity.sum_of_jumps_duration_in_milliseconds = p.sum_of_jumps_duration_in_milliseconds;
    event_data_entity.top_speed = p.top_speed;
    event_session_entity.ended = Some(datetime::current_time_millis(&w.env));

    pursuit_arbitration_packet.rank = 1; // there's only ever 1 player, and the game sets rank to 0... idk why

    let is_busted = pursuit_arbitration_packet.finish_reason == 266;

    let mut persona_entity =
        persona::find(w.db(), active_persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut transaction = achievement::create_transaction(active_persona_id);
    let accolades = reward_pursuit::get_accolades(
        w,
        active_persona_id,
        &mut pursuit_arbitration_packet,
        &mut event_data_entity,
        &event_session_entity,
        &mut transaction,
        &mut persona_entity,
    )?;
    let durability =
        car_damage::induce_car_damage(w, active_persona_id, &pursuit_arbitration_packet, &event_data_entity.event)?;
    let heat = if is_busted { 1.0 } else { pursuit_arbitration_packet.heat };
    let pursuit_event_result = PursuitEventResult {
        accolades: Some(accolades),
        durability,
        event_id: event_data_entity.event.id,
        event_session_id,
        exit_path: Some(ExitPath::EXIT_TO_FREEROAM),
        heat,
        invite_lifetime_in_milliseconds: 0,
        lobby_invite_id: 0,
        persona_id: active_persona_id,
    };

    if !is_busted {
        event_result::update_event_achievements(
            w,
            &event_data_entity,
            &event_session_entity,
            &mut persona_entity,
            &pursuit_arbitration_packet,
            &mut transaction,
        )?;
    }

    achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;

    let mut car_entity =
        reward::default_car_of(w, &mut persona_entity)?.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    car_entity.heat = heat;
    car::update(w.db(), &car_entity)?;
    event_data::update(w.db(), &event_data_entity)?;
    event_session::update(w.db(), &event_session_entity)?;

    Ok(pursuit_event_result)
}

//! `LegitRaceBO`: whether a finished event counts.

use crate::bo::social;
use crate::bo::util::arbitration::{Arbitration, PacketRef};
use crate::dao::event_data::EventDataEntity;
use crate::dao::event_session::EventSessionEntity;
use crate::{Fault, World};

// port: bo/LegitRaceBO#isLegit(Long,ArbitrationPacket,EventSessionEntity,EventDataEntity)
/// Not when the server timed the event faster than its `legitTime`, nor
/// when the game detected hacks (both reported, as the server, for the
/// driver); nor a pursuit with more cops disabled than deployed, or one with
/// no top speed but infractions (not reported). Pursuits ending with reason
/// 8202 skip the cop checks.
pub fn is_legit(
    w: &World,
    active_persona_id: i64,
    arbitration_packet: &dyn Arbitration,
    session_entity: &EventSessionEntity,
    data_entity: &EventDataEntity,
) -> Result<bool, Fault> {
    let minimum_time = session_entity.event.legit_time;
    let legit = data_entity.server_time_in_milliseconds >= minimum_time;

    if !legit {
        social::send_report(
            w,
            Some(0),
            Some(active_persona_id),
            Some(3),
            Some(format!(
                "Abnormal event time: {} (below minimum of {} on event {}; session {})",
                data_entity.server_time_in_milliseconds, minimum_time, session_entity.event.id, session_entity.id
            )),
            Some(arbitration_packet.car_id() as i32),
            Some(0),
            arbitration_packet.hacks_detected(),
        )?;
        return Ok(false);
    }

    if arbitration_packet.hacks_detected() > 0 {
        social::send_report(
            w,
            Some(0),
            Some(active_persona_id),
            Some(3),
            Some(format!(
                "hacksDetected={} (event {}; session {})",
                arbitration_packet.hacks_detected(),
                session_entity.event.id,
                session_entity.id
            )),
            Some(arbitration_packet.car_id() as i32),
            Some(0),
            arbitration_packet.hacks_detected(),
        )?;
        return Ok(false);
    }

    match arbitration_packet.packet() {
        PacketRef::TeamEscape(team_escape_arbitration_packet)
            if team_escape_arbitration_packet.finish_reason != 8202 =>
        {
            return Ok(team_escape_arbitration_packet.cops_disabled <= team_escape_arbitration_packet.cops_deployed);
        }
        PacketRef::Pursuit(pursuit_arbitration_packet) if pursuit_arbitration_packet.finish_reason != 8202 => {
            if pursuit_arbitration_packet.cops_disabled > pursuit_arbitration_packet.cops_deployed {
                return Ok(false);
            }

            return Ok(pursuit_arbitration_packet.top_speed != 0.0 || pursuit_arbitration_packet.infractions == 0);
        }
        _ => {}
    }

    Ok(true)
}

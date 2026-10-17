//! `RewardDragBO`: what drags pay.

use sbrw_xml::dto::http::{Accolades, DragArbitrationPacket};

use crate::bo::achievement_transaction::AchievementTransaction;
use crate::bo::legit_race;
use crate::bo::reward::{self, SkillModRewardType};
use crate::dao::event_data::EventDataEntity;
use crate::dao::event_session::EventSessionEntity;
use crate::dao::persona::PersonaEntity;
use crate::{Fault, World};

// port: bo/RewardDragBO#getAccolades(Long,DragArbitrationPacket,EventDataEntity,EventSessionEntity,AchievementTransaction)
/// Nothing unless the race is legit and finished (reason 22). Otherwise what
/// a sprint pays: base, rank, perfect start, top speed, socialite skill mods,
/// multipliers and amplifiers, paid at once.
/// `persona_entity` is the driver, as the caller loaded them.
pub fn get_accolades(
    w: &mut World,
    active_persona_id: i64,
    drag_arbitration_packet: &DragArbitrationPacket,
    event_data_entity: &mut EventDataEntity,
    event_session_entity: &EventSessionEntity,
    achievement_transaction: &mut AchievementTransaction,
    persona_entity: &mut PersonaEntity,
) -> Result<Accolades, Fault> {
    let finish_reason = drag_arbitration_packet.finish_reason;
    let legit =
        legit_race::is_legit(w, active_persona_id, drag_arbitration_packet, event_session_entity, event_data_entity)?;
    event_data_entity.is_legit = legit;
    if !legit || finish_reason != 22 {
        return Ok(Accolades::default());
    }
    let mut reward_vo = reward::get_reward_vo(w, persona_entity)?;
    let event_reward_entity = reward::get_reward_configuration(w, event_session_entity)?;

    reward::set_base_reward(
        w,
        persona_entity,
        &event_session_entity.event,
        &event_reward_entity,
        drag_arbitration_packet,
        &mut reward_vo,
    )?;
    reward::set_rank_reward(&event_reward_entity, drag_arbitration_packet, &mut reward_vo);
    reward::set_perfect_start_reward(&event_reward_entity, drag_arbitration_packet.perfect_start, &mut reward_vo);
    reward::set_top_speed_reward(&event_reward_entity, drag_arbitration_packet.top_speed, &mut reward_vo);
    reward::set_skill_multiplier_reward(w, persona_entity, &mut reward_vo, SkillModRewardType::Socialite)?;
    reward::set_multiplier_reward(&event_reward_entity, &mut reward_vo);
    reward::set_amplifier_reward(w, persona_entity, &mut reward_vo)?;

    reward::apply_race_reward(
        w,
        reward_vo.rep(),
        reward_vo.cash(),
        persona_entity,
        true,
        Some(achievement_transaction),
    )?;
    reward::get_accolades(w, persona_entity, &event_reward_entity, drag_arbitration_packet, &reward_vo)
}

//! `RewardPursuitBO`: what single-player pursuits pay.

use sbrw_xml::dto::http::{Accolades, EnumRewardType, PursuitArbitrationPacket};

use crate::bo::achievement_transaction::AchievementTransaction;
use crate::bo::legit_race;
use crate::bo::reward::{self, SkillModRewardType};
use crate::bo::util::reward_vo::RewardVO;
use crate::dao::event::EventEntity;
use crate::dao::event_data::EventDataEntity;
use crate::dao::event_reward::{self, EventRewardEntity};
use crate::dao::event_session::EventSessionEntity;
use crate::dao::persona::PersonaEntity;
use crate::{Fault, World};

// port: bo/RewardPursuitBO#getAccolades(Long,PursuitArbitrationPacket,EventDataEntity,EventSessionEntity,AchievementTransaction)
/// Nothing unless the pursuit is legit and escaped (reason 518). Otherwise
/// the pursuit rewards from the single-player configuration, whatever the
/// session; then the packet's rank is drawn from 1 to 3 for the lucky draw.
pub fn get_accolades(
    w: &mut World,
    active_persona_id: i64,
    pursuit_arbitration_packet: &mut PursuitArbitrationPacket,
    event_data_entity: &mut EventDataEntity,
    event_session_entity: &EventSessionEntity,
    achievement_transaction: &mut AchievementTransaction,
    persona_entity: &mut PersonaEntity,
) -> Result<Accolades, Fault> {
    let finish_reason = pursuit_arbitration_packet.finish_reason;
    let legit = legit_race::is_legit(
        w,
        active_persona_id,
        &*pursuit_arbitration_packet,
        event_session_entity,
        event_data_entity,
    )?;
    event_data_entity.is_legit = legit;
    let is_busted = pursuit_arbitration_packet.finish_reason == 266;
    if !legit || is_busted || finish_reason != 518 {
        return Ok(Accolades::default());
    }
    let event_entity = &event_session_entity.event;
    let mut reward_vo = reward::get_reward_vo(w, persona_entity)?;
    let event_reward_entity = event_reward::find(w.db(), &event_entity.singleplayer_reward_config_id)?
        .ok_or_else(|| Fault::null_pointer("singleplayerRewardConfig"))?;

    set_pursuit_rewards(
        w,
        persona_entity,
        &event_session_entity.event,
        &event_reward_entity,
        pursuit_arbitration_packet,
        &mut reward_vo,
    )?;

    pursuit_arbitration_packet.rank = w.env.random(|r| r.next_int_bounded(4 - 1)) + 1;
    reward::apply_race_reward(
        w,
        reward_vo.rep(),
        reward_vo.cash(),
        persona_entity,
        true,
        Some(achievement_transaction),
    )?;
    reward::get_accolades(w, persona_entity, &event_reward_entity, &*pursuit_arbitration_packet, &reward_vo)
}

// port: bo/RewardPursuitBO#setPursuitRewards(PersonaEntity,EventEntity,EventRewardEntity,PursuitArbitrationPacket,RewardVO)
/// The base, then each pursuit statistic, top speed, bounty hunter skill
/// mods, the event's multipliers and the driver's amplifiers.
fn set_pursuit_rewards(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    event_entity: &EventEntity,
    event_reward_entity: &EventRewardEntity,
    pursuit_arbitration_packet: &PursuitArbitrationPacket,
    reward_vo: &mut RewardVO,
) -> Result<(), Fault> {
    let p = pursuit_arbitration_packet;
    reward::set_base_reward(w, persona_entity, event_entity, event_reward_entity, p, reward_vo)?;
    reward::set_pursit_param_reward(w, p.cops_deployed as f32, EnumRewardType::COP_CARS_DEPLOYED, reward_vo)?;
    reward::set_pursit_param_reward(w, p.cops_disabled as f32, EnumRewardType::COP_CARS_DISABLED, reward_vo)?;
    reward::set_pursit_param_reward(w, p.cops_rammed as f32, EnumRewardType::COP_CARS_RAMMED, reward_vo)?;
    reward::set_pursit_param_reward(w, p.cost_to_state as f32, EnumRewardType::COST_TO_STATE, reward_vo)?;
    reward::set_pursit_param_reward(
        w,
        p.event_duration_in_milliseconds as f32,
        EnumRewardType::PURSUIT_LENGTH,
        reward_vo,
    )?;
    reward::set_pursit_param_reward(w, p.heat, EnumRewardType::HEAT_LEVEL, reward_vo)?;
    reward::set_pursit_param_reward(w, p.infractions as f32, EnumRewardType::INFRACTIONS, reward_vo)?;
    reward::set_pursit_param_reward(w, p.road_blocks_dodged as f32, EnumRewardType::ROADBLOCKS_DODGED, reward_vo)?;
    reward::set_pursit_param_reward(w, p.spike_strips_dodged as f32, EnumRewardType::SPIKE_STRIPS_DODGED, reward_vo)?;

    reward::set_top_speed_reward(event_reward_entity, p.top_speed, reward_vo);
    reward::set_skill_multiplier_reward(w, persona_entity, reward_vo, SkillModRewardType::BountyHunter)?;
    reward::set_multiplier_reward(event_reward_entity, reward_vo);
    reward::set_amplifier_reward(w, persona_entity, reward_vo)?;
    Ok(())
}

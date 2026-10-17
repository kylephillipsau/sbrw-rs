//! `RewardTeamEscapeBO`: what team escapes pay.

use sbrw_xml::dto::http::{Accolades, EnumRewardType, TeamEscapeArbitrationPacket};

use crate::bo::achievement_transaction::AchievementTransaction;
use crate::bo::legit_race;
use crate::bo::reward::{self, SkillModRewardType};
use crate::dao::event_data::EventDataEntity;
use crate::dao::event_session::EventSessionEntity;
use crate::dao::persona::PersonaEntity;
use crate::{Fault, World};

// port: bo/RewardTeamEscapeBO#getAccolades(Long,TeamEscapeArbitrationPacket,EventDataEntity,EventSessionEntity,AchievementTransaction)
/// Nothing unless the escape is legit and finished (reason 22). Otherwise
/// the base and rank rewards; then the base divided by one more than the
/// times busted for the perfect start, pursuit statistics (no heat), top
/// speed, bounty hunter skill mods, multipliers and amplifiers. The packet's
/// rank is then drawn from 1 to 4 for the lucky draw.
pub fn get_accolades(
    w: &mut World,
    active_persona_id: i64,
    team_escape_arbitration_packet: &mut TeamEscapeArbitrationPacket,
    event_data_entity: &mut EventDataEntity,
    event_session_entity: &EventSessionEntity,
    achievement_transaction: &mut AchievementTransaction,
    persona_entity: &mut PersonaEntity,
) -> Result<Accolades, Fault> {
    let finish_reason = team_escape_arbitration_packet.finish_reason;
    let legit = legit_race::is_legit(
        w,
        active_persona_id,
        &*team_escape_arbitration_packet,
        event_session_entity,
        event_data_entity,
    )?;
    event_data_entity.is_legit = legit;
    if !legit || finish_reason != 22 {
        return Ok(Accolades::default());
    }

    let mut busted_count = team_escape_arbitration_packet.busted_count as f32;
    busted_count += 1.0;

    let mut reward_vo = reward::get_reward_vo(w, persona_entity)?;
    let event_reward_entity = reward::get_reward_configuration(w, event_session_entity)?;

    let p = &*team_escape_arbitration_packet;
    reward::set_base_reward(w, persona_entity, &event_data_entity.event, &event_reward_entity, p, &mut reward_vo)?;
    reward::set_rank_reward(&event_reward_entity, p, &mut reward_vo);

    let busted_base_rep = reward_vo.base_rep() as f32 / busted_count;
    let busted_base_cash = reward_vo.base_cash() as f32 / busted_count;

    reward_vo.set_base_rep(busted_base_rep as i32);
    reward_vo.set_base_cash(busted_base_cash as i32);

    reward::set_perfect_start_reward(&event_reward_entity, p.perfect_start, &mut reward_vo);
    let vo = &mut reward_vo;
    reward::set_pursit_param_reward(w, p.cops_deployed as f32, EnumRewardType::COP_CARS_DEPLOYED, vo)?;
    reward::set_pursit_param_reward(w, p.cops_disabled as f32, EnumRewardType::COP_CARS_DISABLED, vo)?;
    reward::set_pursit_param_reward(w, p.cops_rammed as f32, EnumRewardType::COP_CARS_RAMMED, vo)?;
    reward::set_pursit_param_reward(w, p.cost_to_state as f32, EnumRewardType::COST_TO_STATE, vo)?;
    reward::set_pursit_param_reward(w, p.event_duration_in_milliseconds as f32, EnumRewardType::PURSUIT_LENGTH, vo)?;
    reward::set_pursit_param_reward(w, p.infractions as f32, EnumRewardType::INFRACTIONS, vo)?;
    reward::set_pursit_param_reward(w, p.road_blocks_dodged as f32, EnumRewardType::ROADBLOCKS_DODGED, vo)?;
    reward::set_pursit_param_reward(w, p.spike_strips_dodged as f32, EnumRewardType::SPIKE_STRIPS_DODGED, vo)?;

    reward::set_top_speed_reward(&event_reward_entity, p.top_speed, &mut reward_vo);
    reward::set_skill_multiplier_reward(w, persona_entity, &mut reward_vo, SkillModRewardType::BountyHunter)?;
    reward::set_multiplier_reward(&event_reward_entity, &mut reward_vo);
    reward::set_amplifier_reward(w, persona_entity, &mut reward_vo)?;

    // `RandomUtils.nextInt(1, 5)`.
    team_escape_arbitration_packet.rank = 1 + w.env.random(|r| r.next_int_bounded(4));
    reward::apply_race_reward(
        w,
        reward_vo.rep(),
        reward_vo.cash(),
        persona_entity,
        true,
        Some(achievement_transaction),
    )?;
    reward::get_accolades(w, persona_entity, &event_reward_entity, &*team_escape_arbitration_packet, &reward_vo)
}

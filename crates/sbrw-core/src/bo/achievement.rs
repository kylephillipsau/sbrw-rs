//! `AchievementBO`: persona progress on achievements, and the XMPP push that
//! reports it.
//!
//! Progress comes from scripts in the static data (`update_trigger`,
//! `update_value`), which ScriptingBO runs.

use std::collections::HashMap;

use anyhow::anyhow;
use rusqlite::Connection;
use sbrw_java::lang::{round_double, round_float, trim};
use sbrw_xml::XmlValue;
use sbrw_xml::dto::http::{
    AchievementDefinitionPacket, AchievementRankPacket, AchievementRewards, AchievementState, AchievementsPacket,
    ArrayOfAchievementDefinitionPacket, ArrayOfAchievementRankPacket, ArrayOfBadgeDefinitionPacket, ArrayOfWalletTrans,
    BadgeDefinitionPacket, BadgePacket, CommerceResultStatus, StatConversion, WalletTrans,
};
use sbrw_xml::dto::xmpp::{
    AchievementAwarded, AchievementProgress, AchievementsAwarded, XMPP_ResponseTypeAchievementsAwarded,
};
use time::PrimitiveDateTime;

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::scripting::{self, ScriptValue};
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::bo::util::achievement_update_info::{
    AchievementUpdateInfo, CompletedAchievementRank, ProgressedAchievement,
};
use crate::bo::{driver_persona, item_reward};
use crate::dao::achievement::{self, AchievementEntity, AchievementRankEntity};
use crate::dao::badge_definition::{self, BadgeDefinitionEntity};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::persona_achievement::{self, PersonaAchievementEntity};
use crate::dao::persona_achievement_rank::{self, PersonaAchievementRankEntity};
use crate::dao::{achievement_rank, achievement_reward};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World, datetime};

/// The singleton's state: the static achievements and badges, as loaded.
#[derive(Debug, Default)]
pub struct Achievements {
    achievement_entities: Vec<AchievementEntity>,
    badge_definition_entities: Vec<BadgeDefinitionEntity>,
}

impl Achievements {
    /// `achievementCategoryMap.get(category)`: the category's achievements, in
    /// load order.
    fn in_category<'a>(&'a self, category: &'a str) -> impl Iterator<Item = &'a AchievementEntity> {
        self.achievement_entities.iter().filter(move |a| a.category.as_deref() == Some(category))
    }

    /// An achievement's LAZY badge definition. Static data, so the loaded copy
    /// is what Hibernate would fetch.
    fn badge_definition(&self, id: i64) -> Result<&BadgeDefinitionEntity, Fault> {
        self.badge_definition_entities
            .iter()
            .find(|b| b.id == id)
            .ok_or_else(|| anyhow!("no badge_definition row {id}").into())
    }
}

// port: bo/AchievementBO#loadData()
pub fn load_data(db: &Connection) -> rusqlite::Result<Achievements> {
    Ok(Achievements {
        achievement_entities: achievement::find_all(db)?,
        badge_definition_entities: badge_definition::find_all(db)?,
    })
}

// port: bo/AchievementBO#updateRankRarities()
/// The half-hourly job: each rank's rarity is the share of drivers who have
/// reached it. Only the database changes; the achievements loaded at startup
/// keep the rarities they were loaded with.
pub fn update_rank_rarities(w: &mut World) -> Result<(), Fault> {
    let count_personas = persona::count_personas(w.db())?;

    if count_personas > 0 {
        for achievement_entity in &w.achievements.achievement_entities {
            for achievement_rank_entity in &achievement_entity.ranks {
                let with_rank = persona_achievement_rank::count_personas_with_rank(w.db(), achievement_rank_entity.id)?;
                achievement_rank::update_rank_rarity(
                    w.db(),
                    achievement_rank_entity.id,
                    with_rank as f32 / count_personas as f32,
                )?;
            }
        }
    }
    Ok(())
}

// port: bo/AchievementBO#createTransaction(Long)
pub fn create_transaction(persona_id: i64) -> AchievementTransaction {
    AchievementTransaction::new(persona_id)
}

// port: bo/AchievementBO#commitTransaction(PersonaEntity,AchievementTransaction)
/// Apply the transaction's updates to the persona, save it, and push the
/// outcome (with the persona's badges) to its game client.
pub fn commit_transaction(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    transaction: &mut AchievementTransaction,
) -> Result<(), Fault> {
    let mut achievement_update_info_list = Vec::new();
    for (category, entries) in transaction.entries() {
        for properties in entries {
            let mut persona_achievements = get_persona_achievement_entity_map(w.db(), persona_entity.persona_id)?;
            achievement_update_info_list.extend(update_achievements(
                w,
                persona_entity,
                &mut persona_achievements,
                category,
                properties,
            )?);
        }
    }
    persona::update(w.db(), persona_entity)?;
    let badge_packet_list = driver_persona::get_badges(w, persona_entity.persona_id)?.badge_packet;

    send_update_message(w, persona_entity, &achievement_update_info_list, badge_packet_list)?;

    transaction.mark_committed()?;
    transaction.clear()?;
    Ok(())
}

/// `StatConversion.fromValue`: an exact match of a constant's value.
fn stat_conversion_from_value(v: Option<&str>) -> Result<StatConversion, Fault> {
    [
        StatConversion::NONE,
        StatConversion::FROM_MILLISECONDS_TO_MINUTES,
        StatConversion::FROM_SECONDS_TO_MINUTES,
        StatConversion::FROM_CENTIMETERS_PER_SECOND_TO_SPEED,
        StatConversion::FROM_METERS_TO_DISTANCE,
        StatConversion::FROM_NUMBER_TO_CURRENCY,
    ]
    .into_iter()
    .find(|c| Some(c.print().as_str()) == v)
    .ok_or_else(|| anyhow!("IllegalArgumentException: {}", v.unwrap_or("null")).into())
}

/// `AchievementState.fromValue`: an exact match of a constant's value.
fn achievement_state_from_value(v: Option<&str>) -> Result<AchievementState, Fault> {
    [
        AchievementState::LOCKED,
        AchievementState::IN_PROGRESS,
        AchievementState::REWARD_WAITING,
        AchievementState::COMPLETED,
    ]
    .into_iter()
    .find(|c| Some(c.print().as_str()) == v)
    .ok_or_else(|| anyhow!("IllegalArgumentException: {}", v.unwrap_or("null")).into())
}

// port: bo/AchievementBO#loadAll(Long)
/// Every achievement as loaded at startup, with the persona's progress and
/// each rank's state, and every badge.
pub fn load_all(w: &World, persona_id: i64) -> Result<AchievementsPacket, Fault> {
    if persona_id == 0 {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, true));
    }
    let null = |what| move || Fault::null_pointer(what);

    let mut definitions = ArrayOfAchievementDefinitionPacket::default();
    let persona_achievement_entity_map = get_persona_achievement_entity_map(w.db(), persona_id)?;
    for achievement_entity in &w.achievements.achievement_entities {
        let mut achievement_definition_packet = AchievementDefinitionPacket {
            achievement_definition_id: achievement_entity.id as i32,
            achievement_ranks: Some(ArrayOfAchievementRankPacket::default()),
            badge_definition_id: achievement_entity.badge_definition_id as i32,
            is_visible: achievement_entity.visible.ok_or_else(null("AchievementEntity.visible"))?,
            progress_text: achievement_entity.progress_text.clone(),
            stat_conversion: Some(stat_conversion_from_value(achievement_entity.stat_conversion.as_deref())?),
            can_progress: true,
            ..Default::default()
        };

        let persona_achievement_entity = persona_achievement_entity_map.get(&achievement_entity.id);

        if let Some(persona_achievement_entity) = persona_achievement_entity {
            achievement_definition_packet.can_progress = persona_achievement_entity.can_progress;
            achievement_definition_packet.current_value =
                persona_achievement_entity.current_value.ok_or_else(null("PersonaAchievementEntity.currentValue"))?;
        }

        for achievement_rank_entity in &achievement_entity.ranks {
            let rank = achievement_rank_entity.rank.ok_or_else(null("AchievementRankEntity.rank"))?;
            let mut rank_packet = AchievementRankPacket {
                achieved_on: Some("0001-01-01T00:00:00".into()),
                is_rare: achievement_rank_entity.is_rare().ok_or_else(null("AchievementRankEntity.rarity"))?,
                rarity: achievement_rank_entity.rarity.ok_or_else(null("AchievementRankEntity.rarity"))?,
                rank: rank as i16,
                reward_description: achievement_rank_entity.reward_description.clone(),
                reward_type: achievement_rank_entity.reward_type.clone(),
                reward_visual_style: achievement_rank_entity.reward_visual_style.clone(),
                points: achievement_rank_entity.points.ok_or_else(null("AchievementRankEntity.points"))? as i16,
                achievement_rank_id: achievement_rank_entity.id as i32,
                // NOTE 17.05.2020: this is apparently necessary, game ignores updates otherwise
                state: Some(if rank == 1 { AchievementState::IN_PROGRESS } else { AchievementState::LOCKED }),
                threshold_value: i64::from(
                    achievement_rank_entity.threshold_value.ok_or_else(null("AchievementRankEntity.thresholdValue"))?,
                ),
            };

            if let Some(persona_achievement_entity) = persona_achievement_entity
                && persona_achievement_entity.ranks.len() as i64 >= i64::from(rank)
            {
                let persona_achievement_rank_entity = usize::try_from(rank - 1)
                    .ok()
                    .and_then(|i| persona_achievement_entity.ranks.get(i))
                    .ok_or_else(|| anyhow!("Index {} out of bounds", rank - 1))?;
                rank_packet.state =
                    Some(achievement_state_from_value(persona_achievement_rank_entity.state.as_deref())?);

                if let Some(achieved_on) = persona_achievement_rank_entity.achieved_on {
                    rank_packet.achieved_on = Some(rank_completed_at(achieved_on));
                }
            }

            if let Some(ranks) = &mut achievement_definition_packet.achievement_ranks {
                ranks.achievement_rank_packet.push(rank_packet);
            }
        }

        definitions.achievement_definition_packet.push(achievement_definition_packet);
    }

    let mut badges = ArrayOfBadgeDefinitionPacket::default();
    for badge_definition_entity in &w.achievements.badge_definition_entities {
        badges.badge_definition_packet.push(BadgeDefinitionPacket {
            background: badge_definition_entity.background.clone(),
            border: badge_definition_entity.border.clone(),
            description: badge_definition_entity.description.clone(),
            icon: badge_definition_entity.icon.clone(),
            name: badge_definition_entity.name.clone(),
            badge_definition_id: badge_definition_entity.id as i32,
        });
    }

    Ok(AchievementsPacket { persona_id, badges: Some(badges), definitions: Some(definitions) })
}

// port: bo/AchievementBO#redeemReward(Long,Long)
/// Give the persona a rank's reward, which must be waiting, by its reward
/// script; the rank is then Completed.
pub fn redeem_reward(
    w: &mut World,
    persona_id: i64,
    achievement_rank_id: Option<i64>,
) -> Result<AchievementRewards, Fault> {
    let mut persona_entity = persona::find(w.db(), persona_id)?;
    let achievement_rank_id =
        achievement_rank_id.ok_or_else(|| anyhow!("IllegalArgumentException: {persona_id} does not have null"))?;
    let Some((mut persona_achievement_rank_entity, achievement_rank_entity)) =
        persona_achievement_rank::find_by_persona_id_and_achievement_rank_id(w.db(), persona_id, achievement_rank_id)?
    else {
        return Err(anyhow!("IllegalArgumentException: {persona_id} does not have {achievement_rank_id}").into());
    };

    if persona_achievement_rank_entity.state.as_deref() != Some("RewardWaiting") {
        return Err(anyhow!("IllegalArgumentException: {persona_id} has no reward for {achievement_rank_id}").into());
    }

    let mut achievement_rewards = AchievementRewards {
        achievement_rank_id: Some(achievement_rank_id),
        visual_style: achievement_rank_entity.reward_visual_style.clone(),
        status: Some(CommerceResultStatus::SUCCESS),
        ..Default::default()
    };
    let reward_script = w
        .ejb(|w| {
            achievement_reward::find_by_description(w.db(), achievement_rank_entity.reward_description.as_deref())
        })?
        .reward_script;
    let persona_entity_ref = persona_entity.as_mut().ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let rewards = item_reward::get_rewards(w, persona_entity_ref, Some(&reward_script))?;
    item_reward::convert_rewards(w, &rewards, &mut achievement_rewards)?;
    achievement_rewards.wallets = Some(ArrayOfWalletTrans {
        wallet_trans: vec![WalletTrans { balance: persona_entity_ref.cash, currency: Some("CASH".into()) }],
    });

    persona_achievement_rank_entity.state = Some("Completed".into());
    persona_achievement_rank::update(w.db(), &persona_achievement_rank_entity)?;

    Ok(achievement_rewards)
}

// port: bo/AchievementBO#sendUpdateMessage(PersonaEntity,List,List)
fn send_update_message(
    w: &World,
    persona_entity: &PersonaEntity,
    achievement_update_info_list: &[AchievementUpdateInfo],
    badge_packet_list: Vec<BadgePacket>,
) -> Result<(), Fault> {
    let mut achievements = Vec::new();
    let mut progressed = Vec::new();
    let null = |what| move || Fault::null_pointer(what);

    for info in achievement_update_info_list {
        let achievement_entity = &info.achievement_entity;
        for completed in &info.completed_achievement_ranks {
            let rank = &completed.achievement_rank_entity;
            let badge = w.achievements.badge_definition(achievement_entity.badge_definition_id)?;
            achievements.push(AchievementAwarded {
                achievement_definition_id: Some(achievement_entity.id),
                achieved_on: Some(rank_completed_at(completed.achieved_on.ok_or_else(null("achievedOn"))?)),
                achievement_rank_id: Some(rank.id),
                description: badge.description.clone(),
                icon: badge.icon.clone(),
                is_rare: rank.is_rare().ok_or_else(null("AchievementRankEntity.rarity"))?,
                name: badge.name.clone(),
                points: rank.points.ok_or_else(null("AchievementRankEntity.points"))?,
                rarity: rank.rarity.ok_or_else(null("AchievementRankEntity.rarity"))?,
                ..Default::default()
            });
        }
        if let Some(progressed_achievement) = &info.progressed_achievement {
            progressed.push(AchievementProgress {
                achievement_definition_id: progressed_achievement.achievement_definition_id,
                current_value: progressed_achievement.value.ok_or_else(null("ProgressedAchievement.value"))?,
            });
        }
    }

    let response = XMPP_ResponseTypeAchievementsAwarded {
        achievements_awarded: Some(AchievementsAwarded {
            score: persona_entity.score,
            persona_id: persona_entity.persona_id,
            progressed: Some(progressed),
            achievements: Some(achievements),
            badges: Some(badge_packet_list),
        }),
        ..Default::default()
    };
    w.openfire.send(&response, persona_entity.persona_id, &w.effects);
    Ok(())
}

/// `RANK_COMPLETED_AT_FORMATTER`: `yyyy-MM-dd'T'hh:mm:ss`, whose `hh` is the
/// 12-hour clock (1 to 12), as upstream has it.
fn rank_completed_at(t: PrimitiveDateTime) -> String {
    let hour12 = (t.hour() + 11) % 12 + 1;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        t.year(),
        u8::from(t.month()),
        t.day(),
        hour12,
        t.minute(),
        t.second()
    )
}

// port: bo/AchievementBO#getPersonaAchievementEntityMap(Long)
/// The persona's achievement progress, by achievement id.
fn get_persona_achievement_entity_map(
    db: &Connection,
    persona_id: i64,
) -> rusqlite::Result<HashMap<i64, PersonaAchievementEntity>> {
    Ok(persona_achievement::find_all_by_persona_id(db, persona_id)?
        .into_iter()
        .map(|p| (p.achievement_id, p))
        .collect())
}

// port: bo/AchievementBO#updateAchievements(PersonaEntity,Map,String,Map)
/// Update the persona's progress on each achievement of the category whose
/// trigger says so, then, if that scored points, on PROGRESSION.
fn update_achievements(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    persona_achievements: &mut HashMap<i64, PersonaAchievementEntity>,
    achievement_category: &str,
    properties: &Bindings,
) -> Result<Vec<AchievementUpdateInfo>, Fault> {
    let original_score = persona_entity.score;
    let mut new_score = original_score;
    let mut properties = properties.clone();
    let mut achievement_update_info_list = Vec::new();

    let category: Vec<AchievementEntity> = w.achievements.in_category(achievement_category).cloned().collect();
    for achievement_entity in &category {
        if !achievement_entity.auto_update.ok_or_else(|| Fault::null_pointer("AchievementEntity.autoUpdate"))? {
            continue;
        }
        let Some(update_trigger) = achievement_entity.update_trigger.as_deref().filter(|t| !trim(t).is_empty()) else {
            continue;
        };

        // The persona's progress on it, created if there's none yet.
        let insert = !persona_achievements.contains_key(&achievement_entity.id);
        let persona_achievement =
            persona_achievements.entry(achievement_entity.id).or_insert_with(|| PersonaAchievementEntity {
                can_progress: true,
                current_value: Some(0),
                achievement_id: achievement_entity.id,
                persona_id: persona_entity.persona_id,
                ..Default::default()
            });
        properties.insert("personaAchievement".into(), Binding::PersonaAchievement(persona_achievement.clone()));

        match scripting::eval(update_trigger, &properties)? {
            // Upstream prints the stack trace and moves on.
            Err(ex) => tracing::warn!(achievement = achievement_entity.id, "update trigger failed: {}", ex.0),
            Ok(value) => {
                let should_update = match value {
                    ScriptValue::Boolean(b) => b,
                    ScriptValue::Null => return Err(Fault::null_pointer("the update trigger's result")),
                    other => return Err(anyhow!("ClassCastException: {other:?} is not a Boolean").into()),
                };
                if should_update {
                    if insert {
                        persona_achievement::insert(w.db(), persona_achievement)?;
                    }
                    let info = update_achievement(w, achievement_entity, &properties, persona_achievement)?;
                    new_score = new_score.wrapping_add(info.points_given);
                    achievement_update_info_list.push(info);
                }
            }
        }
    }

    persona_entity.score = new_score;

    if new_score != original_score {
        let progression_context = AchievementProgressionContext::new(
            Some(0),
            Some(0),
            Some(persona_entity.level),
            Some(persona_entity.score),
            Some(0),
            false,
            true,
            false,
            false,
        );
        let properties = Bindings::from([
            ("persona".into(), Binding::Persona(persona_entity.clone())),
            ("progression".into(), Binding::Progression(progression_context)),
        ]);
        achievement_update_info_list.extend(update_achievements(
            w,
            persona_entity,
            persona_achievements,
            "PROGRESSION",
            &properties,
        )?);
    }

    Ok(achievement_update_info_list)
}

// port: bo/AchievementBO#updateAchievement(AchievementEntity,Map,PersonaAchievementEntity)
/// Add the achievement's `update_value` (or set it, if it overwrites) to the
/// persona's progress, up to the highest rank's threshold, and move the ranks
/// it reaches to RewardWaiting. The persona's rank rows are created, Locked,
/// as the walk reaches them; changes to them are written as Hibernate would
/// flush them.
fn update_achievement(
    w: &mut World,
    achievement_entity: &AchievementEntity,
    bindings: &Bindings,
    persona_achievement_entity: &mut PersonaAchievementEntity,
) -> Result<AchievementUpdateInfo, Fault> {
    if !persona_achievement_entity.can_progress {
        return Ok(AchievementUpdateInfo::new(achievement_entity.clone()));
    }
    let null = |what| move || Fault::null_pointer(what);
    let mut points_added: i32 = 0;

    let update_value = match achievement_entity.update_value.as_deref() {
        Some(v) if !trim(v).is_empty() => v,
        _ => return Ok(AchievementUpdateInfo::new(achievement_entity.clone())),
    };
    let mut achievement_update_info = AchievementUpdateInfo::new(achievement_entity.clone());
    let raw_val = scripting::eval(update_value, bindings)?
        .map_err(|_| Fault::engine(EngineExceptionCode::UnspecifiedError, true))?;
    let clean_val: i64 = match raw_val {
        ScriptValue::Integer(i) => i64::from(i),
        ScriptValue::Long(l) => l,
        ScriptValue::Float(f) if f.is_nan() => {
            return Err(anyhow!("Float return value is NaN! Script: {update_value}").into());
        }
        ScriptValue::Float(f) => i64::from(round_float(f)),
        ScriptValue::Double(d) if d.is_nan() => {
            return Err(anyhow!("Double return value is NaN! Script: {update_value}").into());
        }
        ScriptValue::Double(d) => round_double(d),
        _ => 0,
    };

    let thresholds = achievement_entity
        .ranks
        .iter()
        .map(|r| r.threshold_value.ok_or_else(null("AchievementRankEntity.thresholdValue")))
        .collect::<Result<Vec<i32>, Fault>>()?;
    if let Some(&max_val) = thresholds.iter().max() {
        // The value it had when the walk began; it changes after.
        let current_value = persona_achievement_entity.current_value;
        let current = || current_value.ok_or_else(null("PersonaAchievementEntity.currentValue"));
        let should_overwrite = achievement_entity
            .should_overwrite_progress
            .ok_or_else(null("AchievementEntity.shouldOverwriteProgress"))?;
        let progress = if should_overwrite { clean_val } else { current()?.wrapping_add(clean_val) };
        let new_val = 0.max(i64::from(max_val).min(progress));
        if new_val == 0 {
            achievement_update_info.progressed_achievement = Some(ProgressedAchievement {
                achievement_definition_id: Some(achievement_entity.id),
                value: Some(new_val),
            });
        } else {
            let ranks = &achievement_entity.ranks;
            for (i, current_rank_def) in ranks.iter().enumerate() {
                let mut previous = None;
                let mut previous_rank = None;
                let current_rank = find_persona_achievement_rank(persona_achievement_entity, current_rank_def)?;

                if i > 0 {
                    let previous_def = &ranks[i - 1];
                    previous = Some(previous_def);
                    previous_rank =
                        Some(match find_persona_achievement_rank(persona_achievement_entity, previous_def)? {
                            Some(index) => index,
                            None => create_persona_achievement_rank(w, persona_achievement_entity, previous_def)?,
                        });
                }

                let current_rank = match current_rank {
                    Some(index) => index,
                    None => create_persona_achievement_rank(w, persona_achievement_entity, current_rank_def)?,
                };

                let threshold = i64::from(current_rank_def.threshold_value.ok_or_else(null("thresholdValue"))?);
                let state = |index: usize, pa: &PersonaAchievementEntity| {
                    pa.ranks[index].state.clone().ok_or_else(null("PersonaAchievementRankEntity.state"))
                };

                let current_state = state(current_rank, persona_achievement_entity)?;
                if current_state == "Completed" || current_state == "RewardWaiting" {
                    continue;
                }

                if new_val >= threshold && new_val != current()? {
                    let rank = &mut persona_achievement_entity.ranks[current_rank];
                    rank.state = Some("RewardWaiting".into());
                    rank.achieved_on = Some(datetime::now(&w.env));
                    persona_achievement_rank::update(w.db(), rank)?;
                    points_added = points_added
                        .wrapping_add(current_rank_def.points.ok_or_else(null("AchievementRankEntity.points"))?);
                    achievement_update_info.completed_achievement_ranks.push(CompletedAchievementRank {
                        achievement_entity: achievement_entity.clone(),
                        achievement_rank_entity: current_rank_def.clone(),
                        achieved_on: rank.achieved_on,
                    });
                } else if let (Some(_), Some(previous_rank)) = (previous, previous_rank)
                    && state(previous_rank, persona_achievement_entity)? == "InProgress"
                {
                    let rank = &mut persona_achievement_entity.ranks[current_rank];
                    rank.state = Some("Locked".into());
                    persona_achievement_rank::update(w.db(), rank)?;
                    break;
                } else if new_val > 0 && new_val != current()? && new_val < threshold {
                    let rank = &mut persona_achievement_entity.ranks[current_rank];
                    rank.state = Some("InProgress".into());
                    persona_achievement_rank::update(w.db(), rank)?;
                    achievement_update_info.progressed_achievement = Some(ProgressedAchievement {
                        achievement_definition_id: Some(achievement_entity.id),
                        value: Some(new_val),
                    });
                    break;
                }
            }
        }

        persona_achievement_entity.current_value = Some(new_val);
        persona_achievement_entity.can_progress = new_val < i64::from(max_val);
        persona_achievement::update(w.db(), persona_achievement_entity)?;
    }

    achievement_update_info.points_given = points_added;
    Ok(achievement_update_info)
}

// port: bo/AchievementBO#findPersonaAchievementRank(PersonaAchievementEntity,AchievementRankEntity)
/// The persona's row for the rank: by the rank's number, as a position in the
/// rows created so far.
fn find_persona_achievement_rank(
    persona_achievement_entity: &PersonaAchievementEntity,
    achievement_rank_entity: &AchievementRankEntity,
) -> Result<Option<usize>, Fault> {
    let rank_num = achievement_rank_entity.rank.ok_or_else(|| Fault::null_pointer("AchievementRankEntity.rank"))?;
    let len = persona_achievement_entity.ranks.len();
    if len as i64 >= i64::from(rank_num) {
        return usize::try_from(rank_num - 1)
            .map(Some)
            .map_err(|_| anyhow!("Index {} out of bounds for length {len}", rank_num - 1).into());
    }
    Ok(None)
}

// port: bo/AchievementBO#createPersonaAchievementRank(PersonaAchievementEntity,AchievementRankEntity)
/// A Locked row for the rank, saved and added to the persona's rows; its position.
fn create_persona_achievement_rank(
    w: &World,
    persona_achievement_entity: &mut PersonaAchievementEntity,
    achievement_rank_entity: &AchievementRankEntity,
) -> Result<usize, Fault> {
    let mut rank_entity = PersonaAchievementRankEntity {
        state: Some("Locked".into()),
        persona_achievement_id: persona_achievement_entity.id,
        achievement_rank_id: achievement_rank_entity.id,
        ..Default::default()
    };
    persona_achievement_rank::insert(w.db(), &mut rank_entity)?;
    persona_achievement_entity.ranks.push(rank_entity);
    Ok(persona_achievement_entity.ranks.len() - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bo::driver_persona::create_persona;
    use crate::dao::user::{self, UserEntity};
    use time::macros::datetime;

    fn world_with_persona() -> (World, PersonaEntity) {
        let mut w = World::test(&[]);
        let mut u = UserEntity::new();
        user::insert(w.db(), &mut u).unwrap();
        let chosen = PersonaEntity { name: Some("DRIVER1".into()), ..Default::default() };
        let id = create_persona(&mut w, u.id, chosen).unwrap().persona_id;
        w.effects.drain();
        let p = persona::find(w.db(), id).unwrap().unwrap();
        (w, p)
    }

    fn persona_binding(p: &PersonaEntity) -> Bindings {
        Bindings::from([("persona".into(), Binding::Persona(p.clone()))])
    }

    #[test]
    fn loads_upstreams_achievements() {
        let (w, _) = world_with_persona();
        assert_eq!(w.achievements.achievement_entities.len(), 67);
        assert_eq!(w.achievements.badge_definition_entities.len(), 98);
        assert_eq!(w.achievements.in_category("LOGIN").count(), 0);
        assert_eq!(w.achievements.in_category("PROGRESSION").count(), 5);
        assert!(w.achievements.achievement_entities.iter().all(|a| !a.ranks.is_empty()));
    }

    #[test]
    fn a_login_transaction_saves_the_persona_and_pushes_no_progress() {
        let (mut w, mut p) = world_with_persona();
        p.last_login = Some(datetime!(2026-01-02 08:00));
        let mut transaction = create_transaction(p.persona_id);
        transaction.add("LOGIN", persona_binding(&p)).unwrap();
        commit_transaction(&mut w, &mut p, &mut transaction).unwrap();
        assert!(transaction.is_committed());
        assert!(transaction.entries().is_empty());
        assert_eq!(persona::find(w.db(), p.persona_id).unwrap().unwrap().last_login, p.last_login);
        let pushes = w.effects.drain().xmpp;
        assert_eq!(pushes.len(), 1);
        assert_eq!(pushes[0].to, p.persona_id);
        assert_eq!(
            pushes[0].body,
            "<response status=\"1\" ticket=\"0\"><AchievementsAwarded><Achievements/><Badges/>\
             <PersonaId>100</PersonaId><Progressed/><Score>0</Score></AchievementsAwarded></response>"
        );
    }

    #[test]
    fn triggers_that_throw_are_skipped() {
        // PROGRESSION's triggers read `progression`, which this transaction
        // doesn't bind: each throws, which upstream logs and moves past.
        let (mut w, mut p) = world_with_persona();
        let mut transaction = create_transaction(p.persona_id);
        transaction.add("PROGRESSION", persona_binding(&p)).unwrap();
        w.transaction(|w| commit_transaction(w, &mut p, &mut transaction)).unwrap();
        assert!(persona_achievement::find_all_by_persona_id(w.db(), p.persona_id).unwrap().is_empty());
        let pushes = w.effects.drain().xmpp;
        assert_eq!(pushes.len(), 1);
        assert!(pushes[0].body.contains("<Progressed/>"), "{}", pushes[0].body);
    }

    #[test]
    fn ranks_completed_at_use_a_12_hour_clock() {
        assert_eq!(rank_completed_at(datetime!(2026-03-04 00:05:06)), "2026-03-04T12:05:06");
        assert_eq!(rank_completed_at(datetime!(2026-03-04 13:00:00)), "2026-03-04T01:00:00");
        assert_eq!(rank_completed_at(datetime!(2026-03-04 12:59:59)), "2026-03-04T12:59:59");
    }
}

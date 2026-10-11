//! `AchievementBO`: persona progress on achievements, and the XMPP push that
//! reports it.
//!
//! Progress comes from scripts in the static data (`update_trigger`,
//! `update_value`), which ScriptingBO runs. That isn't ported yet, so only a
//! category none of whose achievements has a trigger can be committed; LOGIN,
//! the one ported callers use, has no achievements at all in upstream's data.

use std::collections::HashMap;

use anyhow::anyhow;
use rusqlite::Connection;
use sbrw_java::lang::trim;
use sbrw_xml::dto::http::BadgePacket;
use sbrw_xml::dto::xmpp::{
    AchievementAwarded, AchievementProgress, AchievementsAwarded, XMPP_ResponseTypeAchievementsAwarded,
};
use time::PrimitiveDateTime;

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::driver_persona;
use crate::bo::scripting::{self, ScriptValue};
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::bo::util::achievement_update_info::AchievementUpdateInfo;
use crate::dao::achievement::{self, AchievementEntity};
use crate::dao::badge_definition::{self, BadgeDefinitionEntity};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::persona_achievement::{self, PersonaAchievementEntity};
use crate::{Fault, World};

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

/// `updateAchievement(AchievementEntity,Map,PersonaAchievementEntity)`: not
/// ported yet. It evaluates `update_value` with ScriptingBO, so it lands with it.
fn update_achievement(
    _w: &mut World,
    _achievement_entity: &AchievementEntity,
    _bindings: &Bindings,
    _persona_achievement: &mut PersonaAchievementEntity,
) -> Result<AchievementUpdateInfo, Fault> {
    Err(Fault::unported("bo/AchievementBO#updateAchievement(AchievementEntity,Map,PersonaAchievementEntity)"))
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
    fn a_category_with_scripted_achievements_needs_the_script_engine() {
        let (mut w, mut p) = world_with_persona();
        let mut transaction = create_transaction(p.persona_id);
        transaction.add("PROGRESSION", persona_binding(&p)).unwrap();
        let err = commit_transaction(&mut w, &mut p, &mut transaction).unwrap_err();
        assert!(err.to_string().contains("bo/ScriptingBO#eval"), "{err}");
        assert!(!err.commits());
    }

    #[test]
    fn ranks_completed_at_use_a_12_hour_clock() {
        assert_eq!(rank_completed_at(datetime!(2026-03-04 00:05:06)), "2026-03-04T12:05:06");
        assert_eq!(rank_completed_at(datetime!(2026-03-04 13:00:00)), "2026-03-04T01:00:00");
        assert_eq!(rank_completed_at(datetime!(2026-03-04 12:59:59)), "2026-03-04T12:59:59");
    }
}

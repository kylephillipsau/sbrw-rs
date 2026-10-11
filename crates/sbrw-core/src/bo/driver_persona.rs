//! `DriverPersonaBO`: drivers (personas). Ported so far: creating them, name
//! checks, and the profile and level tables the game reads.

use sbrw_xml::dto::http::{ArrayOfBadgePacket, ArrayOfInt, ArrayOfString, BadgePacket, ProfileData};

use crate::bo::inventory;
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::treasure_hunt::{self, TreasureHuntEntity};
use crate::dao::{level_rep, persona_achievement_rank, persona_badge, user};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World, datetime};

// port: bo/DriverPersonaBO#createPersona(Long,PersonaEntity)
/// Save a new persona for the user, with an inventory and treasure hunt state.
/// `persona` carries the name and icon the player chose.
pub fn create_persona(w: &mut World, user_id: i64, mut persona: PersonaEntity) -> Result<ProfileData, Fault> {
    let user_entity = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("UserEntity"))?;
    if user_entity.personas.len() >= 3 {
        return Err(Fault::engine(EngineExceptionCode::MaximumNumberOfPersonasForUserReached, false));
    }
    if persona.icon_index < 0 || persona.icon_index > w.params.get_int_param_or("MAX_ICON_INDEX", 26)? {
        return Err(Fault::engine(EngineExceptionCode::InvalidOperation, false));
    }

    persona.user_id = Some(user_entity.id);
    persona.cash = f64::from(w.params.get_int_param("STARTING_CASH_AMOUNT")?);
    persona.level = w.params.get_int_param("STARTING_LEVEL_NUMBER")?;
    persona.created = Some(datetime::now(&w.env));
    persona.first_login = persona.created;
    persona.last_login = persona.created;
    persona::insert(w.db(), &mut persona)?;

    inventory::create_inventory(w, &persona)?;
    create_th_information(w, &persona)?;

    Ok(cast_persona_entity(&persona))
}

// port: bo/DriverPersonaBO#castPersonaEntity(PersonaEntity)
fn cast_persona_entity(p: &PersonaEntity) -> ProfileData {
    ProfileData {
        name: p.name.clone(),
        cash: p.cash,
        boost: p.boost,
        icon_index: p.icon_index,
        persona_id: p.persona_id,
        level: p.level,
        ..Default::default()
    }
}

// port: bo/DriverPersonaBO#getExpLevelPointsMap()
/// The total rep needed to reach each level.
pub fn get_exp_level_points_map(w: &World) -> Result<ArrayOfInt, Fault> {
    let mut array = ArrayOfInt::default();
    let mut rep = 0i32;
    for level_rep in level_rep::find_all(w.db())? {
        let exp_point = level_rep.exp_point.ok_or_else(|| Fault::null_pointer("LevelRepEntity.expPoint"))?;
        // Long.intValue() truncates, and the int sum wraps.
        rep = rep.wrapping_add(exp_point as i32);
        array._int.push(rep);
    }
    Ok(array)
}

// port: bo/DriverPersonaBO#getPersonaInfo(Long)
pub fn get_persona_info(w: &World, persona_id: i64) -> Result<ProfileData, Fault> {
    let p = persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut profile_data = cast_persona_entity(&p);
    profile_data.badges = Some(get_badges(w, persona_id)?);
    profile_data.motto = p.motto;
    profile_data.percent_to_level = p.percent_to_level;
    profile_data.rating = p.rating;
    profile_data.rep = p.rep;
    profile_data.rep_at_current_level = p.rep_at_current_level;
    profile_data.score = p.score;
    Ok(profile_data)
}

// port: bo/DriverPersonaBO#getBadges(Long)
/// The badges a persona shows, each with the highest rank it has completed of
/// the badge's achievement; badges with none are left out.
pub fn get_badges(w: &World, persona_id: i64) -> Result<ArrayOfBadgePacket, Fault> {
    let mut array = ArrayOfBadgePacket::default();
    for badge in persona_badge::find_all_badges_for_persona(w.db(), persona_id)? {
        let achievement_id = badge
            .badge_definition
            .achievement_id
            .ok_or_else(|| Fault::null_pointer("BadgeDefinitionEntity.achievementEntity"))?;
        let Some((_, rank)) = persona_achievement_rank::find_highest_completed_rank_of_achievement_by_persona(
            w.db(),
            persona_id,
            achievement_id,
        )?
        else {
            continue;
        };
        let null = |what| move || Fault::null_pointer(what);
        array.badge_packet.push(BadgePacket {
            achievement_rank_id: rank.id as i32,
            badge_definition_id: badge.badge_definition.id as i32,
            is_rare: rank.is_rare().ok_or_else(null("AchievementRankEntity.rarity"))?,
            rarity: rank.rarity.ok_or_else(null("AchievementRankEntity.rarity"))?,
            slot_id: badge.slot.ok_or_else(null("PersonaBadgeEntity.slot"))? as i16,
        });
    }
    Ok(array)
}

// port: bo/DriverPersonaBO#reserveName(String)
/// `NONE` when the name is taken; empty when it's free.
pub fn reserve_name(w: &World, name: Option<&str>) -> Result<ArrayOfString, Fault> {
    let mut array = ArrayOfString::default();
    if persona::find_by_name(w.db(), name)?.is_some() {
        array.string.push("NONE".into());
    }
    Ok(array)
}

// port: bo/DriverPersonaBO#createThInformation(PersonaEntity)
/// Today's treasure hunt for a new persona, with a random seed.
pub fn create_th_information(w: &mut World, persona: &PersonaEntity) -> Result<(), Fault> {
    let treasure_hunt = TreasureHuntEntity {
        coins_collected: Some(0),
        is_streak_broken: Some(false),
        num_coins: Some(w.params.get_int_param_or("TREASURE_HUNT_COINS", 15)?),
        persona_id: persona.persona_id,
        seed: Some(w.env.random(|r| r.next_int())),
        streak: Some(1),
        th_date: Some(datetime::today(&w.env)),
        ..Default::default()
    };
    treasure_hunt::insert(w.db(), &treasure_hunt)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::user::UserEntity;
    use sbrw_xml::ToXml;

    fn world_with_user(parameters: &[(&str, &str)]) -> (World, i64) {
        let w = World::test(parameters);
        let mut u = UserEntity::new();
        user::insert(w.db(), &mut u).unwrap();
        (w, u.id)
    }

    fn chosen(name: &str, icon_index: i32) -> PersonaEntity {
        PersonaEntity { name: Some(name.into()), icon_index, ..Default::default() }
    }

    #[test]
    fn creates_a_persona_with_starting_values() {
        let (mut w, user_id) = world_with_user(&[]);
        let profile = create_persona(&mut w, user_id, chosen("DRIVER1", 5)).unwrap();
        assert_eq!(
            profile.to_xml(),
            "<ProfileData><Boost>0.0</Boost><Cash>350000.0</Cash><IconIndex>5</IconIndex><Level>60</Level>\
             <Name>DRIVER1</Name><PercentToLevel>0.0</PercentToLevel><PersonaId>100</PersonaId><Rating>0.0</Rating>\
             <Rep>0.0</Rep><RepAtCurrentLevel>0</RepAtCurrentLevel><Score>0</Score></ProfileData>"
        );
        let stored = persona::find(w.db(), 100).unwrap().unwrap();
        assert_eq!(stored.user_id, Some(user_id));
        assert_eq!(stored.created, stored.first_login);
        assert_eq!(stored.created, stored.last_login);
        // The seed is the shared Random's first draw: new Random(1).nextInt().
        let (seed, date): (i32, String) = w
            .db()
            .query_row("SELECT seed, thDate FROM treasure_hunt WHERE personaId = 100", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((seed, date.as_str()), (-1155869325, "2026-01-01"));
        let items: i64 = w.db().query_row("SELECT COUNT(*) FROM inventory_item", [], |r| r.get(0)).unwrap();
        assert_eq!(items, 11);
    }

    #[test]
    fn three_personas_at_most_and_icons_in_range() {
        let (mut w, user_id) = world_with_user(&[("MAX_ICON_INDEX", "3")]);
        let Err(Fault::Engine(e)) = create_persona(&mut w, user_id, chosen("A1", 4)) else { panic!() };
        assert_eq!((e.code, e.fatal), (EngineExceptionCode::InvalidOperation, false));
        let Err(Fault::Engine(e)) = create_persona(&mut w, user_id, chosen("A1", -1)) else { panic!() };
        assert_eq!(e.code, EngineExceptionCode::InvalidOperation);
        for name in ["A1", "A2", "A3"] {
            create_persona(&mut w, user_id, chosen(name, 3)).unwrap();
        }
        let Err(Fault::Engine(e)) = create_persona(&mut w, user_id, chosen("A4", 0)) else { panic!() };
        assert_eq!((e.code, e.fatal), (EngineExceptionCode::MaximumNumberOfPersonasForUserReached, false));
    }

    #[test]
    fn names_are_reserved_case_insensitively() {
        let (mut w, user_id) = world_with_user(&[]);
        assert!(reserve_name(&w, Some("DRIVER1")).unwrap().string.is_empty());
        create_persona(&mut w, user_id, chosen("DRIVER1", 0)).unwrap();
        assert_eq!(reserve_name(&w, Some("driver1")).unwrap().string, ["NONE"]);
        assert!(reserve_name(&w, None).unwrap().string.is_empty());
    }

    #[test]
    fn exp_level_points_accumulate() {
        let (w, _) = world_with_user(&[]);
        let map = get_exp_level_points_map(&w).unwrap();
        assert_eq!(map._int.len(), 60);
        assert_eq!(map._int[..3], [150, 668, 1800]);
    }

    #[test]
    fn persona_info_includes_badges_and_profile() {
        let (mut w, user_id) = world_with_user(&[]);
        create_persona(&mut w, user_id, chosen("DRIVER1", 5)).unwrap();
        w.db().execute("UPDATE persona SET motto = 'hi', score = 7 WHERE ID = 100", []).unwrap();
        // A badge in slot 2 for achievement 1, whose rank 2 is completed.
        let (achievement, badge, rank): (i64, i64, i64) = w
            .db()
            .query_row(
                "SELECT a.ID, a.badge_definition_id, (SELECT ID FROM achievement_rank WHERE achievement_id = a.ID ORDER BY rank LIMIT 1 OFFSET 1)
                 FROM achievement a WHERE a.ID = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        w.db()
            .execute_batch(&format!(
                "INSERT INTO persona_badge (slot, badge_definition_id, persona_id) VALUES (2, {badge}, 100);
                 INSERT INTO persona_achievement (can_progress, current_value, achievement_id, persona_id) VALUES (1, 5, {achievement}, 100);
                 INSERT INTO persona_achievement_rank (state, achievement_rank_id, persona_achievement_id) VALUES ('Completed', {rank}, 1);"
            ))
            .unwrap();
        let info = get_persona_info(&w, 100).unwrap();
        assert_eq!((info.motto.as_deref(), info.score), (Some("hi"), 7));
        let badges = info.badges.unwrap().badge_packet;
        assert_eq!(badges.len(), 1);
        assert_eq!((badges[0].achievement_rank_id as i64, badges[0].badge_definition_id as i64), (rank, badge));
        assert_eq!(badges[0].slot_id, 2);
        assert!(get_persona_info(&w, 999).is_err());
    }
}

//! `EventsBO`: the daily treasure hunt. Each day's hunt has a seed the game
//! scatters its coins by; collecting every one (a bit each, 32767 for all 15)
//! pays by the streak of days in a row.

use sbrw_xml::dto::http::{Accolades, EnumRewardCategory, EnumRewardType, TreasureHuntEventSession};

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::reward::{self, SkillModRewardType};
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::bo::{achievement, driver_persona};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::treasure_hunt::{self, TreasureHuntEntity};
use crate::dao::treasure_hunt_config;
use crate::engine::{EngineException, EngineExceptionCode};
use crate::{Fault, World, datetime};

/// Every coin collected.
const ALL_COINS: i32 = 32767;

fn null(what: &'static str) -> impl Fn() -> Fault {
    move || Fault::null_pointer(what)
}

fn not_ready() -> Fault {
    Fault::Engine(EngineException::with_message(
        "TH is not ready",
        EngineExceptionCode::SecurityKickedArbitration,
        true,
    ))
}

// port: bo/EventsBO#getTreasureHuntEventSession(Long)
/// Today's hunt: the persona's, created if they have none, or a new one when
/// theirs is from another day.
pub fn get_treasure_hunt_event_session(
    w: &mut World,
    active_persona_id: i64,
) -> Result<TreasureHuntEventSession, Fault> {
    let Some(treasure_hunt_entity) = treasure_hunt::find(w.db(), active_persona_id)? else {
        let persona_entity = persona::find(w.db(), active_persona_id)?.ok_or_else(null("PersonaEntity"))?;
        driver_persona::create_th_information(w, &persona_entity)?;
        return get_treasure_hunt_event_session(w, active_persona_id);
    };

    let th_date = treasure_hunt_entity.th_date.ok_or_else(null("TreasureHuntEntity.thDate"))?;
    let now_date = datetime::today(&w.env);

    if th_date != now_date {
        let is_broken = is_streak_broken(w, &treasure_hunt_entity)?;
        return create_new_treasure_hunt(w, treasure_hunt_entity, is_broken);
    }

    get_treasure_hunt_event_session_of(&treasure_hunt_entity)
}

// port: bo/EventsBO#isStreakBroken(TreasureHuntEntity)
/// A hunt from another day breaks the streak when it's two or more days old
/// or wasn't finished.
fn is_streak_broken(w: &World, treasure_hunt_entity: &TreasureHuntEntity) -> Result<bool, Fault> {
    let th_date = treasure_hunt_entity.th_date.ok_or_else(null("TreasureHuntEntity.thDate"))?;
    let now_date = datetime::today(&w.env);

    if th_date != now_date {
        let days = (now_date - th_date).whole_days() as i32;

        return Ok(days >= 2
            || treasure_hunt_entity.coins_collected.ok_or_else(null("TreasureHuntEntity.coinsCollected"))?
                != ALL_COINS);
    }

    Ok(false)
}

// port: bo/EventsBO#getTreasureHuntEventSession(TreasureHuntEntity)
fn get_treasure_hunt_event_session_of(
    treasure_hunt_entity: &TreasureHuntEntity,
) -> Result<TreasureHuntEventSession, Fault> {
    let t = treasure_hunt_entity;
    Ok(TreasureHuntEventSession {
        coins_collected: t.coins_collected.ok_or_else(null("TreasureHuntEntity.coinsCollected"))?,
        is_streak_broken: t.is_streak_broken.ok_or_else(null("TreasureHuntEntity.isStreakBroken"))?,
        num_coins: t.num_coins.ok_or_else(null("TreasureHuntEntity.numCoins"))?,
        seed: t.seed.ok_or_else(null("TreasureHuntEntity.seed"))?,
        streak: t.streak.ok_or_else(null("TreasureHuntEntity.streak"))?,
    })
}

// port: bo/EventsBO#notifyCoinCollected(Long,Integer)
/// A coin collected: `coins` is the bitmask of every coin collected so far,
/// so it must grow by a single coin (a power of two) or not at all. The last
/// coin gives the day's accolades; any other gives nothing. A persona
/// without a hunt gets nothing either.
pub fn notify_coin_collected(
    w: &mut World,
    active_persona_id: i64,
    coins: Option<i32>,
) -> Result<Option<Accolades>, Fault> {
    let Some(mut treasure_hunt_entity) = treasure_hunt::find(w.db(), active_persona_id)? else { return Ok(None) };
    let coins_collected = treasure_hunt_entity.coins_collected.ok_or_else(null("TreasureHuntEntity.coinsCollected"))?;
    if coins_collected == ALL_COINS {
        return Err(not_ready());
    }

    let coins = coins.ok_or_else(null("coins"))?;
    let difference = coins.wrapping_sub(coins_collected);

    // coin difference should be exactly 1 (or 0)
    if difference & difference.wrapping_sub(1) != 0 {
        return Err(Fault::Engine(EngineException::with_message(
            "Invalid coins",
            EngineExceptionCode::SecurityKickedArbitration,
            true,
        )));
    }

    treasure_hunt_entity.coins_collected = Some(coins);
    treasure_hunt::update(w.db(), &treasure_hunt_entity)?;

    if coins == ALL_COINS { accolades(w, active_persona_id, true).map(Some) } else { Ok(None) }
}

// port: bo/EventsBO#accolades(Long,boolean)
/// The day's reward, once every coin is collected and only once. An intact
/// streak pays and grows by a day. A broken one pays nothing when the last
/// coin comes in (the driver may still revive it); asked for again, it
/// starts over at day 1 and pays day 1's reward.
pub fn accolades(w: &mut World, active_persona_id: i64, is_coin_collected: bool) -> Result<Accolades, Fault> {
    /*
       active streak -> you get rewards for your streak, no need to revive
       broken streak ->
           choose to revive -> you get rewards for your streak, back to green
           don't revive -> reset to day 1 + get day 1 rewards, back to green
    */

    let mut persona_entity = persona::find(w.db(), active_persona_id)?;
    let Some(mut treasure_hunt_entity) = treasure_hunt::find(w.db(), active_persona_id)? else {
        return Err(Fault::Engine(EngineException::with_message(
            format!("TreasureHunt does not exist for driver: {active_persona_id}"),
            EngineExceptionCode::InvalidOperation,
            true,
        )));
    };

    if treasure_hunt_entity.coins_collected.ok_or_else(null("TreasureHuntEntity.coinsCollected"))? != ALL_COINS
        || treasure_hunt_entity.is_completed.ok_or_else(null("TreasureHuntEntity.completed"))?
    {
        return Err(not_ready());
    }
    let persona = persona_entity.as_mut().ok_or_else(null("PersonaEntity"))?;

    if !treasure_hunt_entity.is_streak_broken.ok_or_else(null("TreasureHuntEntity.isStreakBroken"))? {
        // active streak -> you get rewards for your streak, no need to revive
        let mut transaction = achievement::create_transaction(active_persona_id);
        let original_streak = treasure_hunt_entity.streak;
        let accolades = get_treasure_hunt_accolades(w, persona, &treasure_hunt_entity, Some(&mut transaction), true)?;
        treasure_hunt_entity.seed = Some(w.env.random(|r| r.next_int()));
        treasure_hunt_entity.is_streak_broken = Some(false);
        let streak = treasure_hunt_entity.streak.ok_or_else(null("TreasureHuntEntity.streak"))?;
        treasure_hunt_entity.streak = Some(streak.wrapping_add(1));
        treasure_hunt_entity.th_date = Some(datetime::today(&w.env));
        treasure_hunt_entity.is_completed = Some(true);
        treasure_hunt::update(w.db(), &treasure_hunt_entity)?;
        let streak_changed = treasure_hunt_entity.streak != original_streak;
        update_treasure_hunt_streak(persona, &treasure_hunt_entity, &mut transaction, streak_changed)?;
        achievement::commit_transaction(w, persona, &mut transaction)?;
        return Ok(accolades);
    }

    // don't revive -> reset to day 1 + get day 1 rewards, back to green

    if is_coin_collected {
        // we don't want to give the driver a reward just yet
        return get_treasure_hunt_accolades(w, persona, &treasure_hunt_entity, None, false);
    }

    // if we get here, the driver hasn't revived their streak
    treasure_hunt_entity.is_streak_broken = Some(false);
    treasure_hunt_entity.streak = Some(1);
    treasure_hunt_entity.th_date = Some(datetime::today(&w.env));
    treasure_hunt_entity.is_completed = Some(true);
    treasure_hunt::update(w.db(), &treasure_hunt_entity)?;
    let mut transaction = achievement::create_transaction(active_persona_id);
    update_treasure_hunt_streak(persona, &treasure_hunt_entity, &mut transaction, true)?;
    let accolades = get_treasure_hunt_accolades(w, persona, &treasure_hunt_entity, Some(&mut transaction), true)?;
    achievement::commit_transaction(w, persona, &mut transaction)?;
    Ok(accolades)
}

// port: bo/EventsBO#createNewTreasureHunt(TreasureHuntEntity,Boolean)
/// Today's hunt, with a new seed and no coins.
fn create_new_treasure_hunt(
    w: &mut World,
    mut treasure_hunt_entity: TreasureHuntEntity,
    is_broken: bool,
) -> Result<TreasureHuntEventSession, Fault> {
    treasure_hunt_entity.coins_collected = Some(0);
    treasure_hunt_entity.is_streak_broken = Some(is_broken);
    treasure_hunt_entity.seed = Some(w.env.random(|r| r.next_int()));
    treasure_hunt_entity.th_date = Some(datetime::today(&w.env));
    treasure_hunt_entity.is_completed = Some(false);
    treasure_hunt::update(w.db(), &treasure_hunt_entity)?;

    get_treasure_hunt_event_session_of(&treasure_hunt_entity)
}

// port: bo/EventsBO#getTreasureHuntAccolades(Long,TreasureHuntEntity,AchievementTransaction,boolean)
/// The day's rewards: the configuration's base times the driver's level (plus
/// the baseline level) and the treasure hunt and server multipliers, plus a
/// bonus for each day of the streak; explorer skill mods and amplifiers on
/// top. Paid only for an intact streak, and only when `give_reward`.
fn get_treasure_hunt_accolades(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    treasure_hunt_entity: &TreasureHuntEntity,
    achievement_transaction: Option<&mut AchievementTransaction>,
    give_reward: bool,
) -> Result<Accolades, Fault> {
    let treasure_hunt_config_entity = treasure_hunt_config::find_for_streak(w.db(), treasure_hunt_entity.streak)?;
    let mut reward_vo = reward::get_reward_vo(w, persona_entity)?;

    let base_rep_th = treasure_hunt_config_entity.base_rep.ok_or_else(null("TreasureHuntConfigEntity.baseRep"))?;
    let base_cash_th = treasure_hunt_config_entity.base_cash.ok_or_else(null("TreasureHuntConfigEntity.baseCash"))?;

    let player_level_cash_const = reward::get_player_level_const(
        persona_entity.level.wrapping_add(w.params.get_int_param_or("REWARD_CASH_BASELINE_LEVEL", 0)?),
        base_cash_th,
    );
    let player_level_rep_const = reward::get_player_level_const(
        persona_entity.level.wrapping_add(w.params.get_int_param_or("REWARD_REP_BASELINE_LEVEL", 0)?),
        base_rep_th,
    );

    let rep_th_multiplier = w.params.get_float_param("TH_REP_MULTIPLIER")?;
    let cash_th_multiplier = w.params.get_float_param("TH_CASH_MULTIPLIER")?;

    let global_rep_multiplier = w.params.get_float_param_or("REP_REWARD_MULTIPLIER", 1.0)?;
    let global_cash_multiplier = w.params.get_float_param_or("CASH_REWARD_MULTIPLIER", 1.0)?;

    let base_rep = player_level_rep_const * rep_th_multiplier * global_rep_multiplier;
    let base_cash = player_level_cash_const * cash_th_multiplier * global_cash_multiplier;

    let rep_day_multiplier =
        treasure_hunt_config_entity.rep_multiplier.ok_or_else(null("TreasureHuntConfigEntity.repMultiplier"))?;
    let cash_day_multiplier =
        treasure_hunt_config_entity.cash_multiplier.ok_or_else(null("TreasureHuntConfigEntity.cashMultiplier"))?;

    let streak = treasure_hunt_entity.streak.ok_or_else(null("TreasureHuntEntity.streak"))?;
    let day_rep = base_rep + (rep_day_multiplier * streak as f32);
    let day_cash = base_cash + (cash_day_multiplier * streak as f32);

    let is_streak_broken =
        treasure_hunt_entity.is_streak_broken.ok_or_else(null("TreasureHuntEntity.isStreakBroken"))?;
    if !is_streak_broken && give_reward {
        reward_vo.set_base_rep(base_rep as i32);
        reward_vo.set_base_cash(base_cash as i32);
        reward_vo.add(day_rep as i32, day_cash as i32, EnumRewardCategory::BASE, EnumRewardType::NONE);
        reward::set_skill_multiplier_reward(w, persona_entity, &mut reward_vo, SkillModRewardType::Explorer)?;
        reward::set_amplifier_reward(w, persona_entity, &mut reward_vo)?;

        reward::apply_race_reward(
            w,
            reward_vo.rep(),
            reward_vo.cash(),
            persona_entity,
            false,
            achievement_transaction,
        )?;
    }

    reward::get_treasure_hunt_accolades(
        w,
        persona_entity,
        treasure_hunt_entity,
        Some(&treasure_hunt_config_entity),
        &reward_vo,
        give_reward,
    )
}

// port: bo/EventsBO#updateTreasureHuntStreak(PersonaEntity,TreasureHuntEntity,AchievementTransaction,boolean)
/// A PROGRESSION update with the streak, for the treasure hunt achievements.
fn update_treasure_hunt_streak(
    persona_entity: &PersonaEntity,
    treasure_hunt_entity: &TreasureHuntEntity,
    achievement_transaction: &mut AchievementTransaction,
    streak_changed: bool,
) -> Result<(), Fault> {
    let progression_context = AchievementProgressionContext::new(
        Some(0),
        Some(0),
        Some(persona_entity.level),
        Some(persona_entity.score),
        treasure_hunt_entity.streak,
        false,
        false,
        streak_changed,
        false,
    );

    achievement_transaction.add(
        "PROGRESSION",
        Bindings::from([
            ("persona".into(), Binding::Persona(persona_entity.clone())),
            ("progression".into(), Binding::Progression(progression_context)),
        ]),
    )
}

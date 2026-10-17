//! `RewardBO` (and `RewardEventBO`, its subclass for events): what finishing
//! an event or a treasure hunt pays, and the lucky draw after it.
//!
//! The arithmetic is `float`, truncated to `int` at each step, as upstream
//! wrote it; the order of every operation is kept.

use anyhow::anyhow;
use sbrw_java::lang::math_min_float;
use sbrw_xml::dto::http::{
    Accolades, ArrayOfLuckyDrawBox, ArrayOfLuckyDrawItem, EnumRewardCategory, EnumRewardType, LuckyDrawBox,
    LuckyDrawInfo, LuckyDrawItem, Reward,
};

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::item_reward::{self, ItemReward};
use crate::bo::util::achievement_progression_context::AchievementProgressionContext;
use crate::bo::util::arbitration::Arbitration;
use crate::bo::util::reward_vo::RewardVO;
use crate::bo::{driver_persona, drop, inventory, persona};
use crate::dao::amplifier;
use crate::dao::car::CarEntity;
use crate::dao::event::EventEntity;
use crate::dao::event_reward::{self, EventRewardEntity};
use crate::dao::event_session::EventSessionEntity;
use crate::dao::inventory_item::{self, InventoryItemEntity};
use crate::dao::level_rep;
use crate::dao::persona::{self as persona_dao, PersonaEntity};
use crate::dao::product::{self, ProductEntity};
use crate::dao::reward_table::{self, RewardTableEntity};
use crate::dao::treasure_hunt::TreasureHuntEntity;
use crate::dao::treasure_hunt_config::TreasureHuntConfigEntity;
use crate::dao::user::{self, UserEntity};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World};

/// `SkillModRewardType`: the skill mods that add to a kind of event's cash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillModRewardType {
    Explorer,
    Socialite,
    BountyHunter,
}

impl SkillModRewardType {
    // port: jpa/SkillModRewardType#toString()
    /// The name with spaces for underscores: a skill mod product's title.
    pub fn to_java_string(self) -> &'static str {
        match self {
            SkillModRewardType::Explorer => "EXPLORER",
            SkillModRewardType::Socialite => "SOCIALITE",
            SkillModRewardType::BountyHunter => "BOUNTY HUNTER",
        }
    }
}

/// `CardDecks`: the deck the lucky draw shows for a rank.
pub mod card_decks {
    // port: bo/util/CardDecks#getCardDeckType()
    const GOLD_CAR: &str = "LD_CARD_SPECIAL_GOLD";
    const GOLD: &str = "LD_CARD_GOLD";
    const SILVER: &str = "LD_CARD_SILVER";
    const BRONZE: &str = "LD_CARD_BRONZE";
    const BLUE: &str = "LD_CARD_BLUE";

    // port: bo/util/CardDecks#forRank(Integer)
    pub fn for_rank(rank: i32) -> &'static str {
        match rank {
            0 => GOLD_CAR,
            1 => GOLD,
            2 => SILVER,
            3 => BRONZE,
            _ => BLUE,
        }
    }
}

/// The persona's LAZY user.
fn user_of(w: &World, persona_entity: &PersonaEntity) -> Result<UserEntity, Fault> {
    let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))
}

/// `personaBO.getDefaultCarEntity` on the persona being rewarded. Upstream
/// shares one persona instance, so a selection it moves back into range
/// shows in this copy too.
pub(crate) fn default_car_of(w: &mut World, persona_entity: &mut PersonaEntity) -> Result<Option<CarEntity>, Fault> {
    let car_entity = persona::get_default_car_entity(w, persona_entity.persona_id)?;
    if let Some(saved) = persona_dao::find(w.db(), persona_entity.persona_id)? {
        persona_entity.cur_car_index = saved.cur_car_index;
    }
    Ok(car_entity)
}

// port: bo/RewardBO#getPlayerLevelConst(int,float)
pub fn get_player_level_const(player_level: i32, level_cash_reward_multiplier: f32) -> f32 {
    level_cash_reward_multiplier * player_level as f32
}

// port: bo/RewardBO#getTimeConst(Long,Long)
/// The event's time limit over the time taken, at most 1. A time of 0
/// makes it infinite, so 1; a limit of 0 as well makes it NaN.
pub fn get_time_const(legit_time: i64, route_time: i64) -> f32 {
    let time_const = legit_time as f32 / route_time as f32;
    math_min_float(time_const, 1.0)
}

// port: bo/RewardBO#getBaseReward(float,float,float,float)
pub fn get_base_reward(base_reward: f32, player_level_const: f32, time_const: f32, global_multiplier: f32) -> i32 {
    let base_reward_result = base_reward * player_level_const * time_const * global_multiplier;
    base_reward_result as i32
}

// port: bo/RewardBO#setBaseReward(PersonaEntity,EventEntity,EventRewardEntity,ArbitrationPacket,RewardVO)
/// The base rep and cash the other parts are worked out from: the event's
/// base reward, times the driver's level times the level multiplier (plus the
/// baseline level), times the time bonus and the server's multiplier.
pub fn set_base_reward(
    w: &World,
    persona_entity: &PersonaEntity,
    event_entity: &EventEntity,
    event_reward_entity: &EventRewardEntity,
    arbitration_packet: &dyn Arbitration,
    reward_vo: &mut RewardVO,
) -> Result<(), Fault> {
    let base_rep = event_reward_entity.base_rep_reward as f32;
    let base_cash = event_reward_entity.base_cash_reward as f32;
    let level = persona_entity.level;
    let baseline_level_cash = w.params.get_int_param_or("REWARD_CASH_BASELINE_LEVEL", 0)?;
    let baseline_level_rep = w.params.get_int_param_or("REWARD_REP_BASELINE_LEVEL", 0)?;
    let player_level_rep_const =
        get_player_level_const(level, event_reward_entity.level_rep_reward_multiplier) + baseline_level_rep as f32;
    let player_level_cash_const =
        get_player_level_const(level, event_reward_entity.level_cash_reward_multiplier) + baseline_level_cash as f32;
    let time_const =
        get_time_const(event_entity.rewards_time_limit, arbitration_packet.event_duration_in_milliseconds());
    reward_vo.set_base_rep(get_base_reward(
        base_rep,
        player_level_rep_const,
        time_const,
        w.params.get_float_param_or("REP_REWARD_MULTIPLIER", 1.0)?,
    ));
    reward_vo.set_base_cash(get_base_reward(
        base_cash,
        player_level_cash_const,
        time_const,
        w.params.get_float_param_or("CASH_REWARD_MULTIPLIER", 1.0)?,
    ));
    Ok(())
}

// port: bo/RewardBO#getRewardVO(PersonaEntity)
/// Cash is paid while the driver is below the cash cap, rep below the top
/// level.
pub fn get_reward_vo(w: &World, persona_entity: &PersonaEntity) -> Result<RewardVO, Fault> {
    let enable_economy = w.params.get_bool_param("ENABLE_ECONOMY")
        && persona_entity.cash < f64::from(w.params.get_max_cash(&user_of(w, persona_entity)?)?);
    let enable_reputation = w.params.get_bool_param("ENABLE_REPUTATION")
        && persona_entity.level < w.params.get_max_level(&user_of(w, persona_entity)?)?;
    Ok(RewardVO::new(enable_economy, enable_reputation))
}

/// `levelRepDao.find(level).getExpPoint()`.
fn exp_point(w: &World, level: i32) -> Result<i64, Fault> {
    level_rep::find(w.db(), i64::from(level))?
        .ok_or_else(|| Fault::null_pointer("LevelRepEntity"))?
        .exp_point
        .ok_or_else(|| Fault::null_pointer("LevelRepEntity.expPoint"))
}

// port: bo/RewardBO#applyRaceReward(Integer,Integer,PersonaEntity,boolean,AchievementTransaction)
/// Pay the cash (capped) and the rep, levelling up as far as it goes. The
/// level-up loop adds the race's rep again for every level it passes, as
/// upstream's does. Then a PROGRESSION update for the achievements.
pub fn apply_race_reward(
    w: &mut World,
    exp: i32,
    cash: i32,
    persona_entity: &mut PersonaEntity,
    is_in_event: bool,
    achievement_transaction: Option<&mut AchievementTransaction>,
) -> Result<(), Fault> {
    let max_level = w.params.get_max_level(&user_of(w, persona_entity)?)?;
    if w.params.get_bool_param("ENABLE_ECONOMY") {
        let new_cash = persona_entity.cash + f64::from(cash);
        driver_persona::update_cash(w, persona_entity, new_cash)?;
    }

    let mut has_level_changed = false;

    if w.params.get_bool_param("ENABLE_REPUTATION") && persona_entity.level < max_level {
        let mut exp_to_next_level = exp_point(w, persona_entity.level)?;
        let mut exp_max = i64::from(persona_entity.rep_at_current_level.wrapping_add(exp));
        if exp_max >= exp_to_next_level {
            let mut is_leveled_up = true;
            has_level_changed = true;
            while is_leveled_up {
                persona_entity.level = persona_entity.level.wrapping_add(1);
                persona_entity.rep_at_current_level = (exp_max - exp_to_next_level) as i32;

                exp_to_next_level = exp_point(w, persona_entity.level)?;
                exp_max = i64::from(persona_entity.rep_at_current_level.wrapping_add(exp));

                is_leveled_up = exp_max >= exp_to_next_level;
                if persona_entity.level >= max_level {
                    is_leveled_up = false;
                }
            }
        } else {
            persona_entity.rep_at_current_level = exp_max as i32;
        }
        persona_entity.rep += f64::from(exp);
    }
    persona_dao::update(w.db(), persona_entity)?;

    if let Some(achievement_transaction) = achievement_transaction {
        let progression_context = AchievementProgressionContext::new(
            Some(cash),
            Some(exp),
            Some(persona_entity.level),
            Some(persona_entity.score),
            Some(0),
            has_level_changed,
            false,
            false,
            is_in_event,
        );
        achievement_transaction.add(
            "PROGRESSION",
            Bindings::from([
                ("persona".into(), Binding::Persona(persona_entity.clone())),
                ("progression".into(), Binding::Progression(progression_context)),
            ]),
        )?;
    }
    Ok(())
}

// port: bo/RewardBO#setTopSpeedReward(EventRewardEntity,float,RewardVO)
pub fn set_top_speed_reward(event_reward_entity: &EventRewardEntity, top_speed: f32, reward_vo: &mut RewardVO) {
    let min_top_speed_trigger = event_reward_entity.min_top_speed_trigger;
    if top_speed >= min_top_speed_trigger {
        let base_rep = reward_vo.base_rep() as f32;
        let base_cash = reward_vo.base_cash() as f32;
        let top_speed_cash_multiplier = event_reward_entity.top_speed_cash_multiplier;
        let top_speed_rep_multiplier = event_reward_entity.top_speed_rep_multiplier;
        let high_speed_rep = base_rep * top_speed_rep_multiplier;
        let high_speed_cash = base_cash * top_speed_cash_multiplier;
        reward_vo.add(high_speed_rep as i32, high_speed_cash as i32, EnumRewardCategory::BONUS, EnumRewardType::NONE);
    }
}

// port: bo/RewardBO#setSkillMultiplierReward(PersonaEntity,RewardVO,SkillModRewardType)
/// The default car's skill mods of the kind add their value, capped by
/// `SKILL_<kind>_MAX_VALUE` (30 by default), as a percentage of the cash so
/// far. A skill mod whose hash isn't a product fails the event.
pub fn set_skill_multiplier_reward(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    reward_vo: &mut RewardVO,
    skill_mod_reward_type: SkillModRewardType,
) -> Result<(), Fault> {
    let default_car_entity = default_car_of(w, persona_entity)?.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    let skill_mod_parts = default_car_entity.skill_mod_parts;
    let mut skill_multiplier = 0f32;
    let max_skill_multiplier =
        w.params.get_float_param_or(&format!("SKILL_{}_MAX_VALUE", skill_mod_reward_type.to_java_string()), 30.0)?;
    for skill_mod_part_entity in &skill_mod_parts {
        let product_entity = product::find_by_hash(w.db(), skill_mod_part_entity.skill_mod_part_attrib_hash)?;
        let title = product_entity.product_title.as_deref().ok_or_else(|| Fault::null_pointer("productTitle"))?;
        if title == skill_mod_reward_type.to_java_string() {
            let skill_value = product_entity.skill_value.ok_or_else(|| Fault::null_pointer("skillValue"))?;
            skill_multiplier += skill_value;
        }
    }
    let final_skill_multiplier = math_min_float(max_skill_multiplier, skill_multiplier) / 100.0;
    let cash = reward_vo.cash() as f32;
    let final_cash = cash * final_skill_multiplier;
    reward_vo.add(0, final_cash as i32, EnumRewardCategory::SKILL_MOD, EnumRewardType::TOKEN_AMPLIFIER);
    Ok(())
}

// port: bo/RewardBO#getAccolades(PersonaEntity,EventRewardEntity,ArbitrationPacket,RewardVO)
/// The result's rewards, whether the driver levelled up (worked out after
/// the rep was added, so a race that levelled up usually says it didn't),
/// and the lucky draw.
pub fn get_accolades(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    event_reward_entity: &EventRewardEntity,
    arbitration_packet: &dyn Arbitration,
    reward_vo: &RewardVO,
) -> Result<Accolades, Fault> {
    let final_rewards = get_final_reward(reward_vo.rep(), reward_vo.cash());
    let has_leveled_up = is_leveled_up(w, persona_entity, reward_vo.rep())?;
    let lucky_draw_info =
        get_event_lucky_draw(w, arbitration_packet.rank(), persona_entity, Some(event_reward_entity))?;
    Ok(Accolades {
        final_rewards: Some(final_rewards),
        has_leveled_up,
        lucky_draw_info: Some(lucky_draw_info),
        original_rewards: Some(get_final_reward(reward_vo.rep(), reward_vo.cash())),
        reward_info: Some(reward_vo.array_of_reward_part().clone()),
    })
}

// port: bo/RewardBO#getAccolades(PersonaEntity,TreasureHuntEntity,TreasureHuntConfigEntity,RewardVO,boolean)
/// A treasure hunt's rewards, and its lucky draw: an item from the streak's
/// table when the hunt gives a reward.
pub fn get_treasure_hunt_accolades(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    treasure_hunt_entity: &TreasureHuntEntity,
    treasure_hunt_config_entity: Option<&TreasureHuntConfigEntity>,
    reward_vo: &RewardVO,
    give_reward: bool,
) -> Result<Accolades, Fault> {
    let final_rewards = get_final_reward(reward_vo.rep(), reward_vo.cash());
    let has_leveled_up = is_leveled_up(w, persona_entity, reward_vo.rep())?;
    let lucky_draw_info = get_treasure_hunt_lucky_draw(
        w,
        persona_entity,
        treasure_hunt_entity,
        treasure_hunt_config_entity,
        give_reward,
    )?;
    Ok(Accolades {
        final_rewards: Some(final_rewards),
        has_leveled_up,
        lucky_draw_info: Some(lucky_draw_info),
        original_rewards: Some(get_final_reward(reward_vo.rep(), reward_vo.cash())),
        reward_info: Some(reward_vo.array_of_reward_part().clone()),
    })
}

// port: bo/RewardBO#setMultiplierReward(EventRewardEntity,RewardVO)
pub fn set_multiplier_reward(event_reward_entity: &EventRewardEntity, reward_vo: &mut RewardVO) {
    let rep = reward_vo.rep() as f32;
    let cash = reward_vo.cash() as f32;
    let final_rep_reward_multiplier = event_reward_entity.final_rep_reward_multiplier;
    let final_cash_reward_multiplier = event_reward_entity.final_cash_reward_multiplier;
    let final_rep = rep * final_rep_reward_multiplier;
    let final_cash = cash * final_cash_reward_multiplier;
    reward_vo.add(final_rep as i32, 0, EnumRewardCategory::AMPLIFIER, EnumRewardType::REP_AMPLIFIER);
    reward_vo.add(0, final_cash as i32, EnumRewardCategory::AMPLIFIER, EnumRewardType::TOKEN_AMPLIFIER);
}

// port: bo/RewardBO#setPerfectStartReward(EventRewardEntity,int,RewardVO)
pub fn set_perfect_start_reward(event_reward_entity: &EventRewardEntity, perfect_start: i32, reward_vo: &mut RewardVO) {
    if perfect_start == 1 {
        let base_rep = reward_vo.base_rep() as f32;
        let base_cash = reward_vo.base_cash() as f32;
        let perfect_start_cash_multiplier = event_reward_entity.perfect_start_cash_multiplier;
        let perfect_start_rep_multiplier = event_reward_entity.perfect_start_rep_multiplier;
        let perfect_start_rep = base_rep * perfect_start_rep_multiplier;
        let perfect_start_cash = base_cash * perfect_start_cash_multiplier;
        reward_vo.add(
            perfect_start_rep as i32,
            perfect_start_cash as i32,
            EnumRewardCategory::BONUS,
            EnumRewardType::NONE,
        );
    }
}

// port: bo/RewardBO#setAmplifierReward(PersonaEntity,RewardVO)
/// Each amplifier the driver has multiplies the cash or rep so far.
pub fn set_amplifier_reward(w: &World, persona_entity: &PersonaEntity, reward_vo: &mut RewardVO) -> Result<(), Fault> {
    for inventory_item_entity in
        inventory_item::find_all_by_persona_id_and_type(w.db(), persona_entity.persona_id, "AMPLIFIER")?
    {
        process_amplifier(w, &inventory_item_entity, reward_vo)?;
    }
    Ok(())
}

// port: bo/RewardBO#setRankReward(EventRewardEntity,ArbitrationPacket,RewardVO)
/// The rank's multiplier on the base; none past eighth.
pub fn set_rank_reward(
    event_reward_entity: &EventRewardEntity,
    route_arbitration_packet: &dyn Arbitration,
    reward_vo: &mut RewardVO,
) {
    let (rank_rep_multiplier, rank_cash_multiplier) = match route_arbitration_packet.rank() {
        rank @ 1..=8 => {
            let i = (rank - 1) as usize;
            (event_reward_entity.rank_rep_multipliers[i], event_reward_entity.rank_cash_multipliers[i])
        }
        _ => (0f32, 0f32),
    };
    let base_rep = reward_vo.base_rep() as f32;
    let base_cash = reward_vo.base_cash() as f32;
    let rank_rep_result = (base_rep * rank_rep_multiplier) as i32;
    let cash_rep_result = (base_cash * rank_cash_multiplier) as i32;
    reward_vo.add(rank_rep_result, cash_rep_result, EnumRewardCategory::BONUS, EnumRewardType::NONE);
}

/// `EnumRewardType.toString()`: the constant's name. The generated variants
/// are named as Java's constants, so `Debug` prints it.
fn enum_reward_type_name(enum_reward_type: EnumRewardType) -> String {
    format!("{enum_reward_type:?}")
}

// port: bo/RewardBO#setPursitParamReward(float,EnumRewardType,RewardVO)
/// A pursuit statistic times the base, times `PURSUIT_<type>_REP_MULTIPLIER`
/// and `PURSUIT_<type>_CASH_MULTIPLIER`, which must be set.
pub fn set_pursit_param_reward(
    w: &World,
    reward_value: f32,
    enum_reward_type: EnumRewardType,
    reward_vo: &mut RewardVO,
) -> Result<(), Fault> {
    let prefix = format!("PURSUIT_{}", enum_reward_type_name(enum_reward_type));
    let reward_multiplier = w.params.get_float_param(&format!("{prefix}_REP_MULTIPLIER"))?;
    let cash_multiplier = w.params.get_float_param(&format!("{prefix}_CASH_MULTIPLIER"))?;
    let base_rep = reward_vo.base_rep() as f32;
    let base_cash = reward_vo.base_cash() as f32;
    let rep_reward = (base_rep * reward_value * reward_multiplier) as i32;
    let cash_reward = (base_cash * reward_value * cash_multiplier) as i32;
    reward_vo.add(rep_reward, cash_reward, EnumRewardCategory::PURSUIT, enum_reward_type);
    Ok(())
}

// port: bo/RewardBO#processAmplifier(InventoryItemEntity,RewardVO)
fn process_amplifier(
    w: &World,
    inventory_item_entity: &InventoryItemEntity,
    reward_vo: &mut RewardVO,
) -> Result<(), Fault> {
    let product_entity = product::find_by_product_id(w.db(), &inventory_item_entity.product_id)?;
    let amplifier_entity = amplifier::find_amplifier_by_hash(w.db(), product_entity.hash)?;
    let null = |what| move || Fault::null_pointer(what);
    match amplifier_entity.amp_type.as_deref().ok_or_else(null("AmplifierEntity.ampType"))? {
        "CASH" => {
            let multiplier = amplifier_entity.cash_multiplier.ok_or_else(null("cashMultiplier"))?;
            let cash = reward_vo.cash() as f32;
            reward_vo.add(
                0,
                ((cash * multiplier) - cash) as i32,
                EnumRewardCategory::AMPLIFIER,
                EnumRewardType::TOKEN_AMPLIFIER,
            );
        }
        "REP" => {
            let multiplier = amplifier_entity.rep_multiplier.ok_or_else(null("repMultiplier"))?;
            let rep = reward_vo.rep() as f32;
            reward_vo.add(
                ((rep * multiplier) - rep) as i32,
                0,
                EnumRewardCategory::AMPLIFIER,
                EnumRewardType::REP_AMPLIFIER,
            );
        }
        _ => {}
    }
    Ok(())
}

// port: bo/RewardBO#getTreasureHuntLuckyDraw(PersonaEntity,TreasureHuntEntity,TreasureHuntConfigEntity,boolean)
/// Five silver boxes, the streak, and the item when there's a reward.
fn get_treasure_hunt_lucky_draw(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    treasure_hunt_entity: &TreasureHuntEntity,
    treasure_hunt_config_entity: Option<&TreasureHuntConfigEntity>,
    give_reward: bool,
) -> Result<LuckyDrawInfo, Fault> {
    let mut array_of_lucky_draw_item = ArrayOfLuckyDrawItem::default();
    if give_reward {
        // A reward that isn't a single item adds a null, which JAXB leaves out.
        array_of_lucky_draw_item.lucky_draw_item.extend(get_treasure_hunt_reward_item(
            w,
            persona_entity,
            treasure_hunt_config_entity,
        )?);
    }
    let lucky_draw_box = LuckyDrawBox {
        is_valid: true,
        localization_string: Some("LD_CARD_SILVER".into()),
        lucky_draw_set_category_id: 1,
    };
    let null = |what| move || Fault::null_pointer(what);
    Ok(LuckyDrawInfo {
        boxes: Some(ArrayOfLuckyDrawBox { lucky_draw_box: vec![lucky_draw_box; 5] }),
        current_streak: treasure_hunt_entity.streak.ok_or_else(null("TreasureHuntEntity.streak"))?,
        is_streak_broken: treasure_hunt_entity
            .is_streak_broken
            .ok_or_else(null("TreasureHuntEntity.isStreakBroken"))?,
        items: Some(array_of_lucky_draw_item),
        num_box_animations: 180,
        ..Default::default()
    })
}

// port: bo/RewardBO#getEventRewardItem(PersonaEntity,EventRewardEntity,Integer)
/// An item from the rank's reward table, or a random droppable product when
/// the rank has none. Ranks outside 1 to 8 fail.
fn get_event_reward_item(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    event_reward_entity: Option<&EventRewardEntity>,
    rank: i32,
) -> Result<Option<LuckyDrawItem>, Fault> {
    let Some(event_reward_entity) = event_reward_entity else {
        return get_random_reward_item(w, persona_entity).map(Some);
    };

    let reward_table_id = match rank {
        1..=8 => event_reward_entity.reward_table_ranks[(rank - 1) as usize],
        _ => return Err(anyhow!("IllegalArgumentException: cannot handle rank {rank}").into()),
    };
    let reward_table_entity = match reward_table_id {
        Some(id) => reward_table::find(w.db(), id)?,
        None => None,
    };

    match reward_table_entity {
        None => get_random_reward_item(w, persona_entity).map(Some),
        Some(reward_table_entity) => get_random_reward_item_from_table(w, persona_entity, &reward_table_entity),
    }
}

// port: bo/RewardBO#getFinalReward(Integer,Integer)
fn get_final_reward(rep: i32, cash: i32) -> Reward {
    Reward { rep, tokens: cash }
}

// port: bo/RewardBO#isLeveledUp(PersonaEntity,Integer)
fn is_leveled_up(w: &World, persona_entity: &PersonaEntity, exp: i32) -> Result<bool, Fault> {
    Ok(i64::from(persona_entity.rep_at_current_level.wrapping_add(exp)) >= exp_point(w, persona_entity.level)?)
}

// port: bo/RewardBO#getEventLuckyDraw(Integer,PersonaEntity,EventRewardEntity)
/// One item for the rank, with the rank's card deck; nothing when
/// `ENABLE_DROP_ITEM` is off or the table's reward isn't a single item.
fn get_event_lucky_draw(
    w: &mut World,
    rank: i32,
    persona_entity: &mut PersonaEntity,
    event_reward_entity: Option<&EventRewardEntity>,
) -> Result<LuckyDrawInfo, Fault> {
    let mut lucky_draw_info = LuckyDrawInfo::default();
    if !w.params.get_bool_param("ENABLE_DROP_ITEM") {
        return Ok(lucky_draw_info);
    }
    let Some(item_from_product) = get_event_reward_item(w, persona_entity, event_reward_entity, rank)? else {
        return Ok(lucky_draw_info);
    };
    lucky_draw_info.card_deck = Some(card_decks::for_rank(rank).to_owned());
    lucky_draw_info.items = Some(ArrayOfLuckyDrawItem { lucky_draw_item: vec![item_from_product] });
    Ok(lucky_draw_info)
}

// port: bo/RewardBO#getTreasureHuntRewardItem(PersonaEntity,TreasureHuntConfigEntity)
/// An item from the configuration's reward table, or a random droppable
/// product without one.
fn get_treasure_hunt_reward_item(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    treasure_hunt_config_entity: Option<&TreasureHuntConfigEntity>,
) -> Result<Option<LuckyDrawItem>, Fault> {
    let Some(treasure_hunt_config_entity) = treasure_hunt_config_entity else {
        return get_random_reward_item(w, persona_entity).map(Some);
    };

    let reward_table_entity = match treasure_hunt_config_entity.reward_table_id {
        Some(id) => reward_table::find(w.db(), id)?,
        None => None,
    };

    match reward_table_entity {
        None => get_random_reward_item(w, persona_entity).map(Some),
        Some(reward_table_entity) => get_random_reward_item_from_table(w, persona_entity, &reward_table_entity),
    }
}

// port: bo/RewardBO#getRandomRewardItem(PersonaEntity)
fn get_random_reward_item(w: &mut World, persona_entity: &mut PersonaEntity) -> Result<LuckyDrawItem, Fault> {
    let product_entity = drop::get_random_product_item(w)?;
    get_lucky_draw_item(w, persona_entity, &product_entity, -1)
}

// port: bo/RewardBO#getRandomRewardItemFromTable(PersonaEntity,RewardTableEntity)
/// A weighted draw from the table: a product is given (or sold), cash is
/// paid at once, and anything else draws nothing.
fn get_random_reward_item_from_table(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    reward_table_entity: &RewardTableEntity,
) -> Result<Option<LuckyDrawItem>, Fault> {
    let reward_base = item_reward::build_table(w, reward_table_entity.name.clone(), true)?;
    match reward_base {
        ItemReward::Product { product, use_count } => {
            let quantity = use_count.unwrap_or(-1);
            get_lucky_draw_item(w, persona_entity, &product, quantity).map(Some)
        }
        ItemReward::Cash(cash) => {
            let lucky_draw_item = LuckyDrawItem {
                was_sold: false,
                resell_price: 0.0,
                remaining_use_count: cash,
                hash: -429893590,
                icon: Some("128_cash".into()),
                virtual_item: Some("TOKEN_REWARD".into()),
                virtual_item_type: Some("REWARD".into()),
                // GM_CATALOG_00000190 is not the correct label to use. EA's server seemed to think it had formatting
                // arguments.
                // LB_CASH actually accepts a formatting argument!
                description: Some(format!("LB_CASH,{cash}")),
            };
            let new_cash = persona_entity.cash + f64::from(cash);
            driver_persona::update_cash(w, persona_entity, new_cash)?;
            Ok(Some(lucky_draw_item))
        }
        ItemReward::Multi(_) => Ok(None),
    }
}

// port: bo/RewardBO#getLuckyDrawItem(PersonaEntity,ProductEntity)
// port: bo/RewardBO#getLuckyDrawItem(PersonaEntity,ProductEntity,int)
/// The product into the driver's inventory (`quantity` uses, -1 for the
/// product's own); without room, it's sold for its resale price instead.
fn get_lucky_draw_item(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    product_entity: &ProductEntity,
    quantity: i32,
) -> Result<LuckyDrawItem, Fault> {
    let mut lucky_draw_item = drop::copy_product2_lucky_draw(product_entity)?;
    let mut inventory_entity = inventory::get_inventory(w, persona_entity)?;
    let inventory_full = !inventory::can_inventory_hold(&inventory_entity, product_entity);
    if inventory_full {
        lucky_draw_item.was_sold = true;
        if w.params.get_bool_param("ENABLE_ECONOMY") {
            let resale_price = product_entity.resale_price;
            let cash = persona_entity.cash;
            driver_persona::update_cash(w, persona_entity, cash + f64::from(resale_price))?;
        }
    } else {
        if product_entity.product_type == "POWERUP" {
            inventory::add_stacked_inventory_item(
                w,
                &mut inventory_entity,
                &product_entity.product_id,
                quantity,
                false,
            )?;
        } else {
            inventory::add_inventory_item(w, &mut inventory_entity, &product_entity.product_id, quantity, None, false)?;
        }
        lucky_draw_item.remaining_use_count = if quantity == -1 { product_entity.use_count } else { quantity };
        lucky_draw_item.description = Some(format!(
            "{} x{}",
            lucky_draw_item.description.as_deref().unwrap_or("null"),
            lucky_draw_item.remaining_use_count
        ));
    }
    Ok(lucky_draw_item)
}

// port: bo/RewardEventBO#getRewardConfiguration(EventSessionEntity)
/// The event's reward configuration for the session: private or public
/// multiplayer for a lobby's session, single-player otherwise.
pub fn get_reward_configuration(
    w: &World,
    event_session_entity: &EventSessionEntity,
) -> Result<EventRewardEntity, Fault> {
    let event_entity = &event_session_entity.event;
    let id = match &event_session_entity.lobby {
        Some(lobby_entity) => {
            if lobby_entity.is_private.ok_or_else(|| Fault::null_pointer("LobbyEntity.isPrivate"))? {
                &event_entity.private_reward_config_id
            } else {
                &event_entity.multiplayer_reward_config_id
            }
        }
        None => &event_entity.singleplayer_reward_config_id,
    };
    event_reward::find(w.db(), id)?
        .ok_or_else(|| Fault::engine(EngineExceptionCode::LuckyDrawNoTableDefinedForRace, true))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use sbrw_xml::dto::http::{PursuitArbitrationPacket, RouteArbitrationPacket};
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Vectors {
        parameter_sets: Vec<BTreeMap<String, String>>,
        configs: Vec<Config>,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Config {
        id: String,
        base_rep_reward: i32,
        base_cash_reward: i32,
        level_rep_reward_multiplier: f32,
        level_cash_reward_multiplier: f32,
        final_rep_reward_multiplier: f32,
        final_cash_reward_multiplier: f32,
        perfect_start_rep_multiplier: f32,
        perfect_start_cash_multiplier: f32,
        top_speed_rep_multiplier: f32,
        top_speed_cash_multiplier: f32,
        rank_rep_multipliers: [f32; 8],
        rank_cash_multipliers: [f32; 8],
        min_top_speed_trigger: f32,
    }

    #[derive(Deserialize)]
    struct Case {
        chain: String,
        parameter_set: usize,
        config: usize,
        level: i32,
        rewards_time_limit: i64,
        event_duration: i64,
        top_speed: f32,
        economy: bool,
        reputation: bool,
        rank: Option<i32>,
        perfect_start: Option<i32>,
        cops_deployed: Option<i32>,
        cops_disabled: Option<i32>,
        cops_rammed: Option<i32>,
        cost_to_state: Option<i32>,
        heat: Option<f32>,
        infractions: Option<i32>,
        road_blocks_dodged: Option<i32>,
        spike_strips_dodged: Option<i32>,
        base_rep: i32,
        base_cash: i32,
        rep: i32,
        cash: i32,
        parts: Vec<String>,
    }

    fn entity(c: &Config) -> EventRewardEntity {
        EventRewardEntity {
            id: c.id.clone(),
            base_rep_reward: c.base_rep_reward,
            base_cash_reward: c.base_cash_reward,
            level_rep_reward_multiplier: c.level_rep_reward_multiplier,
            level_cash_reward_multiplier: c.level_cash_reward_multiplier,
            final_rep_reward_multiplier: c.final_rep_reward_multiplier,
            final_cash_reward_multiplier: c.final_cash_reward_multiplier,
            perfect_start_rep_multiplier: c.perfect_start_rep_multiplier,
            perfect_start_cash_multiplier: c.perfect_start_cash_multiplier,
            top_speed_rep_multiplier: c.top_speed_rep_multiplier,
            top_speed_cash_multiplier: c.top_speed_cash_multiplier,
            rank_rep_multipliers: c.rank_rep_multipliers,
            rank_cash_multipliers: c.rank_cash_multipliers,
            min_top_speed_trigger: c.min_top_speed_trigger,
            reward_table_ranks: [None; 8],
        }
    }

    /// The case's chain of `RewardBO` calls, as `EventRewards.java` makes them.
    fn run(w: &World, config: &EventRewardEntity, case: &Case) -> RewardVO {
        let persona = PersonaEntity { level: case.level, ..Default::default() };
        let event = EventEntity { rewards_time_limit: case.rewards_time_limit, ..Default::default() };
        let mut vo = RewardVO::new(case.economy, case.reputation);
        if case.chain == "route" {
            let packet = RouteArbitrationPacket {
                rank: case.rank.unwrap(),
                perfect_start: case.perfect_start.unwrap(),
                top_speed: case.top_speed,
                event_duration_in_milliseconds: case.event_duration,
                ..Default::default()
            };
            set_base_reward(w, &persona, &event, config, &packet, &mut vo).unwrap();
            set_rank_reward(config, &packet, &mut vo);
            set_perfect_start_reward(config, packet.perfect_start, &mut vo);
            set_top_speed_reward(config, packet.top_speed, &mut vo);
            set_multiplier_reward(config, &mut vo);
        } else {
            let packet = PursuitArbitrationPacket {
                event_duration_in_milliseconds: case.event_duration,
                top_speed: case.top_speed,
                ..Default::default()
            };
            set_base_reward(w, &persona, &event, config, &packet, &mut vo).unwrap();
            let statistics = [
                (case.cops_deployed.unwrap() as f32, EnumRewardType::COP_CARS_DEPLOYED),
                (case.cops_disabled.unwrap() as f32, EnumRewardType::COP_CARS_DISABLED),
                (case.cops_rammed.unwrap() as f32, EnumRewardType::COP_CARS_RAMMED),
                (case.cost_to_state.unwrap() as f32, EnumRewardType::COST_TO_STATE),
                (case.event_duration as f32, EnumRewardType::PURSUIT_LENGTH),
                (case.heat.unwrap(), EnumRewardType::HEAT_LEVEL),
                (case.infractions.unwrap() as f32, EnumRewardType::INFRACTIONS),
                (case.road_blocks_dodged.unwrap() as f32, EnumRewardType::ROADBLOCKS_DODGED),
                (case.spike_strips_dodged.unwrap() as f32, EnumRewardType::SPIKE_STRIPS_DODGED),
            ];
            for (value, reward_type) in statistics {
                set_pursit_param_reward(w, value, reward_type, &mut vo).unwrap();
            }
            set_top_speed_reward(config, packet.top_speed, &mut vo);
            set_multiplier_reward(config, &mut vo);
        }
        vo
    }

    #[test]
    fn rewards_add_up_as_the_original() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/event_rewards.json");
        let vectors: Vectors = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(vectors.cases.len(), 4050);
        let worlds: Vec<World> = vectors
            .parameter_sets
            .iter()
            .map(|set| {
                let overrides: Vec<(&str, &str)> = set.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                World::test(&overrides)
            })
            .collect();
        let configs: Vec<EventRewardEntity> = vectors.configs.iter().map(entity).collect();
        let mut failures = Vec::new();
        for (i, case) in vectors.cases.iter().enumerate() {
            let vo = run(&worlds[case.parameter_set], &configs[case.config], case);
            let parts: Vec<String> = vo
                .array_of_reward_part()
                .reward_part
                .iter()
                .map(|p| {
                    format!(
                        "{} {} {:?} {:?}",
                        p.rep_part,
                        p.token_part,
                        p.reward_category.unwrap(),
                        p.reward_type.unwrap()
                    )
                })
                .collect();
            let ours = (vo.base_rep(), vo.base_cash(), vo.rep(), vo.cash(), parts);
            let theirs = (case.base_rep, case.base_cash, case.rep, case.cash, case.parts.clone());
            if ours != theirs {
                failures
                    .push(format!("case {i} ({} on {}): ours {ours:?}, theirs {theirs:?}", case.chain, case.config));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} differ:\n{}",
            failures.len(),
            vectors.cases.len(),
            failures[..failures.len().min(5)].join("\n")
        );
        // The time bonus's edges are in the data: no time, and no limit.
        assert!(vectors.cases.iter().any(|c| c.event_duration == 0 && c.rewards_time_limit == 0));
        assert!(vectors.cases.iter().any(|c| c.event_duration == 0 && c.rewards_time_limit > 0));
    }
}

//! `ItemRewardBO`: rewards from the reward scripts in the game data (card
//! packs' cards, reward tables, achievement rewards).
//!
//! A script calls `generator` (upstream's `RewardGenerator`) to build its
//! reward: cash, a product, a draw from a reward table or a card pack, or
//! several. `sbrw-script` logs those calls in the order Nashorn made them, and
//! [`script_to_item`] builds each in that order, with the same draws on the
//! shared `java.util.Random`. `fixtures/vectors/reward_scripts.json` holds
//! what the original built for every script in the data, with three seeds.

use anyhow::anyhow;
use sbrw_java::util::{DEFAULT_TABLE, double_stream_sum, hash_order, long_hash_code};
use sbrw_script::{Arg, GeneratorCall};
use sbrw_xml::dto::http::{
    AchievementRewards, ArrayOfCommerceItemTrans, ArrayOfInventoryItemTrans, ArrayOfOwnedCarTrans, CommerceItemTrans,
    CommerceResultTrans,
};

use crate::bo::util::owned_car_converter;
use crate::bo::{basket, driver_persona, inventory};
use crate::dao::car::CarEntity;
use crate::dao::inventory::InventoryEntity;
use crate::dao::inventory_item::{self, InventoryItemEntity};
use crate::dao::persona::PersonaEntity;
use crate::dao::product::{self, ProductEntity};
use crate::dao::{card_pack, reward_table};
use crate::engine::{EngineException, EngineExceptionCode};
use crate::{Fault, World};

/// How deep reward scripts may draw from tables that draw from tables. The
/// data goes three deep; upstream would recurse until its stack overflowed.
const MAX_DEPTH: usize = 64;

// port: bo/util/ItemRewardBase#getProducts()
// port: bo/util/ItemRewardCash#<init>(int)
// port: bo/util/ItemRewardCash#getCash()
// port: bo/util/ItemRewardCash#getProducts()
// port: bo/util/ItemRewardMulti#<init>(List)
// port: bo/util/ItemRewardMulti#getAchievementRewardList()
// port: bo/util/ItemRewardMulti#getProducts()
// port: bo/util/ItemRewardProduct#<init>(ProductEntity)
// port: bo/util/ItemRewardProduct#getProducts()
// port: bo/util/ItemRewardQuantityProduct#<init>(ProductEntity)
// port: bo/util/ItemRewardQuantityProduct#<init>(ProductEntity,Integer)
// port: bo/util/ItemRewardQuantityProduct#getUseCount()
/// `ItemRewardBase` and its subclasses. A product's `getProducts()` is the
/// product itself, which is all `handleReward` asks of it.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemReward {
    /// `ItemRewardCash`.
    Cash(i32),
    /// `ItemRewardMulti`.
    Multi(Vec<ItemReward>),
    /// `ItemRewardProduct`, or an `ItemRewardQuantityProduct` with its use
    /// count (-1: the product's own).
    Product { product: Box<ProductEntity>, use_count: Option<i32> },
}

/// `WrappedInventoryItemReward`: an item given, and how many uses were asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedInventoryItemReward {
    pub inventory_item_entity: InventoryItemEntity,
    pub use_count: i32,
}

/// `WrappedCarReward`: a car given, and the product it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedCarReward {
    pub car_entity: CarEntity,
    pub product_entity: ProductEntity,
}

/// `RewardedItemsContainer`: what a reward gave, in the order it gave it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RewardedItemsContainer {
    pub cash_reward_list: Vec<i32>,
    pub inventory_item_list: Vec<WrappedInventoryItemReward>,
    pub car_reward_list: Vec<WrappedCarReward>,
}

// port: bo/ItemRewardBO#getRewards(PersonaEntity,String)
/// Run a reward script and give the persona what it built. Any failure
/// becomes an `EngineException` (`LuckyDrawCouldNotDrawProduct`).
pub fn get_rewards(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    reward_script: Option<&str>,
) -> Result<RewardedItemsContainer, Fault> {
    let rewards = (|| {
        let Some(reward_script) = reward_script else { return Err(anyhow!("rewardScript was null!").into()) };
        let item = script_to_item(w, reward_script)?;
        let mut inventory_entity = w.ejb(|w| inventory::get_inventory(w, persona_entity))?;
        handle_reward(w, &item, &mut inventory_entity, persona_entity)
    })();
    rewards.map_err(|_| {
        Fault::Engine(EngineException::with_message(
            format!("Failed to generate rewards with script: {}", reward_script.unwrap_or("null")),
            EngineExceptionCode::LuckyDrawCouldNotDrawProduct,
            true,
        ))
    })
}

/// `GenericCommerceResult`: an answer rewards are written into.
pub trait GenericCommerceResult {
    fn commerce_items(&mut self) -> &mut Option<ArrayOfCommerceItemTrans>;
    fn inventory_items(&mut self) -> &mut Option<ArrayOfInventoryItemTrans>;
    fn purchased_cars(&mut self) -> &mut Option<ArrayOfOwnedCarTrans>;
}

impl GenericCommerceResult for CommerceResultTrans {
    fn commerce_items(&mut self) -> &mut Option<ArrayOfCommerceItemTrans> {
        &mut self.commerce_items
    }
    fn inventory_items(&mut self) -> &mut Option<ArrayOfInventoryItemTrans> {
        &mut self.inventory_items
    }
    fn purchased_cars(&mut self) -> &mut Option<ArrayOfOwnedCarTrans> {
        &mut self.purchased_cars
    }
}

impl GenericCommerceResult for AchievementRewards {
    fn commerce_items(&mut self) -> &mut Option<ArrayOfCommerceItemTrans> {
        &mut self.commerce_items
    }
    fn inventory_items(&mut self) -> &mut Option<ArrayOfInventoryItemTrans> {
        &mut self.inventory_items
    }
    fn purchased_cars(&mut self) -> &mut Option<ArrayOfOwnedCarTrans> {
        &mut self.purchased_cars
    }
}

// port: bo/ItemRewardBO#convertRewards(RewardedItemsContainer,GenericCommerceResult)
/// Write what a reward gave into an answer: a line for each cash amount,
/// item and car, the items themselves, and the cars.
pub fn convert_rewards(
    w: &World,
    rewarded_items_container: &RewardedItemsContainer,
    commerce_result: &mut impl GenericCommerceResult,
) -> Result<(), Fault> {
    let mut commerce_items = commerce_result.commerce_items().take().unwrap_or_default();
    let mut inventory_items = commerce_result.inventory_items().take().unwrap_or_default();
    let mut purchased_cars = commerce_result.purchased_cars().take().unwrap_or_default();

    for cash in &rewarded_items_container.cash_reward_list {
        commerce_items
            .commerce_item_trans
            .push(CommerceItemTrans { title: Some(format!("LB_CASH,{cash}")), hash: -429893590 });
    }

    for wrapped in &rewarded_items_container.inventory_item_list {
        // The item is the managed entity, so it shows the stack as it is now,
        // after any later reward that added to it.
        let inventory_item_entity = inventory_item::find(w.db(), wrapped.inventory_item_entity.id)?
            .unwrap_or_else(|| wrapped.inventory_item_entity.clone());
        let product_entity = product::find_by_product_id(w.db(), &inventory_item_entity.product_id)?;
        let mut title = product_entity.product_title.clone();
        if wrapped.use_count != -1 {
            title = Some(format!("{} x{}", title.as_deref().unwrap_or("null"), wrapped.use_count));
        }
        commerce_items.commerce_item_trans.push(CommerceItemTrans {
            hash: product_entity.hash.ok_or_else(|| Fault::null_pointer("ProductEntity.hash"))?,
            title,
        });
        let inventory_entity = InventoryEntity { id: inventory_item_entity.inventory_id, ..Default::default() };
        inventory_items.inventory_item_trans.push(inventory::convert_item_to_item_trans(
            w,
            &inventory_entity,
            &inventory_item_entity,
        )?);
    }

    for car_reward in &rewarded_items_container.car_reward_list {
        purchased_cars.owned_car_trans.push(owned_car_converter::entity2_trans(&car_reward.car_entity));
        let product_entity = &car_reward.product_entity;
        commerce_items.commerce_item_trans.push(CommerceItemTrans {
            hash: product_entity.hash.ok_or_else(|| Fault::null_pointer("ProductEntity.hash"))?,
            title: product_entity.product_title.clone(),
        });
    }

    *commerce_result.commerce_items() = Some(commerce_items);
    *commerce_result.inventory_items() = Some(inventory_items);
    *commerce_result.purchased_cars() = Some(purchased_cars);
    Ok(())
}

// port: bo/ItemRewardBO#scriptToItem(String)
// port: bo/ItemRewardBO#scriptToItem(String,Map)
// port: bo/ItemRewardBO#getGenerator()
/// Run a reward script and build the reward it returns. A script that fails
/// in JavaScript is an `EngineException`; a failure while building a reward
/// is passed on as it is, as Nashorn passed on Java's `RuntimeException`s.
pub fn script_to_item(w: &mut World, reward_script: &str) -> Result<ItemReward, Fault> {
    script_to_item_at(w, reward_script, 0)
}

fn script_to_item_at(w: &mut World, reward_script: &str, depth: usize) -> Result<ItemReward, Fault> {
    if depth > MAX_DEPTH {
        return Err(anyhow!("StackOverflowError: reward scripts nested past {MAX_DEPTH} levels").into());
    }
    let run = sbrw_script::eval_reward(reward_script);
    let mut built: Vec<ItemReward> = Vec::with_capacity(run.calls.len());
    for call in &run.calls {
        let reward = build(w, call, &built, depth)?;
        built.push(reward);
    }
    match run.result {
        Err(_) => Err(Fault::Engine(EngineException::with_message(
            format!("Failed to execute script: {reward_script}"),
            EngineExceptionCode::LuckyDrawCouldNotDrawProduct,
            true,
        ))),
        Ok(Arg::Reward(index)) => built.get(index).cloned().ok_or_else(|| anyhow!("no such reward: {index}").into()),
        Ok(Arg::Null | Arg::Undefined) => Err(Fault::null_pointer("the script's result")),
        Ok(other) => Err(anyhow!("Invalid script return: {}", java_class(&other)).into()),
    }
}

/// The class of the object Nashorn returned for a value.
fn java_class(value: &Arg) -> &'static str {
    match value {
        Arg::Boolean(_) => "java.lang.Boolean",
        Arg::Integer(_) => "java.lang.Integer",
        Arg::Double(_) => "java.lang.Double",
        Arg::String(_) => "java.lang.String",
        _ => "jdk.nashorn.api.scripting.ScriptObjectMirror",
    }
}

/// One of `RewardGenerator`'s methods, called with these arguments; `built`
/// holds the rewards earlier calls built.
fn build(w: &mut World, call: &GeneratorCall, built: &[ItemReward], depth: usize) -> Result<ItemReward, Fault> {
    let arg = |i: usize| call.args.get(i).cloned().unwrap_or(Arg::Undefined);
    match call.method.as_str() {
        "cashReward" => Ok(ItemReward::Cash(to_int(&arg(0)))),
        "findRandomRatedItem" => {
            let selection =
                ProductSelection { sub_type: to_string(&arg(0)), rating: to_integer(&arg(1)), ..Default::default() };
            build_product_selection(w, &selection)
        }
        "findRandomRatedItemByProdType" => {
            let selection = ProductSelection {
                product_type: to_string(&arg(0)),
                rating: to_integer(&arg(1)),
                ..Default::default()
            };
            build_product_selection(w, &selection)
        }
        "findRandomItemByProdType" => {
            build_product_selection(w, &ProductSelection { product_type: to_string(&arg(0)), ..Default::default() })
        }
        "findWeightedRandomItemByProdType" => {
            let selection = ProductSelection { product_type: to_string(&arg(0)), weighted: true, ..Default::default() };
            build_product_selection(w, &selection)
        }
        "generateSingleItem" => generate_single_item(w, to_string(&arg(0)).as_deref()),
        "getCardPack" => get_card_pack(w, to_string(&arg(0)).as_deref(), depth),
        "multiItems" => {
            let mut product_entities = Vec::new();
            for entitlement_tag in to_strings(&arg(0))? {
                product_entities.push(generate_single_item(w, entitlement_tag.as_deref())?);
            }
            Ok(ItemReward::Multi(product_entities))
        }
        "multipleRewards" => Ok(ItemReward::Multi(to_rewards(&arg(0), built)?)),
        "randomDrop" => {
            // `random().withBuilders(...)`: every choice is built, then one is drawn.
            let mut choices = Vec::new();
            for entitlement_tag in to_strings(&arg(0))? {
                let selection = ProductSelection { entitlement_tag, ..Default::default() };
                choices.push(build_product_selection(w, &selection)?);
            }
            random_choice(w, choices)
        }
        "randomSelection" => {
            let rewards = to_rewards(&arg(0), built)?;
            if rewards.is_empty() {
                return Err(anyhow!("IllegalArgumentException: No rewards to choose from!").into());
            }
            random_choice(w, rewards)
        }
        "randomTableItem" => build_table_selection(w, to_string(&arg(0)), false, depth),
        "rewardQuantityProduct" => {
            let quantity = to_integer(&arg(1)).ok_or_else(|| Fault::null_pointer("quantity"))?;
            if quantity < -1 {
                return Err(anyhow!("IllegalArgumentException: quantity < -1").into());
            }
            let selection = ProductSelection { entitlement_tag: to_string(&arg(0)), quantity, ..Default::default() };
            build_product_selection(w, &selection)
        }
        "weightedRandomTableItem" => build_table_selection(w, to_string(&arg(0)), true, depth),
        other => Err(anyhow!("TypeError: generator.{other} is not a function").into()),
    }
}

/// `RewardGenerator.generateSingleItem`: the product with this tag. A missing
/// one throws in the DAO, so upstream's null check after it never fires.
fn generate_single_item(w: &mut World, entitlement_tag: Option<&str>) -> Result<ItemReward, Fault> {
    let product_entity = w.ejb(|w| find_by_entitlement_tag(w, entitlement_tag))?;
    Ok(ItemReward::Product { product: Box::new(product_entity), use_count: None })
}

/// `productDAO.findByEntitlementTag`, where a null tag matches nothing.
fn find_by_entitlement_tag(w: &World, entitlement_tag: Option<&str>) -> Result<ProductEntity, Fault> {
    match entitlement_tag {
        Some(tag) => product::find_by_entitlement_tag(w.db(), tag),
        None => Err(anyhow!("Could not find product with tag: null").into()),
    }
}

/// `RewardGenerator.getCardPack`: a reward for each card of the pack.
fn get_card_pack(w: &mut World, pack_id: Option<&str>, depth: usize) -> Result<ItemReward, Fault> {
    let card_pack_entity = w.ejb(|w| match pack_id {
        Some(pack_id) => card_pack::find_by_entitlement_tag(w.db(), pack_id),
        None => Err(anyhow!("NoResultException: No entity found for query").into()),
    })?;
    let mut items = Vec::new();
    for card_pack_item_entity in &card_pack_entity.items {
        let item = script_to_item_at(w, &card_pack_item_entity.script, depth + 1).map_err(|e| {
            Fault::Internal(anyhow!(
                "Error while generating card pack {} - could not execute script {}: {e}",
                pack_id.unwrap_or("null"),
                card_pack_item_entity.script
            ))
        })?;
        items.push(item);
    }
    Ok(ItemReward::Multi(items))
}

/// `RandomSelectionBuilder.build`: one of the choices, by `nextInt`.
fn random_choice(w: &mut World, mut choices: Vec<ItemReward>) -> Result<ItemReward, Fault> {
    if choices.is_empty() {
        return Err(anyhow!("IllegalArgumentException: bound must be positive").into());
    }
    let bound = i32::try_from(choices.len()).map_err(|e| anyhow!("{e}"))?;
    let index = w.env.random(|r| r.next_int_bounded(bound));
    Ok(choices.swap_remove(index as usize))
}

/// `ProductSelectionBuilder`'s settings.
#[derive(Debug, Clone)]
struct ProductSelection {
    weighted: bool,
    entitlement_tag: Option<String>,
    category: Option<String>,
    product_type: Option<String>,
    sub_type: Option<String>,
    rating: Option<i32>,
    quantity: i32,
}

impl Default for ProductSelection {
    fn default() -> Self {
        Self {
            weighted: false,
            entitlement_tag: None,
            category: None,
            product_type: None,
            sub_type: None,
            rating: None,
            quantity: -1,
        }
    }
}

/// `ProductSelectionBuilder.build`: the product with the entitlement tag, or
/// one drawn from those with the traits, by drop weight or evenly.
fn build_product_selection(w: &mut World, selection: &ProductSelection) -> Result<ItemReward, Fault> {
    let quantity = Some(selection.quantity);
    if let Some(entitlement_tag) = selection.entitlement_tag.as_deref().filter(|t| !t.is_empty()) {
        let product_entity = w.ejb(|w| product::find_by_entitlement_tag(w.db(), entitlement_tag))?;
        return Ok(ItemReward::Product { product: Box::new(product_entity), use_count: quantity });
    }

    let product_entities = w.ejb(|w| {
        Ok(product::find_by_traits(
            w.db(),
            selection.category.as_deref(),
            selection.product_type.as_deref(),
            selection.sub_type.as_deref(),
            selection.rating,
        )?)
    })?;

    let null = |s: &Option<String>| s.clone().unwrap_or_else(|| "null".into());
    let debug_format = format!(
        "C={} PT={} ST={} R={}",
        null(&selection.category),
        null(&selection.product_type),
        null(&selection.sub_type),
        selection.rating.map_or_else(|| "null".into(), |r| r.to_string())
    );
    if product_entities.is_empty() {
        return Err(anyhow!("No products to choose from! {debug_format}").into());
    }

    let num_products = product_entities.len();
    let index = if selection.weighted {
        let weights: Vec<(i64, Option<f64>)> = product_entities.iter().map(|p| (p.id, p.drop_weight)).collect();
        weighted_index(w, &weights).ok_or_else(|| anyhow!("Weighted random failed! {debug_format}"))?
    } else {
        let bound = i32::try_from(num_products).map_err(|e| anyhow!("{e}"))?;
        w.env.random(|r| r.next_int_bounded(bound)) as usize
    };
    let product_entity = product_entities.into_iter().nth(index).ok_or_else(|| anyhow!("index {index}"))?;
    Ok(ItemReward::Product { product: Box::new(product_entity), use_count: quantity })
}

/// The weighted draw both builders make: weights by row id in a
/// `HashMap<Long, Double>` (a missing weight is an even share), summed in
/// the map's order with `DoubleStream.sum()`; then `Math.random()` times the
/// sum, less each row's weight in list order, until it reaches 0. `None`
/// when it never does.
fn weighted_index(w: &World, rows: &[(i64, Option<f64>)]) -> Option<usize> {
    let mut random = w.env.random(|r| r.next_double()) * weight_sum(rows);
    for i in 0..rows.len() {
        random -= weight(rows, i);
        if random <= 0.0 {
            return Some(i);
        }
    }
    None
}

/// A row's weight: its own, or an even share. (Every row in the data has
/// its own.)
fn weight(rows: &[(i64, Option<f64>)], i: usize) -> f64 {
    rows[i].1.unwrap_or(1.0 / rows.len() as f64)
}

/// `weights.values().stream().mapToDouble(d -> d).sum()` for a
/// `HashMap<Long, Double>` of the rows' weights by id
/// (`fixtures/vectors/java_weight_sum.json`).
fn weight_sum(rows: &[(i64, Option<f64>)]) -> f64 {
    let hashes: Vec<i32> = rows.iter().map(|&(id, _)| long_hash_code(id)).collect();
    double_stream_sum(hash_order(&hashes, DEFAULT_TABLE).into_iter().map(|i| weight(rows, i)))
}

/// `getGenerator().table().tableName(name).weighted(weighted).build()`, as
/// Java code calls it.
pub fn build_table(w: &mut World, table_name: Option<String>, weighted: bool) -> Result<ItemReward, Fault> {
    build_table_selection(w, table_name, weighted, 0)
}

/// `TableSelectionBuilder.build`: a script drawn from the table, by drop
/// weight or evenly, and the reward it builds.
fn build_table_selection(
    w: &mut World,
    table_name: Option<String>,
    weighted: bool,
    depth: usize,
) -> Result<ItemReward, Fault> {
    let table_name = table_name.ok_or_else(|| Fault::null_pointer("tableName"))?;
    let reward_table_entity = w.ejb(|w| reward_table::find_by_name(w.db(), &table_name))?;
    let items = &reward_table_entity.items;

    if items.is_empty() {
        return Err(Fault::Engine(EngineException::with_message(
            format!("No items are available in table {table_name}"),
            EngineExceptionCode::LuckyDrawContextNotFoundOrEmpty,
            true,
        )));
    }

    let index = if weighted {
        let weights: Vec<(i64, Option<f64>)> = items.iter().map(|i| (i.id, i.drop_weight)).collect();
        weighted_index(w, &weights).ok_or_else(|| {
            Fault::Engine(EngineException::with_message(
                format!("Weighted random failed for {table_name}"),
                EngineExceptionCode::LuckyDrawCouldNotDrawProduct,
                true,
            ))
        })?
    } else {
        let bound = i32::try_from(items.len()).map_err(|e| anyhow!("{e}"))?;
        w.env.random(|r| r.next_int_bounded(bound)) as usize
    };

    let script = items[index].script.clone();
    script_to_item_at(w, &script, depth + 1).map_err(|_| {
        Fault::Engine(EngineException::with_message(
            format!("Could not execute script: {script}"),
            EngineExceptionCode::LuckyDrawCouldNotDrawProduct,
            true,
        ))
    })
}

// port: bo/ItemRewardBO#handleReward(ItemRewardBase,InventoryEntity,PersonaEntity)
/// Give the persona a reward: cash, each of several in turn, or a product (a
/// car, a part, or uses of a powerup; other products give nothing).
fn handle_reward(
    w: &mut World,
    item_reward_base: &ItemReward,
    inventory_entity: &mut InventoryEntity,
    persona_entity: &mut PersonaEntity,
) -> Result<RewardedItemsContainer, Fault> {
    let mut rewarded_items_container = RewardedItemsContainer::default();
    match item_reward_base {
        ItemReward::Cash(cash) => {
            let new_cash = persona_entity.cash + f64::from(*cash);
            w.ejb(|w| driver_persona::update_cash(w, persona_entity, new_cash))?;
            rewarded_items_container.cash_reward_list.push(*cash);
        }
        ItemReward::Multi(rewards) => {
            for reward in rewards {
                let new_container = handle_reward(w, reward, inventory_entity, persona_entity)?;
                rewarded_items_container.inventory_item_list.extend(new_container.inventory_item_list);
                rewarded_items_container.car_reward_list.extend(new_container.car_reward_list);
                rewarded_items_container.cash_reward_list.extend(new_container.cash_reward_list);
            }
        }
        ItemReward::Product { product, use_count } => {
            let use_count = use_count.unwrap_or(-1);
            match product.product_type.to_lowercase().as_str() {
                "presetcar" => {
                    let car_entity = w.ejb(|w| basket::add_car(w, product, persona_entity))?;
                    rewarded_items_container
                        .car_reward_list
                        .push(WrappedCarReward { car_entity, product_entity: (**product).clone() });
                }
                "performancepart" | "skillmodpart" | "visualpart" => {
                    let inventory_item_entity = w.ejb(|w| {
                        inventory::add_inventory_item(w, inventory_entity, &product.product_id, use_count, None, true)
                    })?;
                    rewarded_items_container
                        .inventory_item_list
                        .push(WrappedInventoryItemReward { inventory_item_entity, use_count });
                }
                "powerup" => {
                    let inventory_item_entity = w.ejb(|w| {
                        inventory::add_stacked_inventory_item(w, inventory_entity, &product.product_id, use_count, true)
                    })?;
                    rewarded_items_container
                        .inventory_item_list
                        .push(WrappedInventoryItemReward { inventory_item_entity, use_count });
                }
                _ => {}
            }
        }
    }
    Ok(rewarded_items_container)
}

/// A `String` argument as Nashorn converted it: null for null, the string
/// form of anything else.
fn to_string(value: &Arg) -> Option<String> {
    match value {
        Arg::Null => None,
        Arg::Undefined => Some("undefined".into()),
        Arg::Boolean(b) => Some(b.to_string()),
        Arg::Integer(i) => Some(i.to_string()),
        Arg::Double(d) => Some(d.to_string()),
        Arg::String(s) | Arg::Object(s) => Some(s.clone()),
        Arg::Array(items) => Some(items.iter().map(|i| to_string(i).unwrap_or_default()).collect::<Vec<_>>().join(",")),
        Arg::Reward(_) => Some("[object Object]".into()),
    }
}

/// An `Integer` argument: null for null or undefined, otherwise the number
/// as JavaScript's `ToInt32` makes it.
fn to_integer(value: &Arg) -> Option<i32> {
    match value {
        Arg::Null | Arg::Undefined => None,
        other => Some(to_int(other)),
    }
}

/// An `int` argument: JavaScript's `ToInt32`.
fn to_int(value: &Arg) -> i32 {
    let number = match value {
        Arg::Integer(i) => return *i,
        Arg::Double(d) => *d,
        Arg::Boolean(b) => f64::from(u8::from(*b)),
        Arg::String(s) => {
            let s = s.trim();
            if s.is_empty() { 0.0 } else { s.parse().unwrap_or(f64::NAN) }
        }
        _ => f64::NAN,
    };
    if !number.is_finite() {
        return 0;
    }
    (number.trunc().rem_euclid(4_294_967_296.0) as u32) as i32
}

/// A `List<String>` or `String[]` argument, from a JavaScript array.
fn to_strings(value: &Arg) -> Result<Vec<Option<String>>, Fault> {
    match value {
        Arg::Array(items) => Ok(items.iter().map(to_string).collect()),
        Arg::Null => Err(Fault::null_pointer("the list")),
        other => Err(anyhow!("ClassCastException: cannot convert {other:?} to a list").into()),
    }
}

/// A `List<ItemRewardBase>` or `ItemRewardBase[]` argument: rewards earlier
/// calls built.
fn to_rewards(value: &Arg, built: &[ItemReward]) -> Result<Vec<ItemReward>, Fault> {
    match value {
        Arg::Array(items) => items
            .iter()
            .map(|item| match item {
                Arg::Reward(index) => {
                    built.get(*index).cloned().ok_or_else(|| anyhow!("no such reward: {index}").into())
                }
                other => Err(anyhow!("ClassCastException: {other:?} is not a reward").into()),
            })
            .collect(),
        Arg::Null => Err(Fault::null_pointer("the rewards")),
        other => Err(anyhow!("ClassCastException: cannot convert {other:?} to a list").into()),
    }
}

#[cfg(test)]
mod tests {
    use sbrw_common::Env;
    use serde_json::{Value, json};

    use super::*;

    fn describe(reward: &ItemReward) -> Value {
        match reward {
            ItemReward::Cash(cash) => json!({ "cash": cash }),
            ItemReward::Multi(rewards) => json!({ "multi": rewards.iter().map(describe).collect::<Vec<_>>() }),
            ItemReward::Product { product, use_count: Some(n) } => {
                json!({ "product": product.product_id, "use_count": n })
            }
            ItemReward::Product { product, use_count: None } => json!({ "product": product.product_id }),
        }
    }

    #[test]
    fn sums_weights_as_the_original() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/java_weight_sum.json");
        let vectors: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut checked = 0;
        for case in vectors["cases"].as_array().unwrap() {
            let ids: Vec<i64> = serde_json::from_value(case["ids"].clone()).unwrap();
            if (1..ids.len()).any(|i| ids[..i].contains(&ids[i])) {
                continue; // Row ids are distinct.
            }
            let rows: Vec<(i64, Option<f64>)> = ids
                .iter()
                .zip(case["weight_bits"].as_array().unwrap())
                .map(|(&id, bits)| (id, Some(f64::from_bits(u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap()))))
                .collect();
            let expected = u64::from_str_radix(case["sum_bits"].as_str().unwrap(), 16).unwrap();
            assert_eq!(weight_sum(&rows).to_bits(), expected, "ids {ids:?}");
            checked += 1;
        }
        assert!(checked > 100, "{checked}");
    }

    #[test]
    fn builds_every_reward_script_as_the_original() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/reward_scripts.json");
        let vectors: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 2322);
        let mut w = World::test(&[]);
        let mut failures = Vec::new();
        for case in cases {
            let script = case["script"].as_str().unwrap();
            let seed = case["seed"].as_i64().unwrap();
            w.env = Env::deterministic(seed, time::macros::datetime!(2026-01-01 00:00 UTC));
            let built = w.transaction(|w| script_to_item(w, script));
            let next_int = w.env.random(|r| r.next_int());
            let same = match (&built, case.get("reward")) {
                (Ok(reward), Some(expected)) => describe(reward) == *expected,
                (Err(fault), None) => {
                    // The original's outermost exception: an EngineException for a script that fails in JavaScript.
                    let class = case["error"][0]["class"].as_str().unwrap();
                    class == "com.soapboxrace.core.engine.EngineException" && matches!(fault, Fault::Engine(_))
                }
                _ => false,
            };
            if !same || next_int != case["next_int"].as_i64().unwrap() as i32 {
                failures.push(format!("{script} (seed {seed}): {:?}", built.map(|r| describe(&r))));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} differ, first: {:#?}",
            failures.len(),
            cases.len(),
            &failures[..failures.len().min(5)]
        );
    }
}

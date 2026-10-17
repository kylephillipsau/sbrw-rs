//! `CommerceBO`: customising the selected car in the shops. The game sends
//! the car as it wants it; what changed is priced, paid for or taken from the
//! inventory, and saved.

use sbrw_java::util::{MULTIMAP_TABLE, hash_order};
use sbrw_xml::dto::http::{
    ArrayOfCustomVinylTrans, ArrayOfInventoryItemTrans, ArrayOfWalletTrans, CommerceResultStatus,
    CommerceSessionResultTrans, CommerceSessionTrans, CustomCarTrans, CustomPaintTrans, CustomVinylTrans,
    InvalidBasketTrans, PerformancePartTrans, SkillModPartTrans, VisualPartTrans, WalletTrans,
};

use crate::bo::achievement_transaction::Binding;
use crate::bo::util::achievement_customization_context::{AchievementCustomizationContext, Type, WrappedPart};
use crate::bo::util::list_differences::ListDifferences;
use crate::bo::util::owned_car_converter;
use crate::bo::{achievement, driver_persona, inventory, performance, persona as persona_bo};
use crate::dao::{car, inventory_item, persona, product, vinyl_product};
use crate::{Fault, World};

/// A part of the car, as the multimaps below hold it.
#[derive(Debug, Clone)]
enum Part {
    Paint,
    Performance,
    SkillMod,
    Visual,
    Vinyl,
}

/// Guava's `ArrayListMultimap<Integer, Object>`: keys in `HashMap` order,
/// each key's values in the order they were put
/// (`fixtures/vectors/java_multimap_order.json`).
#[derive(Default)]
struct Multimap {
    keys: Vec<i32>,
    values: Vec<Vec<Part>>,
}

impl Multimap {
    fn put(&mut self, key: i32, value: Part) {
        match self.keys.iter().position(|&k| k == key) {
            Some(i) => self.values[i].push(value),
            None => {
                self.keys.push(key);
                self.values.push(vec![value]);
            }
        }
    }

    fn entries(&self) -> Vec<(i32, Part)> {
        hash_order(&self.keys, MULTIMAP_TABLE)
            .into_iter()
            .flat_map(|i| self.values[i].iter().map(move |v| (self.keys[i], v.clone())))
            .collect()
    }
}

/// A list the XML must have: upstream reads it without a null check.
fn list<'a, A, T>(array: &'a Option<A>, items: impl FnOnce(&'a A) -> &'a Vec<T>, what: &str) -> Result<&'a [T], Fault> {
    array.as_ref().map(|a| items(a).as_slice()).ok_or_else(|| Fault::null_pointer(what))
}

fn paints(car: &CustomCarTrans) -> Result<&[CustomPaintTrans], Fault> {
    list(&car.paints, |a| &a.custom_paint_trans, "CustomCarTrans.paints")
}

fn vinyls(car: &CustomCarTrans) -> Result<&[CustomVinylTrans], Fault> {
    list(&car.vinyls, |a| &a.custom_vinyl_trans, "CustomCarTrans.vinyls")
}

fn performance_parts(car: &CustomCarTrans) -> Result<&[PerformancePartTrans], Fault> {
    list(&car.performance_parts, |a| &a.performance_part_trans, "CustomCarTrans.performanceParts")
}

fn visual_parts(car: &CustomCarTrans) -> Result<&[VisualPartTrans], Fault> {
    list(&car.visual_parts, |a| &a.visual_part_trans, "CustomCarTrans.visualParts")
}

fn skill_mod_parts(car: &CustomCarTrans) -> Result<&[SkillModPartTrans], Fault> {
    list(&car.skill_mod_parts, |a| &a.skill_mod_part_trans, "CustomCarTrans.skillModParts")
}

// port: bo/CommerceBO#doCommerce(CommerceSessionTrans,Long)
/// Apply the shops' changes to the persona's selected car.
///
/// Parts added are priced by their product (found by hash: a paint's group,
/// a part's hash; a vinyl by its vinyl product). One the inventory holds is
/// used instead, unless its product is also in the basket; parts removed
/// sell back for their resale price, vinyls for nothing. Items the game lists
/// to sell are sold. Upstream checks the basket in the multimap's hash order
/// and stops at the first invalid part, and sells before it checks the
/// funds; inventory used or sold on the way stays used or sold.
pub fn do_commerce(
    w: &mut World,
    commerce_session_trans: CommerceSessionTrans,
    persona_id: i64,
) -> Result<CommerceSessionResultTrans, Fault> {
    let basket_items = commerce_session_trans
        .basket
        .ok_or_else(|| Fault::null_pointer("CommerceSessionTrans.basket"))?
        .items
        .ok_or_else(|| Fault::null_pointer("BasketTrans.items"))?
        .basket_item_trans;
    let car_entity = persona_bo::get_default_car_entity(w, persona_id)?;
    let owned_car_trans = persona_bo::get_default_car(w, persona_id)?;
    // Upstream finds the persona first; this copy also has the selection
    // `getDefaultCarEntity` may have saved, as its managed entity would.
    let mut persona_entity = persona::find(w.db(), persona_id)?;
    let custom_car_trans = owned_car_trans.custom_car.ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))?;
    let mut commerce_custom_car = commerce_session_trans
        .updated_car
        .ok_or_else(|| Fault::null_pointer("CommerceSessionTrans.updatedCar"))?
        .custom_car
        .ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))?;

    let mut array_of_custom_vinyl_trans = ArrayOfCustomVinylTrans::default();
    for cvt in vinyls(&commerce_custom_car)? {
        let mut cvt = cvt.clone();
        if cvt.scale_x == 3277 {
            cvt.scale_x = 0;
        }
        if cvt.scale_y == 3277 {
            cvt.scale_y = 0;
        }
        array_of_custom_vinyl_trans.custom_vinyl_trans.push(cvt);
    }
    commerce_custom_car.vinyls = Some(array_of_custom_vinyl_trans);

    let paint_differences = ListDifferences::get_differences(paints(&custom_car_trans)?, paints(&commerce_custom_car)?);
    let vinyl_differences = ListDifferences::get_differences(vinyls(&custom_car_trans)?, vinyls(&commerce_custom_car)?);
    let performance_part_differences = ListDifferences::get_differences(
        performance_parts(&custom_car_trans)?,
        performance_parts(&commerce_custom_car)?,
    );
    let visual_part_differences =
        ListDifferences::get_differences(visual_parts(&custom_car_trans)?, visual_parts(&commerce_custom_car)?);
    let skill_mod_part_differences =
        ListDifferences::get_differences(skill_mod_parts(&custom_car_trans)?, skill_mod_parts(&commerce_custom_car)?);

    let mut added_items = Multimap::default();
    let mut removed_items = Multimap::default();
    let paints_added = &paint_differences.added;
    paints_added.iter().for_each(|p| added_items.put(p.group, Part::Paint));
    paint_differences.removed.iter().for_each(|p| removed_items.put(p.group, Part::Paint));
    let perf_parts_added = &performance_part_differences.added;
    perf_parts_added.iter().for_each(|p| added_items.put(p.performance_part_attrib_hash, Part::Performance));
    performance_part_differences
        .removed
        .iter()
        .for_each(|p| removed_items.put(p.performance_part_attrib_hash, Part::Performance));
    let skill_mods_added = &skill_mod_part_differences.added;
    skill_mods_added.iter().for_each(|p| added_items.put(p.skill_mod_part_attrib_hash, Part::SkillMod));
    skill_mod_part_differences
        .removed
        .iter()
        .for_each(|p| removed_items.put(p.skill_mod_part_attrib_hash, Part::SkillMod));
    let visual_parts_added = &visual_part_differences.added;
    visual_parts_added.iter().for_each(|p| added_items.put(p.part_hash, Part::Visual));
    visual_part_differences.removed.iter().for_each(|p| removed_items.put(p.part_hash, Part::Visual));
    let vinyls_added = &vinyl_differences.added;
    vinyls_added.iter().for_each(|p| added_items.put(p.hash, Part::Vinyl));
    vinyl_differences.removed.iter().for_each(|p| removed_items.put(p.hash, Part::Vinyl));

    let mut commerce_session_result_trans = CommerceSessionResultTrans::default();
    let mut add_cash = 0i32;
    let mut remove_cash = 0i32;
    let mut add_boost = 0i32;
    let mut remove_boost = 0i32;

    let mut inventory_entity = {
        let persona_entity = persona_entity.as_ref().ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
        w.ejb(|w| inventory::get_inventory(w, persona_entity))?
    };

    for (key, value) in added_items.entries() {
        if let Part::Vinyl = value {
            match vinyl_product::find_by_hash(w.db(), key)? {
                Some(vinyl_product_entity) => {
                    let currency = vinyl_product_entity
                        .currency
                        .ok_or_else(|| Fault::null_pointer("VinylProductEntity.currency"))?;
                    if currency == "CASH" {
                        remove_cash = remove_cash.wrapping_add(vinyl_product_entity.price as i32);
                    } else {
                        remove_boost = remove_boost.wrapping_add(vinyl_product_entity.price as i32);
                    }
                }
                None => {
                    commerce_session_result_trans.status = Some(CommerceResultStatus::FAIL_INVALID_BASKET);
                    return Ok(commerce_session_result_trans);
                }
            }
        } else {
            // Throws for an unknown hash, so upstream's null check after it never fires.
            let product_entity = product::find_by_hash(w.db(), key)?;
            let inventory_item_entity = inventory_item::find_by_inventory_id_and_entitlement_tag(
                w.db(),
                inventory_entity.id,
                product_entity.entitlement_tag.as_deref(),
            )?;
            let mut item_in_basket = false;
            for p in &basket_items {
                let id = p.product_id.as_deref().ok_or_else(|| Fault::null_pointer("BasketItemTrans.productId"))?;
                if id.eq_ignore_ascii_case(&product_entity.product_id) {
                    item_in_basket = true;
                    break;
                }
            }

            match inventory_item_entity {
                Some(inventory_item_entity) if !item_in_basket => {
                    w.ejb(|w| inventory::decrease_item_count(w, &mut inventory_entity, inventory_item_entity))?;
                }
                _ => {
                    if product_entity.currency == "CASH" {
                        remove_cash = remove_cash.wrapping_add(product_entity.price as i32);
                    } else {
                        remove_boost = remove_boost.wrapping_add(product_entity.price as i32);
                    }
                }
            }
        }
    }

    for (key, value) in removed_items.entries() {
        if !matches!(value, Part::Vinyl) {
            let product_entity = product::find_by_hash(w.db(), key)?;
            if product_entity.currency == "CASH" {
                add_cash = add_cash.wrapping_add(product_entity.resale_price as i32);
            } else {
                add_boost = add_boost.wrapping_add(product_entity.resale_price as i32);
            }
        }
    }

    let entitlements_to_sell = commerce_session_trans
        .entitlements_to_sell
        .ok_or_else(|| Fault::null_pointer("CommerceSessionTrans.entitlementsToSell"))?;
    if let Some(items) = entitlements_to_sell.items {
        for e in items.entitlement_item_trans {
            let Some(entitlement_id) = e.entitlement_id.as_deref() else { continue };
            if inventory_item::find_by_persona_id_and_entitlement_tag(w.db(), persona_id, entitlement_id)?.is_some() {
                let persona_entity = persona_entity.as_mut().ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
                w.ejb(|w| inventory::remove_item(w, persona_entity, entitlement_id, e.quantity))?;
            }
        }
    }

    let mut persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let final_cash = persona_entity.cash - f64::from(remove_cash) + f64::from(add_cash);
    let final_boost = persona_entity.boost - f64::from(remove_boost) + f64::from(add_boost);

    if final_cash < 0.0 || final_boost < 0.0 {
        commerce_session_result_trans.status = Some(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS);
        commerce_session_result_trans.updated_car = Some(persona_bo::get_default_car(w, persona_id)?);
        return Ok(commerce_session_result_trans);
    }

    let mut car_entity = car_entity.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    owned_car_converter::paints2_new_entity(&commerce_custom_car, &mut car_entity)?;
    owned_car_converter::vinyls2_new_entity(&commerce_custom_car, &mut car_entity)?;
    owned_car_converter::skill_mod_parts2_new_entity(&commerce_custom_car, &mut car_entity)?;
    owned_car_converter::performance_parts2_new_entity(&commerce_custom_car, &mut car_entity)?;
    owned_car_converter::visual_parts2_new_entity(&commerce_custom_car, &mut car_entity)?;

    performance::calc_new_car_class(w, &mut car_entity)?;

    car::update(w.db(), &car_entity)?;
    car::replace_parts(w.db(), &mut car_entity)?;
    persona_entity.boost = final_boost;
    w.ejb(|w| driver_persona::update_cash(w, &mut persona_entity, final_cash))?;

    commerce_session_result_trans.updated_car = Some(persona_bo::get_default_car(w, persona_id)?);
    commerce_session_result_trans.invalid_basket = Some(InvalidBasketTrans::default());
    commerce_session_result_trans.status = Some(CommerceResultStatus::SUCCESS);
    commerce_session_result_trans.inventory_items = Some(ArrayOfInventoryItemTrans::default());

    commerce_session_result_trans.wallets = Some(ArrayOfWalletTrans {
        wallet_trans: vec![
            WalletTrans { currency: Some("CASH".into()), balance: persona_entity.cash },
            WalletTrans { currency: Some("BOOST".into()), balance: persona_entity.boost },
        ],
    });

    let customization_context = if !paints_added.is_empty() {
        let mut ctx = AchievementCustomizationContext::new(Type::Paint);
        ctx.set_paints_added(paints_added.clone())?;
        Some(ctx)
    } else if !perf_parts_added.is_empty() {
        let mut ctx = AchievementCustomizationContext::new(Type::Perf);
        let mut wrapped = Vec::new();
        for p in perf_parts_added {
            let product_entity = product::find_by_hash(w.db(), p.performance_part_attrib_hash)?;
            wrapped.push(WrappedPart { part: p.clone(), product_entity });
        }
        ctx.set_performance_parts_added(wrapped)?;
        Some(ctx)
    } else if !skill_mods_added.is_empty() {
        let mut ctx = AchievementCustomizationContext::new(Type::Skills);
        let mut wrapped = Vec::new();
        for p in skill_mods_added {
            let product_entity = product::find_by_hash(w.db(), p.skill_mod_part_attrib_hash)?;
            wrapped.push(WrappedPart { part: p.clone(), product_entity });
        }
        ctx.set_skill_mod_parts_added(wrapped)?;
        Some(ctx)
    } else if !visual_parts_added.is_empty() {
        let mut ctx = AchievementCustomizationContext::new(Type::Aftermarket);
        ctx.set_visual_parts_added(visual_parts_added.clone())?;
        Some(ctx)
    } else if !vinyls_added.is_empty() {
        let mut ctx = AchievementCustomizationContext::new(Type::Vinyls);
        ctx.set_vinyls_added(vinyls_added.clone())?;
        Some(ctx)
    } else {
        None
    };

    if let Some(customization_context) = customization_context {
        let mut transaction = achievement::create_transaction(persona_entity.persona_id);
        transaction.add(
            "CUSTOMIZATION",
            [
                ("persona".to_owned(), Binding::Persona(persona_entity.clone())),
                ("ctx".to_owned(), Binding::CustomizationCtx(Box::new(customization_context))),
            ]
            .into(),
        )?;
        achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;
    }

    Ok(commerce_session_result_trans)
}

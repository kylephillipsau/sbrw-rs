//! `InventoryBO`: the items a persona owns and the slots they take. Ported so
//! far: everything but the job that deletes expired items.

use anyhow::anyhow;
use sbrw_java::lang::{round_float, split, trim};
use sbrw_xml::dto::http::{ArrayOfInventoryItemTrans, InventoryItemTrans, InventoryTrans};
use time::{Duration, PrimitiveDateTime};

use crate::bo::driver_persona;
use crate::dao::inventory::{self, InventoryEntity};
use crate::dao::inventory_item::{self, InventoryItemEntity};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::product::{self, ProductEntity};
use crate::engine::{EngineException, EngineExceptionCode};
use crate::{Fault, World, datetime};

// port: bo/InventoryBO#getInventory(PersonaEntity)
/// The persona's inventory, created if it has none.
pub fn get_inventory(w: &mut World, persona: &PersonaEntity) -> Result<InventoryEntity, Fault> {
    match inventory::find_by_persona_id(w.db(), persona.persona_id)? {
        Some(inventory) => Ok(inventory),
        None => create_inventory(w, persona),
    }
}

// port: bo/InventoryBO#getClientInventory(InventoryEntity)
pub fn get_client_inventory(w: &World, inventory: &InventoryEntity) -> Result<InventoryTrans, Fault> {
    let mut items = ArrayOfInventoryItemTrans::default();
    for item in &inventory.inventory_items {
        items.inventory_item_trans.push(convert_item_to_item_trans(w, inventory, item)?);
    }
    Ok(InventoryTrans {
        performance_parts_capacity: inventory.performance_parts_capacity.into(),
        performance_parts_used_slot_count: inventory.performance_parts_used_slot_count.into(),
        visual_parts_capacity: inventory.visual_parts_capacity.into(),
        visual_parts_used_slot_count: inventory.visual_parts_used_slot_count.into(),
        skill_mod_parts_capacity: inventory.skill_mod_parts_capacity.into(),
        skill_mod_parts_used_slot_count: inventory.skill_mod_parts_used_slot_count.into(),
        inventory_items: Some(items),
    })
}

// port: bo/InventoryBO#convertItemToItemTrans(InventoryItemEntity)
/// An item as the game sees it. `InventoryId` is the inventory's id, not the item's.
pub fn convert_item_to_item_trans(
    w: &World,
    inventory: &InventoryEntity,
    item: &InventoryItemEntity,
) -> Result<InventoryItemTrans, Fault> {
    // The item's EAGER product, joined on its product id.
    let product = product::find_by_product_id(w.db(), &item.product_id)?;
    let hash = product.hash.ok_or_else(|| Fault::null_pointer("ProductEntity.hash"))?;
    Ok(InventoryItemTrans {
        entitlement_tag: product.entitlement_tag.clone(),
        expiration_date: item.expiration_date.map(|t| {
            t.format(time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]"))
                .expect("a datetime formats")
        }),
        hash,
        inventory_id: inventory.id,
        product_id: Some(product.product_id.clone()),
        remaining_use_count: item.remaining_use_count.into(),
        resell_price: f64::from(item.resell_price),
        status: item.status.clone(),
        string_hash: Some(format!("0x{hash:08X}")),
        virtual_item_type: Some(product.product_type.to_lowercase()),
    })
}

// port: bo/InventoryBO#canInventoryHold(InventoryEntity,ProductEntity)
/// Whether one more item of the product's type fits; a capacity of 0 means
/// unlimited, and other types always fit.
pub fn can_inventory_hold(inventory: &InventoryEntity, product: &ProductEntity) -> bool {
    let (capacity, quantity) = match product.product_type.to_uppercase().as_str() {
        "PERFORMANCEPART" => (inventory.performance_parts_capacity, inventory.performance_parts_used_slot_count),
        "SKILLMODPART" => (inventory.skill_mod_parts_capacity, inventory.skill_mod_parts_used_slot_count),
        "VISUALPART" => (inventory.visual_parts_capacity, inventory.visual_parts_used_slot_count),
        _ => (0, 0),
    };
    capacity == 0 || quantity.wrapping_add(1) <= capacity
}

// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String)
// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String,boolean)
// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String,int)
// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String,int,boolean)
// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String,int,LocalDateTime)
// port: bo/InventoryBO#addInventoryItem(InventoryEntity,String,int,LocalDateTime,boolean)
/// Add an item of a product to the inventory. Upstream's overloads default
/// `quantity` to -1 (the product's use count), `expiration_date` to none (the
/// product's duration, if it has one) and `ignore_limits` to false.
pub fn add_inventory_item(
    w: &mut World,
    inventory: &mut InventoryEntity,
    product_id: &str,
    quantity: i32,
    expiration_date: Option<PrimitiveDateTime>,
    ignore_limits: bool,
) -> Result<InventoryItemEntity, Fault> {
    // Throws for an unknown product, so upstream's null check after it never fires.
    let product = product::find_by_product_id(w.db(), product_id)?;
    if quantity < -1 {
        return Err(Fault::Engine(EngineException::with_message(
            format!("Invalid product quantity: {quantity}"),
            EngineExceptionCode::EntitlementInvalidCount,
            true,
        )));
    }
    if !can_inventory_hold(inventory, &product) && !ignore_limits {
        return Err(Fault::Engine(EngineException::with_message(
            format!("Cannot add item to inventory. ID={product_id} IID={}", inventory.id),
            EngineExceptionCode::NotEnoughSpace,
            true,
        )));
    }

    let expiration_date = match expiration_date {
        None if product.duration_minute != 0 => {
            Some(datetime::now(&w.env) + Duration::minutes(i64::from(product.duration_minute)))
        }
        given => given,
    };
    let quantity = if quantity == -1 { product.use_count } else { quantity };

    let resale_multiplier = w.params.get_float_param_or("INVENTORY_ITEM_RESALE_MULTIPLIER", 1.0)?;
    let mut item = InventoryItemEntity {
        id: 0,
        inventory_id: inventory.id,
        expiration_date,
        product_id: product.product_id.clone(),
        remaining_use_count: quantity,
        resell_price: round_float(product.resale_price * resale_multiplier),
        status: Some("ACTIVE".into()),
    };

    update_inventory_slots(w, inventory, &product, true)?;
    inventory_item::insert(w.db(), &mut item)?;
    inventory.inventory_items.push(item.clone());
    Ok(item)
}

// port: bo/InventoryBO#addStackedInventoryItem(InventoryEntity,String,int)
// port: bo/InventoryBO#addStackedInventoryItem(InventoryEntity,String,int,boolean)
/// Add `quantity` uses of a product (-1: its use count) to the inventory's
/// stack of it, or start a stack; upstream's shorter overload doesn't
/// `ignore_limits`.
pub fn add_stacked_inventory_item(
    w: &mut World,
    inventory: &mut InventoryEntity,
    product_id: &str,
    quantity: i32,
    ignore_limits: bool,
) -> Result<InventoryItemEntity, Fault> {
    // Throws for an unknown product, so upstream's null check after it never fires.
    let product = product::find_by_product_id(w.db(), product_id)?;
    if quantity < -1 {
        return Err(Fault::Engine(EngineException::with_message(
            format!("Invalid product quantity for addStackedInventoryItem: {quantity}"),
            EngineExceptionCode::EntitlementInvalidCount,
            true,
        )));
    }
    let quantity = if quantity == -1 { product.use_count } else { quantity };
    let existing_item = inventory_item::find_by_inventory_id_and_entitlement_tag(
        w.db(),
        inventory.id,
        product.entitlement_tag.as_deref(),
    )?;
    match existing_item {
        Some(mut existing_item) => {
            existing_item.remaining_use_count = existing_item.remaining_use_count.wrapping_add(quantity);
            inventory_item::update(w.db(), &existing_item)?;
            // The inventory's collection holds the same managed item.
            if let Some(held) = inventory.inventory_items.iter_mut().find(|i| i.id == existing_item.id) {
                held.remaining_use_count = existing_item.remaining_use_count;
            }
            Ok(existing_item)
        }
        None => add_inventory_item(w, inventory, product_id, quantity, None, ignore_limits),
    }
}

// port: bo/InventoryBO#decreaseItemCount(InventoryEntity,Integer)
/// Use one of the stack of the product with this hash. An unknown hash throws.
pub fn decrease_item_count_by_hash(
    w: &mut World,
    inventory: &mut InventoryEntity,
    hash: i32,
) -> Result<InventoryItemEntity, Fault> {
    let entitlement_tag = product::find_by_hash(w.db(), hash)?.entitlement_tag;
    decrease_item_count_by_tag(w, inventory, entitlement_tag.as_deref())
}

// port: bo/InventoryBO#decreaseItemCount(InventoryEntity,String)
/// Use one of the stack with this entitlement tag, which must exist.
pub fn decrease_item_count_by_tag(
    w: &mut World,
    inventory: &mut InventoryEntity,
    entitlement_tag: Option<&str>,
) -> Result<InventoryItemEntity, Fault> {
    let Some(existing_item) =
        inventory_item::find_by_inventory_id_and_entitlement_tag(w.db(), inventory.id, entitlement_tag)?
    else {
        return Err(Fault::Engine(EngineException::with_message(
            format!("Could not find entitlement '{}' in IID {}", entitlement_tag.unwrap_or("null"), inventory.id),
            EngineExceptionCode::NoSuchEntitlementExists,
            true,
        )));
    };
    decrease_item_count(w, inventory, existing_item)
}

// port: bo/InventoryBO#decreaseItemCount(InventoryEntity,InventoryItemEntity)
/// Use one of the item's stack; the last use removes the item and frees its slot.
pub fn decrease_item_count(
    w: &mut World,
    inventory: &mut InventoryEntity,
    mut existing_item: InventoryItemEntity,
) -> Result<InventoryItemEntity, Fault> {
    existing_item.remaining_use_count = existing_item.remaining_use_count.wrapping_sub(1);
    if existing_item.remaining_use_count <= 0 {
        let product = product::find_by_product_id(w.db(), &existing_item.product_id)?;
        update_inventory_slots(w, inventory, &product, false)?;
        inventory.inventory_items.retain(|i| i.id != existing_item.id);
        inventory_item::delete(w.db(), &existing_item)?;
        inventory::update(w.db(), inventory)?;
    } else {
        // The item is managed: Hibernate flushes the new count.
        inventory_item::update(w.db(), &existing_item)?;
        if let Some(held) = inventory.inventory_items.iter_mut().find(|i| i.id == existing_item.id) {
            held.remaining_use_count = existing_item.remaining_use_count;
        }
    }
    Ok(existing_item)
}

// port: bo/InventoryBO#removeItem(Long,String)
// port: bo/InventoryBO#removeItem(Long,String,Integer)
/// Remove `quantity` uses (-1: the whole stack) of the first item with this
/// entitlement tag from the persona's inventory.
pub fn remove_item_of(w: &mut World, persona_id: i64, entitlement_tag: &str, quantity: i32) -> Result<(), Fault> {
    let mut persona_entity = persona::find(w.db(), persona_id)?;
    let inventory_entity = inventory::find_by_persona_id(w.db(), persona_id)?;
    remove(w, persona_entity.as_mut(), inventory_entity, entitlement_tag, quantity)
}

// port: bo/InventoryBO#removeItem(PersonaEntity,String,Integer)
pub fn remove_item(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    entitlement_tag: &str,
    quantity: i32,
) -> Result<(), Fault> {
    let inventory_entity = inventory::find_by_persona_id(w.db(), persona_entity.persona_id)?;
    remove(w, Some(persona_entity), inventory_entity, entitlement_tag, quantity)
}

// port: bo/InventoryBO#removeItem(PersonaEntity,InventoryEntity,String)
// port: bo/InventoryBO#removeItem(PersonaEntity,InventoryEntity,String,Integer)
/// The whole stack (`quantity` -1 or the stack's size) goes, and its resell
/// price is paid; otherwise `quantity` uses come off it, for nothing.
fn remove(
    w: &mut World,
    persona_entity: Option<&mut PersonaEntity>,
    inventory_entity: Option<InventoryEntity>,
    entitlement_tag: &str,
    quantity: i32,
) -> Result<(), Fault> {
    let mut inventory_entity = inventory_entity.ok_or_else(|| Fault::null_pointer("InventoryEntity"))?;
    let Some(mut inventory_item_entity) =
        inventory_item::find_by_inventory_id_and_entitlement_tag(w.db(), inventory_entity.id, Some(entitlement_tag))?
    else {
        return Err(Fault::Engine(EngineException::with_message(
            format!(
                "An item stack with the entitlement tag '{entitlement_tag}' could not be found. IID: {}",
                inventory_entity.id
            ),
            EngineExceptionCode::EntitlementNoSuchGroup,
            true,
        )));
    };

    if quantity == -1 || quantity == inventory_item_entity.remaining_use_count {
        let product = product::find_by_product_id(w.db(), &inventory_item_entity.product_id)?;
        update_inventory_slots(w, &mut inventory_entity, &product, false)?;
        inventory_entity.inventory_items.retain(|i| i.id != inventory_item_entity.id);
        inventory_item::delete(w.db(), &inventory_item_entity)?;
        inventory::update(w.db(), &inventory_entity)?;
        let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
        let cash = persona_entity.cash + f64::from(inventory_item_entity.resell_price);
        driver_persona::update_cash(w, persona_entity, cash)?;
    } else {
        if quantity < 1 {
            return Err(Fault::Engine(EngineException::with_message(
                format!("An invalid removal operation was requested. Cannot remove {quantity} items from a stack."),
                EngineExceptionCode::EntitlementInvalidCount,
                true,
            )));
        }
        if quantity > inventory_item_entity.remaining_use_count {
            return Err(Fault::Engine(EngineException::with_message(
                format!(
                    "An invalid removal operation was requested. Cannot remove {quantity} items from the stack, as there are only {}",
                    inventory_item_entity.remaining_use_count
                ),
                EngineExceptionCode::EntitlementInvalidCount,
                true,
            )));
        }
        inventory_item_entity.remaining_use_count -= quantity;
        inventory_item::update(w.db(), &inventory_item_entity)?;
    }
    Ok(())
}

// port: bo/InventoryBO#createInventory(PersonaEntity)
pub fn create_inventory(w: &mut World, persona: &PersonaEntity) -> Result<InventoryEntity, Fault> {
    let mut inventory = InventoryEntity {
        persona_id: Some(persona.persona_id),
        performance_parts_capacity: w.params.get_int_param_or("STARTING_INVENTORY_PERF_SLOTS", 100)?,
        skill_mod_parts_capacity: w.params.get_int_param_or("STARTING_INVENTORY_SKILL_SLOTS", 100)?,
        visual_parts_capacity: w.params.get_int_param_or("STARTING_INVENTORY_VISUAL_SLOTS", 100)?,
        ..Default::default()
    };
    inventory::insert(w.db(), &mut inventory)?;
    add_default_inventory_items(w, &mut inventory)?;
    Ok(inventory)
}

// port: bo/InventoryBO#updateInventorySlots(InventoryEntity,ProductEntity,boolean)
/// Count an item of the product's type in or out of its slots. Unlike
/// `canInventoryHold`, the type must match in upper case.
fn update_inventory_slots(
    w: &World,
    inventory: &mut InventoryEntity,
    product: &ProductEntity,
    is_item_added: bool,
) -> Result<(), Fault> {
    let step = if is_item_added { 1 } else { -1 };
    let count = match product.product_type.as_str() {
        "PERFORMANCEPART" => &mut inventory.performance_parts_used_slot_count,
        "SKILLMODPART" => &mut inventory.skill_mod_parts_used_slot_count,
        "VISUALPART" => &mut inventory.visual_parts_used_slot_count,
        _ => return Ok(()),
    };
    *count = count.wrapping_add(step);
    // Upstream's inventory is managed, so Hibernate writes the new count.
    inventory::update(w.db(), inventory)?;
    Ok(())
}

// port: bo/InventoryBO#addDefaultInventoryItems(InventoryEntity)
/// The items in `STARTING_INVENTORY_ITEMS`: `productId|quantity`, separated by `;`.
fn add_default_inventory_items(w: &mut World, inventory: &mut InventoryEntity) -> Result<(), Fault> {
    let Some(item_desc) = w.params.get_str_param("STARTING_INVENTORY_ITEMS").filter(|d| !d.is_empty()) else {
        return Ok(());
    };
    for item_info in split(trim(&item_desc), ';') {
        let parts = split(&item_info, '|');
        if parts.len() != 2 {
            return Err(Fault::Engine(EngineException::with_message(
                "Failed to parse default inventory items",
                EngineExceptionCode::UnspecifiedError,
                true,
            )));
        }
        let quantity: i32 = parts[1].parse().map_err(|_| anyhow!("For input string: \"{}\"", parts[1]))?;
        add_inventory_item(w, inventory, &parts[0], quantity, None, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::persona;
    use crate::dao::user::{self, UserEntity};

    fn world_with_persona(parameters: &[(&str, &str)]) -> (World, PersonaEntity) {
        let w = World::test(parameters);
        let mut u = UserEntity::new();
        user::insert(w.db(), &mut u).unwrap();
        let mut p = PersonaEntity { user_id: Some(u.id), name: Some("P".into()), ..Default::default() };
        persona::insert(w.db(), &mut p).unwrap();
        (w, p)
    }

    fn items(w: &World) -> Vec<(String, i32, Option<String>, i32)> {
        let mut stmt = w
            .db()
            .prepare("SELECT productId, remainingUseCount, expirationDate, resellPrice FROM inventory_item ORDER BY id")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().map(Result::unwrap).collect()
    }

    #[test]
    fn a_new_inventory_gets_the_starting_items() {
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", " SRV-POWERUP0|15;srv-amp-insurance|1; ")]);
        let inventory = create_inventory(&mut w, &p).unwrap();
        assert_eq!(inventory.persona_id, Some(p.persona_id));
        assert_eq!(
            (inventory.performance_parts_capacity, inventory.skill_mod_parts_capacity, inventory.visual_parts_capacity),
            (100, 150, 200)
        );
        assert_eq!(inventory.inventory_items.len(), 2);
        assert_eq!(
            items(&w),
            [
                ("SRV-POWERUP0".to_owned(), 15, None, 0),
                // The product's own id, and its 7-day duration from the clock.
                ("SRV-AMP-INSURANCE".to_owned(), 1, Some("2026-01-08 12:00:00".to_owned()), 0),
            ]
        );
    }

    #[test]
    fn a_malformed_item_list_is_an_engine_exception() {
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", "SRV-POWERUP0")]);
        let Err(Fault::Engine(e)) = create_inventory(&mut w, &p) else { panic!("expected an EngineException") };
        assert_eq!(e.code, EngineExceptionCode::UnspecifiedError);
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", "SRV-POWERUP0|x")]);
        assert!(matches!(create_inventory(&mut w, &p), Err(Fault::Internal(_))));
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", "NOPE|1")]);
        assert!(matches!(create_inventory(&mut w, &p), Err(Fault::Internal(_))));
    }

    #[test]
    fn no_starting_items_when_the_parameter_is_empty() {
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", "")]);
        assert!(create_inventory(&mut w, &p).unwrap().inventory_items.is_empty());
    }

    #[test]
    fn slot_types_are_checked_and_counted() {
        let (mut w, p) = world_with_persona(&[("STARTING_INVENTORY_ITEMS", "")]);
        let mut inventory = create_inventory(&mut w, &p).unwrap();
        let part: String = w
            .db()
            .query_row("SELECT productId FROM product WHERE productType = 'PERFORMANCEPART' LIMIT 1", [], |r| r.get(0))
            .unwrap();
        inventory.performance_parts_capacity = 1;
        add_inventory_item(&mut w, &mut inventory, &part, -1, None, false).unwrap();
        assert_eq!(inventory.performance_parts_used_slot_count, 1);
        let stored: i32 = w
            .db()
            .query_row("SELECT performancePartsUsedSlotCount FROM inventory WHERE id = ?1", [inventory.id], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, 1);
        let Err(Fault::Engine(full)) = add_inventory_item(&mut w, &mut inventory, &part, -1, None, false) else {
            panic!("expected NotEnoughSpace")
        };
        assert_eq!(full.code, EngineExceptionCode::NotEnoughSpace);
        add_inventory_item(&mut w, &mut inventory, &part, -1, None, true).unwrap();
        assert_eq!(inventory.performance_parts_used_slot_count, 2);
        let Err(Fault::Engine(bad)) = add_inventory_item(&mut w, &mut inventory, &part, -2, None, true) else {
            panic!("expected EntitlementInvalidCount")
        };
        assert_eq!(bad.code, EngineExceptionCode::EntitlementInvalidCount);
    }
}

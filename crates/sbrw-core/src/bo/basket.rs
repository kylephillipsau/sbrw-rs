//! `BasketBO`: buying from the shops and selling cars back. Ported so far:
//! everything but card packs.

use sbrw_xml::dto::http::{ArrayOfOwnedCarTrans, CommerceResultStatus, CommerceResultTrans, OwnedCarTrans};
use time::Duration;

use crate::bo::token_session::{self, TokenSession};
use crate::bo::util::achievement_commerce_context::{AchievementCommerceContext, CommerceType};
use crate::bo::util::owned_car_converter;
use crate::bo::{
    achievement, achievement_transaction::Binding, car_damage, car_slot, driver_persona, inventory, performance,
    persona as persona_bo,
};
use crate::dao::car::{self, CarEntity};
use crate::dao::inventory as inventory_dao;
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::product::{self, ProductEntity};
use crate::dao::user::{self, UserEntity};
use crate::dao::{amplifier, basket_definition, car_classes, inventory_item, treasure_hunt};
use crate::engine::{EngineException, EngineExceptionCode};
use crate::{Fault, World, datetime};

// port: bo/BasketBO#findProduct(String)
/// A missing product throws.
pub fn find_product(w: &World, product_id: &str) -> Result<ProductEntity, Fault> {
    product::find_by_product_id(w.db(), product_id)
}

// port: bo/BasketBO#repairCar(String,PersonaEntity)
/// Repair the selected car to full durability, at the product's price for
/// each point missing (truncated to an `int`).
pub fn repair_car(
    w: &mut World,
    product_id: &str,
    persona_entity: &mut PersonaEntity,
) -> Result<CommerceResultStatus, Fault> {
    let mut default_car_entity = persona_bo::get_default_car_entity(w, persona_entity.persona_id)?
        .ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    let price = (product::find_by_product_id(w.db(), product_id)?.price
        * 100i32.wrapping_sub(default_car_entity.durability) as f32) as i32;
    // Throws for an unknown product, so upstream's null check after it never fires.
    let repair_product = product::find_by_product_id(w.db(), product_id)?;

    if can_purchase_product(w, persona_entity, &repair_product, price as f32)? {
        persona::update(w.db(), persona_entity)?;
        car_damage::update_durability(w, &mut default_car_entity, 100)?;
        car::update(w.db(), &default_car_entity)?;
        perform_persona_transaction(w, persona_entity, &repair_product, price as f32)?;
        return Ok(CommerceResultStatus::SUCCESS);
    }

    Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS)
}

// port: bo/BasketBO#buyPowerups(String,PersonaEntity)
/// Add a powerup product's uses to the persona's stack of it, if the economy
/// is on and the persona can pay.
pub fn buy_powerups(
    w: &mut World,
    product_id: &str,
    persona_entity: &mut PersonaEntity,
) -> Result<CommerceResultStatus, Fault> {
    if !w.params.get_bool_param("ENABLE_ECONOMY") {
        return Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS);
    }
    // Throws for an unknown product, so upstream's null check after it never fires.
    let powerup_product = product::find_by_product_id(w.db(), product_id)?;

    if can_purchase_product(w, persona_entity, &powerup_product, -1.0)? {
        let mut inventory_entity = inventory_dao::find_by_persona_id(w.db(), persona_entity.persona_id)?
            .ok_or_else(|| Fault::null_pointer("InventoryEntity"))?;
        inventory::add_stacked_inventory_item(w, &mut inventory_entity, product_id, powerup_product.use_count, false)?;
        inventory_dao::update(w.db(), &inventory_entity)?;
        perform_persona_transaction(w, persona_entity, &powerup_product, -1.0)?;
        return Ok(CommerceResultStatus::SUCCESS);
    }

    Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS)
}

// port: bo/BasketBO#reviveTreasureHunt(String,PersonaEntity)
/// Mend a broken treasure-hunt streak.
pub fn revive_treasure_hunt(
    w: &mut World,
    product_id: &str,
    persona_entity: &mut PersonaEntity,
) -> Result<CommerceResultStatus, Fault> {
    let product_entity = product::find_by_product_id(w.db(), product_id)?;

    if can_purchase_product(w, persona_entity, &product_entity, -1.0)? {
        let mut treasure_hunt_entity = treasure_hunt::find(w.db(), persona_entity.persona_id)?
            .ok_or_else(|| Fault::null_pointer("TreasureHuntEntity"))?;
        treasure_hunt_entity.is_streak_broken = Some(false);
        perform_persona_transaction(w, persona_entity, &product_entity, -1.0)?;

        treasure_hunt::update(w.db(), &treasure_hunt_entity)?;

        return Ok(CommerceResultStatus::SUCCESS);
    }

    Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS)
}

// port: bo/BasketBO#buyAmplifier(PersonaEntity,String)
/// An amplifier, if the persona has none of it yet and can pay.
pub fn buy_amplifier(
    w: &mut World,
    persona_entity: &mut PersonaEntity,
    product_id: &str,
) -> Result<CommerceResultStatus, Fault> {
    let product_entity = product::find_by_product_id(w.db(), product_id)?;

    if !can_add_amplifier(w, persona_entity.persona_id, product_entity.entitlement_tag.as_deref())? {
        return Ok(CommerceResultStatus::FAIL_MAX_ALLOWED_PURCHASES_FOR_THIS_PRODUCT);
    }

    if can_purchase_product(w, persona_entity, &product_entity, -1.0)? {
        add_amplifier(w, persona_entity, &product_entity)?;
        perform_persona_transaction(w, persona_entity, &product_entity, -1.0)?;
        return Ok(CommerceResultStatus::SUCCESS);
    }

    Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS)
}

// port: bo/BasketBO#buyCar(ProductEntity,PersonaEntity,TokenSessionEntity,CommerceResultTrans)
/// Buy a car product for the persona, if a slot is free and it can pay; the
/// new car is selected and goes into `commerce_result_trans`. An
/// `EngineException` on the way is the rental limit, and what was done
/// before it stands. `user` is the session's.
pub fn buy_car(
    w: &mut World,
    product_entity: &ProductEntity,
    persona_entity: &mut PersonaEntity,
    user: &UserEntity,
    commerce_result_trans: &mut CommerceResultTrans,
) -> Result<CommerceResultStatus, Fault> {
    if get_persona_car_count(w, persona_entity.persona_id)? >= w.params.get_car_limit(user)? {
        return Ok(CommerceResultStatus::FAIL_INSUFFICIENT_CAR_SLOTS);
    }

    if can_purchase_product(w, persona_entity, product_entity, -1.0)? {
        match buy(w, product_entity, persona_entity, commerce_result_trans) {
            Err(Fault::Engine(_)) => return Ok(CommerceResultStatus::FAIL_MAX_STACK_OR_RENTAL_LIMIT),
            other => other?,
        }
        return Ok(CommerceResultStatus::SUCCESS);
    }

    Ok(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS)
}

/// `buyCar`'s `try` block.
fn buy(
    w: &mut World,
    product_entity: &ProductEntity,
    persona_entity: &mut PersonaEntity,
    commerce_result_trans: &mut CommerceResultTrans,
) -> Result<(), Fault> {
    let car_entity = add_car(w, product_entity, persona_entity)?;
    persona_entity.cur_car_index = car::find_num_by_persona_id(w.db(), persona_entity.persona_id)?.wrapping_sub(1);
    persona::update(w.db(), persona_entity)?;

    let owned_car_trans = owned_car_converter::entity2_trans(&car_entity);
    commerce_result_trans.purchased_cars = Some(ArrayOfOwnedCarTrans { owned_car_trans: vec![owned_car_trans] });

    perform_persona_transaction(w, persona_entity, product_entity, -1.0)
}

// port: bo/BasketBO#addCar(ProductEntity,PersonaEntity)
/// A new car for the persona from the product's basket definition, rated,
/// with insurance for a rental if the persona has none, and the COMMERCE
/// achievements for owning it. A rental needs a car of the persona's own.
pub fn add_car(
    w: &mut World,
    product_entity: &ProductEntity,
    persona_entity: &mut PersonaEntity,
) -> Result<CarEntity, Fault> {
    let is_rental = product_entity.duration_minute > 0;
    if is_rental && car::find_num_non_rentals_by_persona_id(w.db(), persona_entity.persona_id)? == 0 {
        return Err(Fault::Engine(EngineException::with_message(
            format!("Persona {} has no non-rental cars", persona_entity.name.as_deref().unwrap_or("null")),
            EngineExceptionCode::MissingRequiredEntitlements,
            true,
        )));
    }

    let owned_car_trans = get_car(w, product_entity)?;

    let mut car_entity = CarEntity { persona_id: persona_entity.persona_id, ..Default::default() };
    owned_car_converter::trans2_entity(&owned_car_trans, &mut car_entity)?;
    owned_car_converter::details2_new_entity(&owned_car_trans, &mut car_entity)?;

    if is_rental {
        car_entity.expiration_date =
            Some(datetime::now(&w.env) + Duration::minutes(i64::from(product_entity.duration_minute)));
        car_entity.ownership_type = Some("RentalCar".into());
    }

    car::insert(w.db(), &mut car_entity)?;
    performance::calc_new_car_class(w, &mut car_entity)?;
    // The car is managed: Hibernate flushes the new class and rating.
    car::update(w.db(), &car_entity)?;

    if is_rental && can_add_amplifier(w, persona_entity.persona_id, Some("INSURANCE_AMPLIFIER"))? {
        let insurance = product::find_by_entitlement_tag(w.db(), "INSURANCE_AMPLIFIER")?;
        add_amplifier(w, persona_entity, &insurance)?;
    }

    let car_classes_entity =
        car_entity.name.as_deref().map(|name| car_classes::find(w.db(), name)).transpose()?.flatten();

    let mut transaction = achievement::create_transaction(persona_entity.persona_id);
    if let Some(car_classes_entity) = car_classes_entity {
        let mut commerce_context = AchievementCommerceContext::new(CommerceType::CarPurchase);
        commerce_context.set_car_classes_entity(car_classes_entity);
        commerce_context.set_product_entity(product_entity.clone());
        transaction.add(
            "COMMERCE",
            [
                ("persona".to_owned(), Binding::Persona(persona_entity.clone())),
                ("ownedCar".to_owned(), Binding::OwnedCar(Box::new(car_entity.clone()))),
                ("commerceCtx".to_owned(), Binding::CommerceCtx(Box::new(commerce_context))),
            ]
            .into(),
        )?;
        achievement::commit_transaction(w, persona_entity, &mut transaction)?;
    }

    Ok(car_entity)
}

// port: bo/BasketBO#sellCar(TokenSessionEntity,Long,Long)
/// Sell one of the persona's cars for its resale price. Never a rental, and
/// never the persona's last car of its own (see `remove_car`).
pub fn sell_car(
    w: &mut World,
    session: &TokenSession,
    persona_id: i64,
    serial_number: Option<i64>,
) -> Result<bool, Fault> {
    token_session::verify_persona_ownership(session, persona_id)?;

    // `find(null)`: Hibernate refuses a null id.
    let serial_number = serial_number.ok_or_else(|| anyhow::anyhow!("id to load is required for loading"))?;
    let Some(car_entity) = car::find(w.db(), serial_number)? else { return Ok(false) };

    if car_entity.ownership_type.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("RentalCar")) {
        return Ok(false);
    }

    let mut persona_entity = persona::find(w.db(), persona_id)?;

    if !remove_car(w, persona_entity.as_mut(), serial_number)? {
        return Ok(false);
    }

    let persona_entity = persona_entity.as_mut().ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let cash_total = persona_entity.cash + f64::from(car_entity.resale_price);
    driver_persona::update_cash(w, persona_entity, cash_total)?;

    Ok(true)
}

// port: bo/BasketBO#removeCar(PersonaEntity,Long)
/// Remove a car the persona owns, unless it is its only car that isn't a
/// rental (rentals are kept apart: one needs a car of one's own). The
/// selected index moves down one, or to the last car from the first.
pub fn remove_car(
    w: &mut World,
    persona_entity: Option<&mut PersonaEntity>,
    serial_number: i64,
) -> Result<bool, Fault> {
    let Some(car_entity) = car::find(w.db(), serial_number)? else { return Ok(false) };
    let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    if car_entity.persona_id != persona_entity.persona_id {
        return Err(Fault::engine(EngineExceptionCode::CarNotOwnedByDriver, false));
    }
    let non_rental_car_count = car::find_num_non_rentals_by_persona_id(w.db(), persona_entity.persona_id)?;

    if !car_entity.ownership_type.as_deref().is_some_and(|t| t.eq_ignore_ascii_case("RentalCar")) {
        if non_rental_car_count <= 1 {
            return Ok(false);
        }
    } else if non_rental_car_count == 0 {
        return Ok(false);
    }

    car::delete(w.db(), &car_entity)?;

    let cur_car_index = persona_entity.cur_car_index;

    if cur_car_index > 0 {
        persona_entity.cur_car_index = cur_car_index - 1;
    } else {
        persona_entity.cur_car_index = car::find_num_by_persona_id(w.db(), persona_entity.persona_id)?.wrapping_sub(1);
    }

    persona::update(w.db(), persona_entity)?;

    Ok(true)
}

// port: bo/BasketBO#canAddAmplifier(Long,String)
/// Whether the persona has no item with this entitlement tag; a null tag
/// matches no item.
fn can_add_amplifier(w: &World, persona_id: i64, entitlement_tag: Option<&str>) -> Result<bool, Fault> {
    let Some(entitlement_tag) = entitlement_tag else { return Ok(true) };
    Ok(inventory_item::find_all_by_persona_id_and_entitlement_tag(w.db(), persona_id, entitlement_tag)?.is_empty())
}

// port: bo/BasketBO#addAmplifier(PersonaEntity,ProductEntity)
/// Give the persona the amplifier; insurance repairs every car.
fn add_amplifier(w: &mut World, persona_entity: &PersonaEntity, product_entity: &ProductEntity) -> Result<(), Fault> {
    let mut inventory_entity = inventory::get_inventory(w, persona_entity)?;
    inventory::add_inventory_item(w, &mut inventory_entity, &product_entity.product_id, -1, None, false)?;

    let amplifier_entity = amplifier::find_amplifier_by_hash(w.db(), product_entity.hash)?;
    let amp_type = amplifier_entity.amp_type.ok_or_else(|| Fault::null_pointer("AmplifierEntity.ampType"))?;
    if amp_type == "INSURANCE" {
        persona_bo::repair_all_cars(w, persona_entity)?;
    }
    Ok(())
}

// port: bo/BasketBO#getCar(ProductEntity)
/// The product's car from its basket definition, made new: full heat and
/// durability, the product's resale price, and rental ownership for a rental.
fn get_car(w: &World, product_entity: &ProductEntity) -> Result<OwnedCarTrans, Fault> {
    let product_id = &product_entity.product_id;
    let Some(basket_definition_entity) = basket_definition::find(w.db(), product_id)? else {
        return Err(anyhow::anyhow!("IllegalArgumentException: No basket definition for {product_id}").into());
    };
    let xml = basket_definition_entity
        .owned_car_trans
        .ok_or_else(|| Fault::null_pointer("BasketDefinitionEntity.ownedCarTrans"))?;
    let mut owned_car_trans: OwnedCarTrans = sbrw_xml::from_xml(&xml)
        .map_err(|e| anyhow::anyhow!("Failed to unmarshal stream to OwnedCarTrans instance: {e}"))?;

    if product_entity.duration_minute != 0 {
        owned_car_trans.ownership_type = Some("RentalCar".into());
    }
    owned_car_trans.heat = 1.0;
    owned_car_trans.durability = 100;
    owned_car_trans.custom_car.as_mut().ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))?.resale_price =
        product_entity.resale_price;
    Ok(owned_car_trans)
}

// port: bo/BasketBO#getPersonaCarCount(Long)
fn get_persona_car_count(w: &World, persona_id: i64) -> Result<i32, Fault> {
    car_slot::count_personas_car(w, persona_id)
}

// port: bo/BasketBO#canPurchaseProduct(PersonaEntity,ProductEntity)
// port: bo/BasketBO#canPurchaseProduct(PersonaEntity,ProductEntity,float)
/// Whether the product is on sale to the persona (premium ones only to
/// premium users) and it can pay `value_override`, or the price when that is -1.
fn can_purchase_product(
    w: &World,
    persona_entity: &PersonaEntity,
    product_entity: &ProductEntity,
    value_override: f32,
) -> Result<bool, Fault> {
    if !product_entity.enabled {
        return Ok(false);
    }
    if product_entity.premium {
        let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
        let user = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
        if !user.premium {
            return Ok(false);
        }
    }
    let price = if value_override == -1.0 { product_entity.price } else { value_override };
    match product_entity.currency.as_str() {
        "CASH" => Ok(persona_entity.cash >= f64::from(price)),
        "_NS" => Ok(persona_entity.boost >= f64::from(price)),
        other => Err(invalid_currency(other)),
    }
}

// port: bo/BasketBO#performPersonaTransaction(PersonaEntity,ProductEntity)
// port: bo/BasketBO#performPersonaTransaction(PersonaEntity,ProductEntity,float)
/// Take the price (or `value_override`, unless it is -1) from the product's currency.
fn perform_persona_transaction(
    w: &World,
    persona_entity: &mut PersonaEntity,
    product_entity: &ProductEntity,
    value_override: f32,
) -> Result<(), Fault> {
    let price = if value_override == -1.0 { product_entity.price } else { value_override };
    match product_entity.currency.as_str() {
        "CASH" => persona_entity.cash -= f64::from(price),
        "_NS" => persona_entity.boost -= f64::from(price),
        other => return Err(invalid_currency(other)),
    }
    persona::update(w.db(), persona_entity)?;
    Ok(())
}

fn invalid_currency(currency: &str) -> Fault {
    Fault::Engine(EngineException::with_message(
        format!("Invalid currency in product entry: {currency}"),
        EngineExceptionCode::UnspecifiedError,
        true,
    ))
}

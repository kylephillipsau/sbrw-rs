//! `api/Personas`: a driver's garage and inventory. As upstream, each call
//! into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::util::owned_car_converter;
use sbrw_core::bo::{basket, car_slot, inventory, persona, token_session};
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{
    ArrayOfCommerceItemTrans, ArrayOfInventoryItemTrans, ArrayOfOwnedCarTrans, ArrayOfProductTrans, ArrayOfWalletTrans,
    BasketTrans, CarSlotInfoTrans, CommerceResultStatus, CommerceResultTrans, InvalidBasketTrans, InventoryItemTrans,
    ProductTrans, WalletTrans,
};

use super::{Call, entity};
use crate::AppState;

/// A `{personaId}` or `{carId}` path parameter; the route always has one.
fn path_id(call: &Call, name: &str) -> Result<i64, Fault> {
    call.path_long(name)?.ok_or_else(|| Fault::null_pointer(name))
}

// port: api/Personas#baskets(InputStream,Long)
/// Buying the first item of a basket. Ported so far: cars. Every answer has
/// one empty inventory item and the persona's wallets as they end up.
pub(super) async fn baskets(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    let session = call.session();
    token_session::verify_persona_ownership(session, persona_id)?;

    let world = state.world();
    let persona_entity = world.call(move |w| persona::get_persona_by_id(w, persona_id)).await?;

    let mut commerce_result_trans = CommerceResultTrans {
        commerce_items: Some(ArrayOfCommerceItemTrans::default()),
        invalid_basket: Some(InvalidBasketTrans::default()),
        inventory_items: Some(ArrayOfInventoryItemTrans { inventory_item_trans: vec![InventoryItemTrans::default()] }),
        ..Default::default()
    };

    let basket_trans: BasketTrans = call.unmarshal()?;
    let items = basket_trans.items.ok_or_else(|| Fault::null_pointer("BasketTrans.items"))?.basket_item_trans;
    let item = items.into_iter().next().ok_or_else(|| anyhow::anyhow!("Index 0 out of bounds for length 0"))?;
    let product_id = item.product_id.ok_or_else(|| Fault::null_pointer("BasketItemTrans.productId"))?;

    let mut persona_entity = persona_entity;
    if product_id == "-1" || product_id == "SRV-GARAGESLOT" {
        commerce_result_trans.status = Some(CommerceResultStatus::FAIL_INSUFFICIENT_FUNDS);
    } else if product_id.contains("SRV-POWERUP") {
        return Err(Fault::unported("bo/BasketBO#buyPowerups(String,PersonaEntity)"));
    } else if product_id == "SRV-REPAIR" {
        return Err(Fault::unported("bo/BasketBO#repairCar(String,PersonaEntity)"));
    } else if product_id == "SRV-THREVIVE" {
        return Err(Fault::unported("bo/BasketBO#reviveTreasureHunt(String,PersonaEntity)"));
    } else {
        let product_entity = {
            let product_id = product_id.clone();
            world.call(move |w| basket::find_product(w, &product_id)).await?
        };
        match product_entity.product_type.as_str() {
            "PRESETCAR" => {
                let user = session.user.clone();
                let (status, persona, result) = world
                    .call(move |w| {
                        let mut persona = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
                        let status =
                            basket::buy_car(w, &product_entity, &mut persona, &user, &mut commerce_result_trans)?;
                        Ok((status, persona, commerce_result_trans))
                    })
                    .await?;
                persona_entity = Some(persona);
                commerce_result_trans = result;
                commerce_result_trans.status = Some(status);
            }
            "BUNDLE" => return Err(Fault::unported("bo/BasketBO#buyBundle(String,PersonaEntity,CommerceResultTrans)")),
            "AMPLIFIER" => return Err(Fault::unported("bo/BasketBO#buyAmplifier(PersonaEntity,String)")),
            _ => {}
        }
    }

    let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    commerce_result_trans.wallets = Some(ArrayOfWalletTrans {
        wallet_trans: vec![
            WalletTrans { balance: persona_entity.cash, currency: Some("CASH".into()) },
            WalletTrans { balance: persona_entity.boost, currency: Some("BOOST".into()) },
        ],
    });
    Ok(entity(call, StatusCode::OK, commerce_result_trans.to_xml()))
}

// port: api/Personas#carslots(Long)
/// The garage: every car, the selected one, the slot count, and the
/// garage-slot product.
pub(super) async fn carslots(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    let session = call.session();
    token_session::verify_persona_ownership(session, persona_id)?;

    let world = state.world();
    let persona_entity = world.call(move |w| persona::get_persona_by_id(w, persona_id)).await?;
    let personas_car = world.call(move |w| car_slot::get_personas_car(w, persona_id)).await?;
    let mut array_of_owned_car_trans = ArrayOfOwnedCarTrans::default();
    for car_entity in &personas_car {
        array_of_owned_car_trans.owned_car_trans.push(owned_car_converter::entity2_trans(car_entity));
    }
    let default_owned_car_index = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?.cur_car_index;
    let user = session.user.clone();
    let carlimit = world.call(move |w| w.params.get_car_limit(&user)).await?;
    let product_trans = ProductTrans {
        bundle_items: Some(ArrayOfProductTrans::default()),
        category_id: Some(String::new()),
        currency: Some("_NS".into()),
        description: Some("New car slot !!".into()),
        duration_minute: 0,
        hash: -1143680669,
        icon: Some("128_cash".into()),
        level: 70,
        long_description: Some("New car slot !".into()),
        price: 100.0,
        priority: 0,
        product_id: Some("SRV-GARAGESLOT".into()),
        secondary_icon: Some(String::new()),
        use_count: 1,
        visual_style: Some(String::new()),
        web_icon: Some(String::new()),
        web_location: Some(String::new()),
        ..Default::default()
    };
    let car_slot_info_trans = CarSlotInfoTrans {
        cars_owned_by_persona: Some(array_of_owned_car_trans),
        default_owned_car_index,
        owned_car_slots_count: carlimit,
        obtainable_slots: Some(ArrayOfProductTrans { product_trans: vec![product_trans] }),
    };
    Ok(entity(call, StatusCode::OK, car_slot_info_trans.to_xml()))
}

// port: api/Personas#inventoryObjects(String)
/// The selected driver's inventory.
pub(super) async fn inventory_objects(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = call.session().active_persona_id;
    let world = state.world();
    let persona_entity = world
        .call(move |w| {
            // `personaDao.find(null)`: Hibernate refuses a null id.
            let id = active_persona_id.ok_or_else(|| anyhow::anyhow!("id to load is required for loading"))?;
            persona::get_persona_by_id(w, id)
        })
        .await?;
    let inventory_entity = world
        .call(move |w| {
            let persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
            inventory::get_inventory(w, &persona_entity)
        })
        .await?;
    let inventory_trans = world.call(move |w| inventory::get_client_inventory(w, &inventory_entity)).await?;
    Ok(entity(call, StatusCode::OK, inventory_trans.to_xml()))
}

// port: api/Personas#carsGet(Long)
/// Any driver's cars: the session isn't checked.
pub(super) async fn cars_get(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    let personas_car = state.world().call(move |w| car_slot::get_personas_car(w, persona_id)).await?;
    let array =
        ArrayOfOwnedCarTrans { owned_car_trans: personas_car.iter().map(owned_car_converter::entity2_trans).collect() };
    Ok(entity(call, StatusCode::OK, array.to_xml()))
}

// port: api/Personas#carsGet(Long,Long)
/// Any car by its id, whoever owns it.
pub(super) async fn car_get(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let car_id = path_id(call, "carId")?;
    let car_entity = state.world().call(move |w| persona::get_car_by_owned_car_id(w, car_id)).await?;
    Ok(entity(call, StatusCode::OK, owned_car_converter::entity2_trans(&car_entity).to_xml()))
}

// port: api/Personas#carsPut(Long,InputStream)
/// The performance and skill shops' save: the car sent is ignored, and the
/// answer is the selected car.
pub(super) async fn cars_put(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    token_session::verify_persona_ownership(call.session(), persona_id)?;
    let owned_car_trans = state.world().call(move |w| persona::get_default_car(w, persona_id)).await?;
    Ok(entity(call, StatusCode::OK, owned_car_trans.to_xml()))
}

// port: api/Personas#defaultcarGet(Long)
/// Any driver's selected car.
pub(super) async fn defaultcar_get(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    let owned_car_trans = state.world().call(move |w| persona::get_default_car(w, persona_id)).await?;
    Ok(entity(call, StatusCode::OK, owned_car_trans.to_xml()))
}

// port: api/Personas#defaultcar(Long,Long)
pub(super) async fn defaultcar_put(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = path_id(call, "personaId")?;
    let car_id = path_id(call, "carId")?;
    token_session::verify_persona_ownership(call.session(), persona_id)?;
    state.world().call(move |w| persona::change_default_car(w, persona_id, car_id)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

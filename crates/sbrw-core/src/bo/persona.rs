//! `PersonaBO`: a persona's cars, as the garage and shops use them (the
//! default car and choosing it, looking cars up, and repairs), and the
//! badges it shows.

use anyhow::anyhow;
use sbrw_xml::dto::http::{BadgeBundle, OwnedCarTrans};

use crate::bo::util::owned_car_converter;
use crate::bo::{car_damage, car_slot};
use crate::dao::badge_definition;
use crate::dao::car::{self, CarEntity};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::persona_badge::{self, PersonaBadgeEntity};
use crate::{Fault, World};

// port: bo/PersonaBO#updateBadges(Long,BadgeBundle)
/// Each badge into its slot (0 to 3), replacing what was there. A bad slot
/// or badge fails the whole bundle.
pub fn update_badges(w: &mut World, persona_id: i64, badge_bundle: &BadgeBundle) -> Result<(), Fault> {
    let persona_entity = persona::find(w.db(), persona_id)?;

    for badge_input in badge_bundle.badge_inputs.iter().flatten() {
        if badge_input.slot_id > 3 {
            return Err(anyhow!("Invalid SlotId: {}", badge_input.slot_id).into());
        }

        let Some(badge_definition_entity) = badge_definition::find(w.db(), i64::from(badge_input.badge_definition_id))?
        else {
            return Err(anyhow!("Invalid BadgeDefinitionId: {}", badge_input.badge_definition_id).into());
        };

        let slot = i32::from(badge_input.slot_id);
        match persona_badge::find_badge_in_slot_for_persona(w.db(), persona_id, slot)? {
            None => {
                let persona_entity =
                    persona_entity.as_ref().ok_or_else(|| Fault::null_pointer("PersonaBadgeEntity.personaEntity"))?;
                let mut persona_badge_entity = PersonaBadgeEntity {
                    id: 0,
                    persona_id: persona_entity.persona_id,
                    badge_definition: badge_definition_entity,
                    slot: Some(slot),
                };
                persona_badge::insert(w.db(), &mut persona_badge_entity)?;
            }
            Some(mut persona_badge_entity) => {
                persona_badge_entity.badge_definition = badge_definition_entity;
                persona_badge::update(w.db(), &persona_badge_entity)?;
            }
        }
    }
    Ok(())
}

// port: bo/PersonaBO#changeDefaultCar(Long,Long)
pub fn change_default_car(w: &mut World, persona_id: i64, default_car_id: i64) -> Result<(), Fault> {
    let mut persona_entity = persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    change_default_car_of(w, &mut persona_entity, default_car_id)?;
    persona::update(w.db(), &persona_entity)?;
    Ok(())
}

// port: bo/PersonaBO#changeDefaultCar(PersonaEntity,Long)
/// Select the car with this id: its index in the garage, or one past the
/// last car when the garage has no such car.
pub fn change_default_car_of(w: &World, persona_entity: &mut PersonaEntity, default_car_id: i64) -> Result<(), Fault> {
    let car_slot_list = car::find_by_persona_id(w.db(), persona_entity.persona_id)?;
    let i = car_slot_list.iter().position(|c| c.id == default_car_id).unwrap_or(car_slot_list.len());
    persona_entity.cur_car_index = i as i32;
    Ok(())
}

// port: bo/PersonaBO#getPersonaById(Long)
pub fn get_persona_by_id(w: &World, persona_id: i64) -> Result<Option<PersonaEntity>, Fault> {
    Ok(persona::find(w.db(), persona_id)?)
}

// port: bo/PersonaBO#getDefaultCarEntity(Long)
/// The selected car, with its parts. An index past the last car selects, and
/// saves, the last car. `None` without cars.
pub fn get_default_car_entity(w: &mut World, persona_id: i64) -> Result<Option<CarEntity>, Fault> {
    let car_slot_count = car_slot::count_personas_car(w, persona_id)?;
    if car_slot_count <= 0 {
        return Ok(None);
    }
    let mut persona_entity = persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    let mut cur_car_index = persona_entity.cur_car_index;
    if cur_car_index >= car_slot_count {
        cur_car_index = car_slot_count - 1;
        persona_entity.cur_car_index = cur_car_index;
        persona::update(w.db(), &persona_entity)?;
    }
    if cur_car_index < 0 {
        // `setFirstResult` refuses a negative position.
        return Err(Fault::Internal(anyhow::anyhow!("first-result value cannot be negative : {cur_car_index}")));
    }
    Ok(Some(car::find_by_persona_id_eager_at(w.db(), persona_id, cur_car_index)?))
}

// port: bo/PersonaBO#getDefaultCar(Long)
/// The selected car as the game sees it; an empty car without cars.
pub fn get_default_car(w: &mut World, persona_id: i64) -> Result<OwnedCarTrans, Fault> {
    Ok(match get_default_car_entity(w, persona_id)? {
        Some(car_entity) => owned_car_converter::entity2_trans(&car_entity),
        None => OwnedCarTrans::default(),
    })
}

// port: bo/PersonaBO#repairAllCars(PersonaEntity)
pub fn repair_all_cars(w: &mut World, persona_entity: &PersonaEntity) -> Result<(), Fault> {
    for mut car_entity in car_slot::get_personas_car(w, persona_entity.persona_id)? {
        car_damage::update_durability(w, &mut car_entity, 100)?;
    }
    Ok(())
}

// port: bo/PersonaBO#getCarByOwnedCarId(Long)
pub fn get_car_by_owned_car_id(w: &World, owned_car_id: i64) -> Result<CarEntity, Fault> {
    car::find(w.db(), owned_car_id)?.ok_or_else(|| Fault::null_pointer("CarEntity"))
}

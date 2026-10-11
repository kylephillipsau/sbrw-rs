//! `CarSlotBO`: the cars in a persona's garage.

use crate::bo::performance;
use crate::dao::car::{self, CarEntity};
use crate::{Fault, World};

// port: bo/CarSlotBO#getPersonasCar(Long)
/// The persona's cars in id order, with their parts. A car whose class hash
/// is 0 has its class and rating worked out again and saved.
pub fn get_personas_car(w: &mut World, persona_id: i64) -> Result<Vec<CarEntity>, Fault> {
    let mut owned_car_entities = car::find_by_persona_id(w.db(), persona_id)?;
    for car_entity in &mut owned_car_entities {
        if car_entity.car_class_hash == 0 {
            performance::calc_new_car_class(w, car_entity)?;
            car::update(w.db(), car_entity)?;
        }
    }
    Ok(owned_car_entities)
}

// port: bo/CarSlotBO#countPersonasCar(Long)
pub fn count_personas_car(w: &World, persona_id: i64) -> Result<i32, Fault> {
    Ok(car::find_num_by_persona_id(w.db(), persona_id)?)
}

//! `CarDamageBO`: a car's durability. Ported so far: setting it.

use crate::bo::performance;
use crate::dao::car::{self, CarEntity};
use crate::{Fault, World};

// port: bo/CarDamageBO#updateDurability(CarEntity,int)
/// Set the car's durability; a car wrecked to 0 is rated without its parts,
/// and one repaired from 0 with them again.
pub fn update_durability(w: &World, car_entity: &mut CarEntity, new_durability: i32) -> Result<(), Fault> {
    let old_durability = car_entity.durability;
    car_entity.durability = new_durability;
    car::update(w.db(), car_entity)?;

    if new_durability == 0 && old_durability != 0 {
        performance::calc_new_car_class_ignoring_parts(w, car_entity, true)?;
        car::update(w.db(), car_entity)?;
    } else if new_durability != 0 && old_durability == 0 {
        performance::calc_new_car_class_ignoring_parts(w, car_entity, false)?;
        car::update(w.db(), car_entity)?;
    }
    Ok(())
}

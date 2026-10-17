//! `CarDamageBO`: a car's durability, and the damage events do to it.

use crate::bo::performance;
use crate::bo::util::arbitration::Arbitration;
use crate::dao::car::{self, CarEntity};
use crate::dao::event::EventEntity;
use crate::dao::inventory_item;
use crate::{Fault, World};

// port: bo/CarDamageBO#induceCarDamage(Long,ArbitrationPacket,EventEntity)
/// Take 5% off the durability of the car the packet names (2% for a drag),
/// down to 0, and report the car's durability. With `ENABLE_CAR_DAMAGE` off,
/// or insurance, nothing changes and 100 is reported. Whose car it is isn't
/// checked.
pub fn induce_car_damage(
    w: &World,
    persona_id: i64,
    arbitration_packet: &dyn Arbitration,
    event_entity: &EventEntity,
) -> Result<i32, Fault> {
    if !w.params.get_bool_param("ENABLE_CAR_DAMAGE") {
        return Ok(100);
    }

    let insurance = inventory_item::find_by_persona_id_and_entitlement_tag(w.db(), persona_id, "INSURANCE_AMPLIFIER")?;

    if insurance.is_some() {
        return Ok(100);
    }

    let car_id = arbitration_packet.car_id();
    let mut car_entity = car::find(w.db(), car_id)?.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    let durability = car_entity.durability;
    if durability > 0 {
        let calc_damage = if event_entity.event_mode_id == 19 { 2 } else { 5 }; // 5% for non-drags, 2% for drags
        let new_car_damage = (durability - calc_damage).max(0);

        update_durability(w, &mut car_entity, new_car_damage)?;
    }
    Ok(car_entity.durability)
}

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

//! `PerformanceBO`: a car's rating and class from its model's stock stats
//! and its performance parts, in `float` arithmetic as upstream does it.

use crate::dao::car::CarEntity;
use crate::dao::{car_classes, product};
use crate::{Fault, World};

// port: bo/PerformanceBO#calcNewCarClass(CarEntity)
/// A wrecked car (durability 0) is rated without its parts.
pub fn calc_new_car_class(w: &World, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let ignore_parts = car_entity.durability == 0;
    calc_new_car_class_ignoring_parts(w, car_entity, ignore_parts)
}

// port: bo/PerformanceBO#calcNewCarClass(CarEntity,boolean)
/// Set the car's rating and class hash. A car whose physics profile isn't
/// exactly one model in `car_classes` keeps the ones it has.
pub fn calc_new_car_class_ignoring_parts(
    w: &World,
    car_entity: &mut CarEntity,
    ignore_parts: bool,
) -> Result<(), Fault> {
    let Some(car_classes_entity) = car_classes::find_by_hash(w.db(), car_entity.physics_profile_hash)? else {
        return Ok(());
    };
    let null = |what| move || Fault::null_pointer(what);
    let (mut top_speed, mut accel, mut handling) = (0i32, 0i32, 0i32);
    if !ignore_parts {
        for performance_part_entity in &car_entity.performance_parts {
            let product_entity = product::find_by_hash(w.db(), performance_part_entity.performance_part_attrib_hash)?;
            top_speed = product_entity.top_speed.ok_or_else(null("ProductEntity.topSpeed"))?.wrapping_add(top_speed);
            accel = product_entity.accel.ok_or_else(null("ProductEntity.accel"))?.wrapping_add(accel);
            handling = product_entity.handling.ok_or_else(null("ProductEntity.handling"))?.wrapping_add(handling);
        }
    }
    let mut tt = (f64::from(top_speed) * 0.01) as f32;
    let mut ta = (f64::from(accel) * 0.01) as f32;
    let mut th = (f64::from(handling) * 0.01) as f32;
    // Upstream's literal, which rounds to the same float as 0.6666667.
    #[allow(clippy::excessive_precision)]
    let total_changes = 1.0f32 / (((tt + ta + th) * 0.666666666666666f32) + 1.0f32);
    tt *= total_changes;
    ta *= total_changes;
    th *= total_changes;
    let final_constant = 1.0f32 - tt - ta - th;

    // `Integer.floatValue()` of each stat.
    let stat = |value: Option<i32>, what| value.map(|v| v as f32).ok_or_else(null(what));
    let c = &car_classes_entity;
    let final_top_speed1 = stat(c.ts_var1, "tsVar1")? * th;
    let final_top_speed2 = stat(c.ts_var2, "tsVar2")? * ta;
    let final_top_speed3 = stat(c.ts_var3, "tsVar3")? * tt;
    let final_top_speed =
        (final_constant * stat(c.ts_stock, "tsStock")?) + final_top_speed1 + final_top_speed2 + final_top_speed3;

    let final_accel1 = stat(c.ac_var1, "acVar1")? * th;
    let final_accel2 = stat(c.ac_var2, "acVar2")? * ta;
    let final_accel3 = stat(c.ac_var3, "acVar3")? * tt;
    let final_accel = (final_constant * stat(c.ac_stock, "acStock")?) + final_accel1 + final_accel2 + final_accel3;

    let final_handling1 = stat(c.ha_var1, "haVar1")? * th;
    let final_handling2 = stat(c.ha_var2, "haVar2")? * ta;
    let final_handling3 = stat(c.ha_var3, "haVar3")? * tt;
    let final_handling =
        (final_constant * stat(c.ha_stock, "haStock")?) + final_handling1 + final_handling2 + final_handling3;

    // `(int)` casts truncate, as `as` does.
    let sum = (final_top_speed as i32).wrapping_add(final_accel as i32).wrapping_add(final_handling as i32);
    let final_class = sum as f32 / 3.0f32;
    let final_class_int = final_class as i32;

    let carclass_hash = match final_class_int {
        250..400 => 415909161,
        400..500 => 1866825865,
        500..600 => -406473455,
        600..750 => -405837480,
        750.. => -2142411446,
        _ => 872416321,
    };
    car_entity.car_class_hash = carclass_hash;
    car_entity.rating = final_class_int;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::dao::car::PerformancePartEntity;

    #[test]
    fn rates_every_model_as_the_original() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/car_ratings.json");
        let vectors: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = vectors["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 1323);
        let w = World::test(&[]);
        let mut failures = Vec::new();
        for case in cases {
            let parts: Vec<i32> = serde_json::from_value(case["parts"].clone()).unwrap();
            let mut car = CarEntity {
                physics_profile_hash: case["physics_profile_hash"].as_i64().unwrap() as i32,
                car_class_hash: -1,
                rating: -1,
                performance_parts: parts
                    .iter()
                    .map(|&h| PerformancePartEntity { id: 0, performance_part_attrib_hash: h })
                    .collect(),
                ..Default::default()
            };
            calc_new_car_class_ignoring_parts(&w, &mut car, case["ignore_parts"].as_bool().unwrap()).unwrap();
            let expected = (case["car_class_hash"].as_i64().unwrap() as i32, case["rating"].as_i64().unwrap() as i32);
            if (car.car_class_hash, car.rating) != expected {
                failures.push(format!("{case}: got {} {}", car.car_class_hash, car.rating));
            }
        }
        assert!(
            failures.is_empty(),
            "{} of {} differ: {:#?}",
            failures.len(),
            cases.len(),
            &failures[..failures.len().min(5)]
        );
        let rated = cases.iter().filter(|c| c["rating"].as_i64() != Some(-1)).count();
        assert!(rated > 1000, "most cases are real models: {rated}");
    }

    #[test]
    fn classes_change_at_upstreams_boundaries() {
        // No recorded car lands on a boundary, so bare made-up models whose
        // stock stats are the rating.
        let w = World::test(&[]);
        let classes = [
            (249, 872416321),
            (250, 415909161),
            (399, 415909161),
            (400, 1866825865),
            (499, 1866825865),
            (500, -406473455),
            (599, -406473455),
            (600, -405837480),
            (749, -405837480),
            (750, -2142411446),
        ];
        for (i, (rating, class)) in classes.into_iter().enumerate() {
            let hash = 900_000 + i as i32;
            w.db()
                .execute(
                    "INSERT INTO car_classes (store_name, full_name, manufactor, model, ts_stock, ts_var1, ts_var2,
                     ts_var3, ac_stock, ac_var1, ac_var2, ac_var3, ha_stock, ha_var1, ha_var2, ha_var3, hash)
                     VALUES (?1, '', '', '', ?2, 0, 0, 0, ?2, 0, 0, 0, ?2, 0, 0, 0, ?3)",
                    rusqlite::params![format!("test{i}"), rating, hash],
                )
                .unwrap();
            let mut car = CarEntity { physics_profile_hash: hash, ..Default::default() };
            calc_new_car_class(&w, &mut car).unwrap();
            assert_eq!((car.rating, car.car_class_hash), (rating, class));
        }
    }
}

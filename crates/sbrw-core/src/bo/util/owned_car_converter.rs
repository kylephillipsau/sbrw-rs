//! `OwnedCarConverter`: cars between their entities and the game's XML.

use sbrw_xml::dto::http::{
    ArrayOfCustomPaintTrans, ArrayOfCustomVinylTrans, ArrayOfPerformancePartTrans, ArrayOfSkillModPartTrans,
    ArrayOfVisualPartTrans, CustomCarTrans, CustomPaintTrans, CustomVinylTrans, OwnedCarTrans, PerformancePartTrans,
    SkillModPartTrans, VisualPartTrans,
};

use crate::bo::util::time_converter;
use crate::dao::car::CarEntity;

// port: bo/util/OwnedCarConverter#entity2Trans(CarEntity)
/// A car and its parts, in the order the entity holds them.
pub fn entity2_trans(car: &CarEntity) -> OwnedCarTrans {
    let custom_car = CustomCarTrans {
        base_car: car.base_car,
        car_class_hash: car.car_class_hash,
        // `getId().intValue()`.
        id: car.id as i32,
        is_preset: car.is_preset,
        level: car.level,
        name: car.name.clone(),
        physics_profile_hash: car.physics_profile_hash,
        rating: car.rating,
        resale_price: car.resale_price,
        ride_height_drop: car.ride_height_drop,
        skill_mod_slot_count: car.skill_mod_slot_count,
        version: car.version,
        paints: Some(ArrayOfCustomPaintTrans {
            custom_paint_trans: car
                .paints
                .iter()
                .map(|p| CustomPaintTrans { group: p.group, hue: p.hue, sat: p.sat, slot: p.slot, var: p.var })
                .collect(),
        }),
        performance_parts: Some(ArrayOfPerformancePartTrans {
            performance_part_trans: car
                .performance_parts
                .iter()
                .map(|p| PerformancePartTrans { performance_part_attrib_hash: p.performance_part_attrib_hash })
                .collect(),
        }),
        skill_mod_parts: Some(ArrayOfSkillModPartTrans {
            skill_mod_part_trans: car
                .skill_mod_parts
                .iter()
                .map(|p| SkillModPartTrans {
                    is_fixed: p.is_fixed,
                    skill_mod_part_attrib_hash: p.skill_mod_part_attrib_hash,
                })
                .collect(),
        }),
        vinyls: Some(ArrayOfCustomVinylTrans {
            custom_vinyl_trans: car
                .vinyls
                .iter()
                .map(|v| CustomVinylTrans {
                    hash: v.hash,
                    hue1: v.hue1,
                    hue2: v.hue2,
                    hue3: v.hue3,
                    hue4: v.hue4,
                    layer: v.layer,
                    mir: v.mir,
                    rot: v.rot,
                    sat1: v.sat1,
                    sat2: v.sat2,
                    sat3: v.sat3,
                    sat4: v.sat4,
                    scale_x: v.scalex,
                    scale_y: v.scaley,
                    shear: v.shear,
                    tran_x: v.tranx,
                    tran_y: v.trany,
                    var1: v.var1,
                    var2: v.var2,
                    var3: v.var3,
                    var4: v.var4,
                })
                .collect(),
        }),
        visual_parts: Some(ArrayOfVisualPartTrans {
            visual_part_trans: car
                .visual_parts
                .iter()
                .map(|p| VisualPartTrans { part_hash: p.part_hash, slot_hash: p.slot_hash })
                .collect(),
        }),
    };
    OwnedCarTrans {
        id: car.id,
        durability: car.durability,
        heat: car.heat,
        ownership_type: car.ownership_type.clone(),
        expiration_date: car.expiration_date.map(time_converter::generate_gregorian_calendar),
        custom_car: Some(custom_car),
    }
}

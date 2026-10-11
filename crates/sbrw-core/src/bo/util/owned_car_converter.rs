//! `OwnedCarConverter`: cars between their entities and the game's XML.

use sbrw_xml::dto::http::{
    ArrayOfCustomPaintTrans, ArrayOfCustomVinylTrans, ArrayOfPerformancePartTrans, ArrayOfSkillModPartTrans,
    ArrayOfVisualPartTrans, CustomCarTrans, CustomPaintTrans, CustomVinylTrans, OwnedCarTrans, PerformancePartTrans,
    SkillModPartTrans, VisualPartTrans,
};

use crate::Fault;
use crate::bo::util::time_converter;
use crate::dao::car::{
    CarEntity, PaintEntity, PerformancePartEntity, SkillModPartEntity, VinylEntity, VisualPartEntity,
};

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

fn custom_car(owned_car_trans: &OwnedCarTrans) -> Result<&CustomCarTrans, Fault> {
    owned_car_trans.custom_car.as_ref().ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))
}

// port: bo/util/OwnedCarConverter#trans2Entity(OwnedCarTrans,CarEntity)
/// The car's own fields from the XML. Not its id or expiry; and upstream sets
/// the ownership type to the entity's own, so the XML's is ignored.
pub fn trans2_entity(owned_car_trans: &OwnedCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    car_entity.durability = owned_car_trans.durability;
    car_entity.heat = owned_car_trans.heat;
    let custom_car_trans = custom_car(owned_car_trans)?;
    car_entity.base_car = custom_car_trans.base_car;
    car_entity.car_class_hash = custom_car_trans.car_class_hash;
    car_entity.level = custom_car_trans.level;
    car_entity.name = custom_car_trans.name.clone();
    car_entity.physics_profile_hash = custom_car_trans.physics_profile_hash;
    car_entity.is_preset = custom_car_trans.is_preset;
    car_entity.rating = custom_car_trans.rating;
    car_entity.resale_price = custom_car_trans.resale_price;
    car_entity.ride_height_drop = custom_car_trans.ride_height_drop;
    car_entity.skill_mod_slot_count = custom_car_trans.skill_mod_slot_count;
    car_entity.version = custom_car_trans.version;
    Ok(())
}

/// A list the XML must have: upstream reads it without a null check.
fn list<'a, A, T>(array: &'a Option<A>, items: impl FnOnce(&'a A) -> &'a [T], what: &str) -> Result<&'a [T], Fault> {
    array.as_ref().map(items).ok_or_else(|| Fault::null_pointer(what))
}

// port: bo/util/OwnedCarConverter#paints2NewEntity(CustomCarTrans,CarEntity)
pub fn paints2_new_entity(custom_car_trans: &CustomCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let paints = list(&custom_car_trans.paints, |a| &a.custom_paint_trans, "CustomCarTrans.paints")?;
    car_entity.paints = paints
        .iter()
        .map(|p| PaintEntity { id: 0, group: p.group, hue: p.hue, sat: p.sat, slot: p.slot, var: p.var })
        .collect();
    Ok(())
}

// port: bo/util/OwnedCarConverter#performanceParts2NewEntity(CustomCarTrans,CarEntity)
pub fn performance_parts2_new_entity(
    custom_car_trans: &CustomCarTrans,
    car_entity: &mut CarEntity,
) -> Result<(), Fault> {
    let parts =
        list(&custom_car_trans.performance_parts, |a| &a.performance_part_trans, "CustomCarTrans.performanceParts")?;
    car_entity.performance_parts = parts
        .iter()
        .map(|p| PerformancePartEntity { id: 0, performance_part_attrib_hash: p.performance_part_attrib_hash })
        .collect();
    Ok(())
}

// port: bo/util/OwnedCarConverter#skillModParts2NewEntity(CustomCarTrans,CarEntity)
pub fn skill_mod_parts2_new_entity(custom_car_trans: &CustomCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let parts = list(&custom_car_trans.skill_mod_parts, |a| &a.skill_mod_part_trans, "CustomCarTrans.skillModParts")?;
    car_entity.skill_mod_parts = parts
        .iter()
        .map(|p| SkillModPartEntity {
            id: 0,
            is_fixed: p.is_fixed,
            skill_mod_part_attrib_hash: p.skill_mod_part_attrib_hash,
        })
        .collect();
    Ok(())
}

// port: bo/util/OwnedCarConverter#vinyls2NewEntity(CustomCarTrans,CarEntity)
pub fn vinyls2_new_entity(custom_car_trans: &CustomCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let vinyls = list(&custom_car_trans.vinyls, |a| &a.custom_vinyl_trans, "CustomCarTrans.vinyls")?;
    car_entity.vinyls = vinyls
        .iter()
        .map(|v| VinylEntity {
            id: 0,
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
            scalex: v.scale_x,
            scaley: v.scale_y,
            shear: v.shear,
            tranx: v.tran_x,
            trany: v.tran_y,
            var1: v.var1,
            var2: v.var2,
            var3: v.var3,
            var4: v.var4,
        })
        .collect();
    Ok(())
}

// port: bo/util/OwnedCarConverter#visuallParts2NewEntity(CustomCarTrans,CarEntity)
pub fn visual_parts2_new_entity(custom_car_trans: &CustomCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let parts = list(&custom_car_trans.visual_parts, |a| &a.visual_part_trans, "CustomCarTrans.visualParts")?;
    car_entity.visual_parts =
        parts.iter().map(|p| VisualPartEntity { id: 0, part_hash: p.part_hash, slot_hash: p.slot_hash }).collect();
    Ok(())
}

// port: bo/util/OwnedCarConverter#details2NewEntity(OwnedCarTrans,CarEntity)
/// Replace the car's parts with new ones from the XML.
pub fn details2_new_entity(owned_car_trans: &OwnedCarTrans, car_entity: &mut CarEntity) -> Result<(), Fault> {
    let custom_car_trans = custom_car(owned_car_trans)?;
    paints2_new_entity(custom_car_trans, car_entity)?;
    performance_parts2_new_entity(custom_car_trans, car_entity)?;
    skill_mod_parts2_new_entity(custom_car_trans, car_entity)?;
    vinyls2_new_entity(custom_car_trans, car_entity)?;
    visual_parts2_new_entity(custom_car_trans, car_entity)?;
    Ok(())
}

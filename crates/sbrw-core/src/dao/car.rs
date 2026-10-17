//! `CarEntity` (a car a persona owns), the parts it holds, and the parts of
//! `CarDAO` ported so far.

// port: dao/CarDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::datetime;

/// `CarEntity`. Its five part collections are `Set`s fetched with the car
/// (`LazyCollectionOption.FALSE`). Upstream iterates them in identity-hash
/// order, which can't be reproduced; they are kept in id order here.
#[derive(Debug, Clone, PartialEq)]
pub struct CarEntity {
    pub id: i64,
    /// The `personaId` the LAZY `persona` joins on.
    pub persona_id: i64,
    pub durability: i32,
    pub heat: f32,
    pub expiration_date: Option<PrimitiveDateTime>,
    pub ownership_type: Option<String>,
    pub base_car: i32,
    pub car_class_hash: i32,
    pub is_preset: bool,
    pub level: i32,
    pub name: Option<String>,
    pub physics_profile_hash: i32,
    pub rating: i32,
    pub resale_price: f32,
    pub ride_height_drop: f32,
    pub skill_mod_slot_count: i32,
    pub version: i32,
    pub paints: Vec<PaintEntity>,
    pub performance_parts: Vec<PerformancePartEntity>,
    pub skill_mod_parts: Vec<SkillModPartEntity>,
    pub vinyls: Vec<VinylEntity>,
    pub visual_parts: Vec<VisualPartEntity>,
}

impl Default for CarEntity {
    /// `new CarEntity()`: a customised car, with no parts loaded.
    fn default() -> Self {
        Self {
            id: 0,
            persona_id: 0,
            durability: 0,
            heat: 0.0,
            expiration_date: None,
            ownership_type: Some("CustomizedCar".into()),
            base_car: 0,
            car_class_hash: 0,
            is_preset: false,
            level: 0,
            name: None,
            physics_profile_hash: 0,
            rating: 0,
            resale_price: 0.0,
            ride_height_drop: 0.0,
            skill_mod_slot_count: 0,
            version: 0,
            paints: Vec::new(),
            performance_parts: Vec::new(),
            skill_mod_parts: Vec::new(),
            vinyls: Vec::new(),
            visual_parts: Vec::new(),
        }
    }
}

/// `PaintEntity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaintEntity {
    pub id: i64,
    pub group: i32,
    pub hue: i32,
    pub sat: i32,
    pub slot: i32,
    pub var: i32,
}

/// `PerformancePartEntity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerformancePartEntity {
    pub id: i64,
    pub performance_part_attrib_hash: i32,
}

/// `SkillModPartEntity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SkillModPartEntity {
    pub id: i64,
    pub is_fixed: bool,
    pub skill_mod_part_attrib_hash: i32,
}

/// `VinylEntity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VinylEntity {
    pub id: i64,
    pub hash: i32,
    pub hue1: i32,
    pub hue2: i32,
    pub hue3: i32,
    pub hue4: i32,
    pub layer: i32,
    pub mir: bool,
    pub rot: i32,
    pub sat1: i32,
    pub sat2: i32,
    pub sat3: i32,
    pub sat4: i32,
    pub scalex: i32,
    pub scaley: i32,
    pub shear: i32,
    pub tranx: i32,
    pub trany: i32,
    pub var1: i32,
    pub var2: i32,
    pub var3: i32,
    pub var4: i32,
}

/// `VisualPartEntity`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisualPartEntity {
    pub id: i64,
    pub part_hash: i32,
    pub slot_hash: i32,
}

impl CarEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            persona_id: row.get("personaId")?,
            durability: row.get("durability")?,
            heat: row.get::<_, f64>("heat")? as f32,
            expiration_date: datetime::column(row, "expirationDate")?,
            ownership_type: row.get("ownershipType")?,
            base_car: row.get("baseCar")?,
            car_class_hash: row.get("carClassHash")?,
            is_preset: row.get("isPreset")?,
            level: row.get("level")?,
            name: row.get("name")?,
            physics_profile_hash: row.get("physicsProfileHash")?,
            rating: row.get("rating")?,
            resale_price: row.get::<_, f64>("resalePrice")? as f32,
            ride_height_drop: row.get::<_, f64>("rideHeightDrop")? as f32,
            skill_mod_slot_count: row.get("skillModSlotCount")?,
            version: row.get("version")?,
            ..Default::default()
        })
    }

    /// The car with its parts, as Hibernate loads it.
    fn with_parts(mut self, db: &Connection) -> rusqlite::Result<Self> {
        self.paints = parts(db, "paint", self.id, |r| {
            Ok(PaintEntity {
                id: r.get("id")?,
                group: r.get("paintGroup")?,
                hue: r.get("hue")?,
                sat: r.get("sat")?,
                slot: r.get("slot")?,
                var: r.get("paintVar")?,
            })
        })?;
        self.performance_parts = parts(db, "performancepart", self.id, |r| {
            Ok(PerformancePartEntity {
                id: r.get("id")?,
                performance_part_attrib_hash: r.get("performancePartAttribHash")?,
            })
        })?;
        self.skill_mod_parts = parts(db, "skillmodpart", self.id, |r| {
            Ok(SkillModPartEntity {
                id: r.get("id")?,
                is_fixed: r.get("isFixed")?,
                skill_mod_part_attrib_hash: r.get("skillModPartAttribHash")?,
            })
        })?;
        self.vinyls = parts(db, "vinyl", self.id, |r| {
            Ok(VinylEntity {
                id: r.get("id")?,
                hash: r.get("hash")?,
                hue1: r.get("hue1")?,
                hue2: r.get("hue2")?,
                hue3: r.get("hue3")?,
                hue4: r.get("hue4")?,
                layer: r.get("layer")?,
                mir: r.get("mir")?,
                rot: r.get("rot")?,
                sat1: r.get("sat1")?,
                sat2: r.get("sat2")?,
                sat3: r.get("sat3")?,
                sat4: r.get("sat4")?,
                scalex: r.get("scalex")?,
                scaley: r.get("scaley")?,
                shear: r.get("shear")?,
                tranx: r.get("tranx")?,
                trany: r.get("trany")?,
                var1: r.get("var1")?,
                var2: r.get("var2")?,
                var3: r.get("var3")?,
                var4: r.get("var4")?,
            })
        })?;
        self.visual_parts = parts(db, "visualpart", self.id, |r| {
            Ok(VisualPartEntity { id: r.get("id")?, part_hash: r.get("partHash")?, slot_hash: r.get("slotHash")? })
        })?;
        Ok(self)
    }
}

fn parts<T>(
    db: &Connection,
    table: &str,
    car_id: i64,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    db.prepare_cached(&format!("SELECT * FROM {table} WHERE carId = ?1 ORDER BY id"))?
        .query_map([car_id], map)?
        .collect()
}

fn with_parts(db: &Connection, cars: Vec<CarEntity>) -> rusqlite::Result<Vec<CarEntity>> {
    cars.into_iter().map(|car| car.with_parts(db)).collect()
}

/// `CarDAO.find`.
pub fn find(db: &Connection, id: i64) -> rusqlite::Result<Option<CarEntity>> {
    db.prepare_cached("SELECT * FROM car WHERE id = ?1")?
        .query_row([id], CarEntity::from_row)
        .optional()?
        .map(|car| car.with_parts(db))
        .transpose()
}

// port: dao/CarDAO#findByPersonaId(Long)
// port: jpa/CarEntity@CarEntity.findByPersonaId
pub fn find_by_persona_id(db: &Connection, persona_id: i64) -> rusqlite::Result<Vec<CarEntity>> {
    let cars = db
        .prepare_cached("SELECT * FROM car WHERE personaId = ?1 ORDER BY id")?
        .query_map([persona_id], CarEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    with_parts(db, cars)
}

// port: dao/CarDAO#findByPersonaIdEager(Long,int)
// port: jpa/CarEntity@CarEntity.findByPersonaIdEager
/// The persona's car at `index` in id order. Like `getSingleResult`, it fails
/// when there is none.
pub fn find_by_persona_id_eager_at(db: &Connection, persona_id: i64, index: i32) -> rusqlite::Result<CarEntity> {
    db.prepare_cached("SELECT * FROM car WHERE personaId = ?1 ORDER BY id LIMIT 1 OFFSET ?2")?
        .query_row(rusqlite::params![persona_id, index], CarEntity::from_row)?
        .with_parts(db)
}

// port: dao/CarDAO#findNumByPersonaId(Long)
// port: jpa/CarEntity@CarEntity.findNumByPersonaId
pub fn find_num_by_persona_id(db: &Connection, persona_id: i64) -> rusqlite::Result<i32> {
    // `Long.intValue()`.
    Ok(db
        .prepare_cached("SELECT COUNT(*) FROM car WHERE personaId = ?1")?
        .query_row([persona_id], |r| r.get::<_, i64>(0))? as i32)
}

// port: dao/CarDAO#findNumNonRentalsByPersonaId(Long)
// port: jpa/CarEntity@CarEntity.findNumNonRentalsByPersonaId
pub fn find_num_non_rentals_by_persona_id(db: &Connection, persona_id: i64) -> rusqlite::Result<i64> {
    db.prepare_cached("SELECT COUNT(*) FROM car WHERE personaId = ?1 AND expirationDate IS NULL")?
        .query_row([persona_id], |r| r.get(0))
}

/// `CarDAO.insert` (`persist`): the car, then its parts (cascaded); sets the ids.
pub fn insert(db: &Connection, car: &mut CarEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO car (durability, expirationDate, heat, ownershipType, personaId, baseCar, carClassHash, isPreset,
         level, name, physicsProfileHash, rating, resalePrice, rideHeightDrop, skillModSlotCount, version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?
    .execute(params![
        car.durability,
        datetime::opt_to_sql(car.expiration_date),
        f64::from(car.heat),
        car.ownership_type,
        car.persona_id,
        car.base_car,
        car.car_class_hash,
        car.is_preset,
        car.level,
        car.name,
        car.physics_profile_hash,
        car.rating,
        f64::from(car.resale_price),
        f64::from(car.ride_height_drop),
        car.skill_mod_slot_count,
        car.version,
    ])?;
    car.id = db.last_insert_rowid();
    insert_parts(db, car)
}

/// The car's parts, as new rows; sets their ids.
fn insert_parts(db: &Connection, car: &mut CarEntity) -> rusqlite::Result<()> {
    for p in &mut car.paints {
        db.prepare_cached(
            "INSERT INTO paint (paintGroup, hue, sat, slot, paintVar, carId) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?
        .execute(params![p.group, p.hue, p.sat, p.slot, p.var, car.id])?;
        p.id = db.last_insert_rowid();
    }
    for p in &mut car.performance_parts {
        db.prepare_cached("INSERT INTO performancepart (performancePartAttribHash, carId) VALUES (?1, ?2)")?
            .execute(params![p.performance_part_attrib_hash, car.id])?;
        p.id = db.last_insert_rowid();
    }
    for p in &mut car.skill_mod_parts {
        db.prepare_cached("INSERT INTO skillmodpart (isFixed, skillModPartAttribHash, carId) VALUES (?1, ?2, ?3)")?
            .execute(params![p.is_fixed, p.skill_mod_part_attrib_hash, car.id])?;
        p.id = db.last_insert_rowid();
    }
    for v in &mut car.vinyls {
        db.prepare_cached(
            "INSERT INTO vinyl (hash, hue1, hue2, hue3, hue4, layer, mir, rot, sat1, sat2, sat3, sat4, scalex, scaley,
             shear, tranx, trany, var1, var2, var3, var4, carId)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)",
        )?
        .execute(params![
            v.hash, v.hue1, v.hue2, v.hue3, v.hue4, v.layer, v.mir, v.rot, v.sat1, v.sat2, v.sat3, v.sat4, v.scalex,
            v.scaley, v.shear, v.tranx, v.trany, v.var1, v.var2, v.var3, v.var4, car.id,
        ])?;
        v.id = db.last_insert_rowid();
    }
    for p in &mut car.visual_parts {
        db.prepare_cached("INSERT INTO visualpart (partHash, slotHash, carId) VALUES (?1, ?2, ?3)")?
            .execute(params![p.part_hash, p.slot_hash, car.id])?;
        p.id = db.last_insert_rowid();
    }
    Ok(())
}

/// What Hibernate flushes for a managed car whose part sets were replaced
/// (`OwnedCarConverter.*2NewEntity` clear and refill them): the old parts
/// are orphans, so their rows go, and the new parts get new rows; sets their ids.
pub fn replace_parts(db: &Connection, car: &mut CarEntity) -> rusqlite::Result<()> {
    for table in ["paint", "performancepart", "skillmodpart", "vinyl", "visualpart"] {
        db.prepare_cached(&format!("DELETE FROM {table} WHERE carId = ?1"))?.execute([car.id])?;
    }
    insert_parts(db, car)
}

/// `CarDAO.update` (`merge`), and what Hibernate flushes for a changed managed
/// car: the car's columns. The merge cascades to parts the car already holds;
/// [`replace_parts`] writes parts that changed.
pub fn update(db: &Connection, car: &CarEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE car SET durability = ?2, expirationDate = ?3, heat = ?4, ownershipType = ?5, personaId = ?6,
         baseCar = ?7, carClassHash = ?8, isPreset = ?9, level = ?10, name = ?11, physicsProfileHash = ?12,
         rating = ?13, resalePrice = ?14, rideHeightDrop = ?15, skillModSlotCount = ?16, version = ?17 WHERE id = ?1",
    )?
    .execute(params![
        car.id,
        car.durability,
        datetime::opt_to_sql(car.expiration_date),
        f64::from(car.heat),
        car.ownership_type,
        car.persona_id,
        car.base_car,
        car.car_class_hash,
        car.is_preset,
        car.level,
        car.name,
        car.physics_profile_hash,
        car.rating,
        f64::from(car.resale_price),
        f64::from(car.ride_height_drop),
        car.skill_mod_slot_count,
        car.version,
    ])?;
    Ok(())
}

/// `CarDAO.delete` (`remove(merge(car))`). The part collections have
/// `orphanRemoval`, so Hibernate removes the parts with the car; the foreign
/// keys' `ON DELETE CASCADE` does the same here.
pub fn delete(db: &Connection, car: &CarEntity) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM car WHERE id = ?1")?.execute([car.id])?;
    Ok(())
}

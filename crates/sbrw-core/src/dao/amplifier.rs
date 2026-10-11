//! `AmplifierEntity` (what an amplifier product does, static data) and
//! `AmplifierDAO`.

// port: dao/AmplifierDAO#<init>()

use rusqlite::{Connection, Row};

/// `AmplifierEntity`. Its product is held by the `productId` it joins on.
#[derive(Debug, Clone, PartialEq)]
pub struct AmplifierEntity {
    pub id: i64,
    pub product_id: String,
    pub amp_type: Option<String>,
    pub cash_multiplier: Option<f32>,
    pub rep_multiplier: Option<f32>,
}

impl AmplifierEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            product_id: row.get("product_id")?,
            amp_type: row.get("ampType")?,
            cash_multiplier: row.get::<_, Option<f64>>("cashMultiplier")?.map(|v| v as f32),
            rep_multiplier: row.get::<_, Option<f64>>("repMultiplier")?.map(|v| v as f32),
        })
    }
}

// port: dao/AmplifierDAO#findAmplifierByHash(Integer)
// port: jpa/AmplifierEntity@AmplifierEntity.findAmplifierByHash
/// The amplifier whose product has this hash. Like `getSingleResult`, it
/// fails unless there is exactly one.
pub fn find_amplifier_by_hash(db: &Connection, hash: Option<i32>) -> rusqlite::Result<AmplifierEntity> {
    let found: Vec<AmplifierEntity> = db
        .prepare_cached(
            "SELECT a.* FROM amplifiers a JOIN product p ON p.productId = a.product_id WHERE p.hash = ?1 LIMIT 2",
        )?
        .query_map([hash], AmplifierEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    match <[AmplifierEntity; 1]>::try_from(found) {
        Ok([amplifier]) => Ok(amplifier),
        Err(_) => Err(rusqlite::Error::QueryReturnedNoRows),
    }
}

//! `TreasureHuntConfigEntity` (what a day of the treasure hunt pays, by
//! streak) and `TreasureHuntConfigDAO`. Static data.

// port: dao/TreasureHuntConfigDAO#<init>()

use rusqlite::{Connection, Row};

use crate::Fault;
use crate::engine::{EngineException, EngineExceptionCode};

/// `TreasureHuntConfigEntity`. Its EAGER reward table is kept as the id it
/// joins on.
#[derive(Debug, Clone, PartialEq)]
pub struct TreasureHuntConfigEntity {
    pub id: i64,
    pub streak: Option<i32>,
    pub reward_table_id: Option<i64>,
    pub base_rep: Option<f32>,
    pub base_cash: Option<f32>,
    pub rep_multiplier: Option<f32>,
    pub cash_multiplier: Option<f32>,
}

impl TreasureHuntConfigEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        // MySQL FLOAT columns.
        let float = |name: &str| row.get::<_, Option<f64>>(name).map(|v| v.map(|v| v as f32));
        Ok(Self {
            id: row.get("ID")?,
            streak: row.get("streak")?,
            reward_table_id: row.get("reward_table_id")?,
            base_rep: float("base_rep")?,
            base_cash: float("base_cash")?,
            rep_multiplier: float("rep_multiplier")?,
            cash_multiplier: float("cash_multiplier")?,
        })
    }
}

// port: dao/TreasureHuntConfigDAO#findForStreak(Integer)
// port: jpa/TreasureHuntConfigEntity@TreasureHuntConfigEntity.findConfigForStreak
/// The configuration for the highest streak up to this one.
pub fn find_for_streak(db: &Connection, streak: Option<i32>) -> Result<TreasureHuntConfigEntity, Fault> {
    let found = db
        .prepare_cached("SELECT * FROM treasure_hunt_config WHERE streak <= ?1 ORDER BY streak DESC, ID LIMIT 1")?
        .query_map([streak], TreasureHuntConfigEntity::from_row)?
        .next()
        .transpose()?;
    found.ok_or_else(|| {
        Fault::Engine(EngineException::with_message(
            "No treasure hunt configuration was found in the database",
            EngineExceptionCode::UnspecifiedError,
            true,
        ))
    })
}

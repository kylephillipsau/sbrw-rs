//! `TreasureHuntEntity` (a persona's daily treasure hunt) and `TreasureHuntDAO`.

// port: dao/TreasureHuntDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::Date;

use crate::datetime;

/// `TreasureHuntEntity`, keyed by its persona. `Default` is `new TreasureHuntEntity()`.
#[derive(Debug, Clone, PartialEq)]
pub struct TreasureHuntEntity {
    pub persona_id: i64,
    pub coins_collected: Option<i32>,
    pub is_streak_broken: Option<bool>,
    pub is_completed: Option<bool>,
    pub num_coins: Option<i32>,
    pub seed: Option<i32>,
    pub streak: Option<i32>,
    pub th_date: Option<Date>,
}

impl Default for TreasureHuntEntity {
    fn default() -> Self {
        Self {
            persona_id: 0,
            coins_collected: None,
            is_streak_broken: None,
            is_completed: Some(false),
            num_coins: None,
            seed: None,
            streak: None,
            th_date: None,
        }
    }
}

impl TreasureHuntEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            persona_id: row.get("personaId")?,
            coins_collected: row.get("coinsCollected")?,
            is_streak_broken: row.get("isStreakBroken")?,
            is_completed: row.get("isCompleted")?,
            num_coins: row.get("numCoins")?,
            seed: row.get("seed")?,
            streak: row.get("streak")?,
            th_date: datetime::date_column(row, "thDate")?,
        })
    }
}

/// `TreasureHuntDAO.find` (`LongKeyedDAO.find`).
pub fn find(db: &Connection, persona_id: i64) -> rusqlite::Result<Option<TreasureHuntEntity>> {
    db.prepare_cached("SELECT * FROM treasure_hunt WHERE personaId = ?1")?
        .query_row([persona_id], TreasureHuntEntity::from_row)
        .optional()
}

/// `TreasureHuntDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, th: &TreasureHuntEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE treasure_hunt SET coinsCollected = ?2, isStreakBroken = ?3, numCoins = ?4, seed = ?5, streak = ?6,
         thDate = ?7, isCompleted = ?8 WHERE personaId = ?1",
    )?
    .execute(params![
        th.persona_id,
        th.coins_collected,
        th.is_streak_broken,
        th.num_coins,
        th.seed,
        th.streak,
        th.th_date.map(datetime::date_to_sql),
        th.is_completed,
    ])?;
    Ok(())
}

/// `TreasureHuntDAO.insert` (`persist`).
pub fn insert(db: &Connection, th: &TreasureHuntEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO treasure_hunt (personaId, coinsCollected, isStreakBroken, numCoins, seed, streak, thDate, isCompleted)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?
    .execute(params![
        th.persona_id,
        th.coins_collected,
        th.is_streak_broken,
        th.num_coins,
        th.seed,
        th.streak,
        th.th_date.map(datetime::date_to_sql),
        th.is_completed,
    ])?;
    Ok(())
}

//! `TreasureHuntEntity` (a persona's daily treasure hunt) and `TreasureHuntDAO`.

// port: dao/TreasureHuntDAO#<init>()

use rusqlite::{Connection, params};
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

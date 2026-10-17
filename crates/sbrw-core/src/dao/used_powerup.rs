//! `UsedPowerupEntity` (a record of a powerup a persona used) and
//! `UsedPowerupDAO`.

// port: dao/UsedPowerupDAO#<init>()

use rusqlite::{Connection, params};
use time::PrimitiveDateTime;

use crate::datetime;

/// `UsedPowerupEntity`: its persona and event session by the ids they join on.
#[derive(Debug, Clone, PartialEq)]
pub struct UsedPowerupEntity {
    pub id: i64,
    pub persona_id: i64,
    pub event_session_id: Option<i64>,
    pub powerup_hash: Option<i32>,
}

/// `UsedPowerupDAO.insert` (`persist`); sets the generated id. The unmapped
/// `recorded_at` column defaults to MySQL's `CURRENT_TIMESTAMP`, which is
/// `now` here.
pub fn insert(db: &Connection, used: &mut UsedPowerupEntity, now: PrimitiveDateTime) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO used_powerup (personaId, eventSessionId, powerupHash, recorded_at) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![used.persona_id, used.event_session_id, used.powerup_hash, datetime::to_sql(now)])?;
    used.id = db.last_insert_rowid();
    Ok(())
}

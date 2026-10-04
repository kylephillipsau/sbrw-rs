//! `BanEntity` and `BanDAO`.

// port: dao/BanDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row};
use time::PrimitiveDateTime;

use crate::datetime;

#[derive(Debug, Clone, PartialEq)]
pub struct BanEntity {
    pub id: i64,
    pub user_id: Option<i64>,
    pub banned_by_id: Option<i64>,
    pub reason: Option<String>,
    pub started: Option<PrimitiveDateTime>,
    pub ends_at: Option<PrimitiveDateTime>,
    pub active: Option<bool>,
}

impl BanEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            user_id: row.get("user_id")?,
            banned_by_id: row.get("banned_by_id")?,
            reason: row.get("reason")?,
            started: datetime::column(row, "started")?,
            ends_at: datetime::column(row, "ends_at")?,
            active: row.get("active")?,
        })
    }
}

// port: dao/BanDAO#findByUser(UserEntity)
// port: jpa/BanEntity@BanEntity.findByUser
/// An active ban that hasn't ended. Upstream compares against MySQL's
/// `CURRENT_TIMESTAMP`; here that is the server clock, passed in as `now`.
pub fn find_by_user(db: &Connection, user_id: i64, now: PrimitiveDateTime) -> rusqlite::Result<Option<BanEntity>> {
    db.prepare_cached(
        "SELECT * FROM ban WHERE user_id = ?1 AND (ends_at IS NULL OR ends_at > ?2) AND active = 1 ORDER BY id LIMIT 1",
    )?
    .query_row(
        rusqlite::params![user_id, datetime::to_sql(now.replace_nanosecond(0).expect("0 ns"))],
        BanEntity::from_row,
    )
    .optional()
}

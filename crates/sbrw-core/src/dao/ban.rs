//! `BanEntity` and `BanDAO`.

// port: dao/BanDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::datetime;

#[derive(Debug, Clone, PartialEq, Default)]
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

// port: dao/BanDAO#findAllExpired()
// port: jpa/BanEntity@BanEntity.findAllExpired
/// Active bans with an end that has come.
pub fn find_all_expired(db: &Connection, now: PrimitiveDateTime) -> rusqlite::Result<Vec<BanEntity>> {
    db.prepare_cached("SELECT * FROM ban WHERE ends_at IS NOT NULL AND ends_at <= ?1 AND active = 1 ORDER BY id")?
        .query_map([datetime::to_sql(now.replace_nanosecond(0).expect("0 ns"))], BanEntity::from_row)?
        .collect()
}

/// `BanDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, b: &mut BanEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO ban (ends_at, reason, user_id, started, banned_by_id, active) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        datetime::opt_to_sql(b.ends_at),
        b.reason,
        b.user_id,
        datetime::opt_to_sql(b.started),
        b.banned_by_id,
        b.active
    ])?;
    b.id = db.last_insert_rowid();
    Ok(())
}

/// `BanDAO.update` (`merge`): every mapped column.
pub fn update(db: &Connection, b: &BanEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE ban SET ends_at = ?2, reason = ?3, user_id = ?4, started = ?5, banned_by_id = ?6, active = ?7
         WHERE id = ?1",
    )?
    .execute(params![
        b.id,
        datetime::opt_to_sql(b.ends_at),
        b.reason,
        b.user_id,
        datetime::opt_to_sql(b.started),
        b.banned_by_id,
        b.active
    ])?;
    Ok(())
}

//! `RecoveryPasswordEntity` (a password reset key sent by email) and
//! `RecoveryPasswordDAO`.

// port: dao/RecoveryPasswordDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::datetime;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct RecoveryPasswordEntity {
    pub id: i64,
    /// A `java.util.Date` in a `DATETIME` column.
    pub expiration_date: Option<PrimitiveDateTime>,
    pub is_close: Option<bool>,
    pub random_key: Option<String>,
    pub user_id: Option<i64>,
}

impl RecoveryPasswordEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            expiration_date: datetime::column(row, "expirationDate")?,
            is_close: row.get("isClose")?,
            random_key: row.get("randomKey")?,
            user_id: row.get("userId")?,
        })
    }
}

// port: dao/RecoveryPasswordDAO#findAllOpenByUserId(Long)
// port: jpa/RecoveryPasswordEntity@RecoveryPasswordEntity.findAllOpenByUserId
/// The user's keys that are neither used nor expired, `now` being the
/// `new Date()` the DAO binds, to the millisecond.
pub fn find_all_open_by_user_id(
    db: &Connection,
    user_id: i64,
    now: PrimitiveDateTime,
) -> rusqlite::Result<Vec<RecoveryPasswordEntity>> {
    db.prepare_cached(
        "SELECT * FROM recovery_password WHERE userId = ?1 AND isClose = 0 AND expirationDate || '.000' > ?2
         ORDER BY id",
    )?
    .query_map(params![user_id, datetime::to_sql_millis(now)], RecoveryPasswordEntity::from_row)?
    .collect()
}

// port: dao/RecoveryPasswordDAO#findByRandomKey(String)
// port: jpa/RecoveryPasswordEntity@RecoveryPasswordEntity.findByRandomKey
/// The first row with the key; keys match without regard to case, as in
/// MySQL's collation.
pub fn find_by_random_key(db: &Connection, random_key: &str) -> rusqlite::Result<Option<RecoveryPasswordEntity>> {
    db.prepare_cached("SELECT * FROM recovery_password WHERE randomKey = ?1 ORDER BY id LIMIT 1")?
        .query_row([random_key], RecoveryPasswordEntity::from_row)
        .optional()
}

/// `RecoveryPasswordDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, r: &mut RecoveryPasswordEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO recovery_password (expirationDate, isClose, randomKey, userId) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![datetime::opt_to_sql(r.expiration_date), r.is_close, r.random_key, r.user_id])?;
    r.id = db.last_insert_rowid();
    Ok(())
}

/// `RecoveryPasswordDAO.update` (`merge`): every mapped column.
pub fn update(db: &Connection, r: &RecoveryPasswordEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE recovery_password SET expirationDate = ?2, isClose = ?3, randomKey = ?4, userId = ?5 WHERE id = ?1",
    )?
    .execute(params![r.id, datetime::opt_to_sql(r.expiration_date), r.is_close, r.random_key, r.user_id])?;
    Ok(())
}

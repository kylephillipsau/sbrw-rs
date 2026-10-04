//! `OnlineUsersEntity` (one row of player counts per minute) and `OnlineUsersDAO`.

// port: dao/OnlineUsersDAO#<init>()

use rusqlite::{Connection, params};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OnlineUsersEntity {
    /// Unix seconds, truncated; the primary key.
    pub time_record: i32,
    pub number_of_online: i64,
    pub number_of_registered: i64,
}

/// `OnlineUsersDAO.insert` (`persist`).
pub fn insert(db: &Connection, row: &OnlineUsersEntity) -> rusqlite::Result<()> {
    db.prepare_cached("INSERT INTO online_users (ID, numberOfOnline, numberOfRegistered) VALUES (?1, ?2, ?3)")?
        .execute(params![row.time_record, row.number_of_online, row.number_of_registered])?;
    Ok(())
}

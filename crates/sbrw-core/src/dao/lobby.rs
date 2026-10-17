//! `LobbyEntity` (a multiplayer lobby). The lobbies themselves arrive with
//! multiplayer; event sessions read one through their `lobby`.

use rusqlite::{Connection, OptionalExtension, Row};
use time::PrimitiveDateTime;

use crate::datetime;

/// `LobbyEntity`, without its EAGER entrants: nothing that reads a lobby yet
/// looks at them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LobbyEntity {
    pub id: i64,
    pub event_id: Option<i32>,
    pub started_time: Option<PrimitiveDateTime>,
    pub is_private: Option<bool>,
    pub persona_id: Option<i64>,
}

impl LobbyEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            event_id: row.get("EVENTID")?,
            started_time: datetime::column(row, "startedTime")?,
            is_private: row.get("isPrivate")?,
            persona_id: row.get("personaId")?,
        })
    }
}

/// `LobbyDAO.find` (`LongKeyedDAO.find`).
pub fn find(db: &Connection, id: i64) -> rusqlite::Result<Option<LobbyEntity>> {
    db.prepare_cached("SELECT * FROM lobby WHERE ID = ?1")?.query_row([id], LobbyEntity::from_row).optional()
}

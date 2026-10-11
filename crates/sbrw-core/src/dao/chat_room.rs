//! `ChatRoomEntity` (a public chat channel group) and `ChatRoomDAO`. Static data.

// port: dao/ChatRoomDAO#<init>()

use rusqlite::Connection;

#[derive(Debug, Clone, PartialEq)]
pub struct ChatRoomEntity {
    pub id: i64,
    pub long_name: Option<String>,
    pub short_name: Option<String>,
    pub amount: Option<i32>,
}

// port: dao/ChatRoomDAO#findAll()
// port: jpa/ChatRoomEntity@ChatRoomEntity.findAll
/// In primary-key order, as MySQL returns the unordered query.
pub fn find_all(db: &Connection) -> rusqlite::Result<Vec<ChatRoomEntity>> {
    let mut stmt = db.prepare_cached("SELECT ID, longName, shortName, amount FROM chat_room ORDER BY ID")?;
    stmt.query_map([], |r| {
        Ok(ChatRoomEntity { id: r.get(0)?, long_name: r.get(1)?, short_name: r.get(2)?, amount: r.get(3)? })
    })?
    .collect()
}

//! `LevelRepEntity` (rep needed for each level) and the parts of `LevelRepDAO`
//! ported so far. Static data.

// port: dao/LevelRepDAO#<init>()

use rusqlite::{Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq)]
pub struct LevelRepEntity {
    pub level: i64,
    pub exp_point: Option<i64>,
}

// port: dao/LevelRepDAO#findAll()
// port: jpa/LevelRepEntity@LevelRepEntity.findAll
/// Every level, in primary-key order as MySQL returns the unordered query.
pub fn find_all(db: &Connection) -> rusqlite::Result<Vec<LevelRepEntity>> {
    let mut stmt = db.prepare_cached("SELECT level, expPoint FROM level_rep ORDER BY level")?;
    stmt.query_map([], |r| Ok(LevelRepEntity { level: r.get(0)?, exp_point: r.get(1)? }))?.collect()
}

/// `LevelRepDAO.find` (`LongKeyedDAO.find`): the level's row, by its level.
pub fn find(db: &Connection, level: i64) -> rusqlite::Result<Option<LevelRepEntity>> {
    db.prepare_cached("SELECT level, expPoint FROM level_rep WHERE level = ?1")?
        .query_row([level], |r| Ok(LevelRepEntity { level: r.get(0)?, exp_point: r.get(1)? }))
        .optional()
}

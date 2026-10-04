//! `HardwareInfoEntity` and the parts of `HardwareInfoDAO` ported so far.

// port: dao/HardwareInfoDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HardwareInfoEntity {
    pub id: i64,
    pub hardware_info: Option<String>,
    pub hardware_hash: Option<String>,
    pub user_id: Option<i64>,
    pub banned: bool,
}

impl HardwareInfoEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            hardware_info: row.get("hardwareInfo")?,
            hardware_hash: row.get("hardwareHash")?,
            user_id: row.get("userId")?,
            banned: row.get::<_, Option<bool>>("banned")?.unwrap_or(false),
        })
    }
}

// port: dao/HardwareInfoDAO#findByHardwareHash(String)
// port: jpa/HardwareInfoEntity@HardwareInfoEntity.findByHardwareHash
pub fn find_by_hardware_hash(db: &Connection, hash: &str) -> rusqlite::Result<Option<HardwareInfoEntity>> {
    db.prepare_cached("SELECT * FROM hardware_info WHERE hardwareHash = ?1 ORDER BY ID LIMIT 1")?
        .query_row([hash], HardwareInfoEntity::from_row)
        .optional()
}

//! `BadgeDefinitionEntity` (static badge artwork and text) and
//! `BadgeDefinitionDAO`.

// port: dao/BadgeDefinitionDAO#<init>()

use rusqlite::{Connection, Row};

/// `BadgeDefinitionEntity`. Its achievement (the EAGER inverse side of
/// `achievement.badge_definition_id`) is held by id.
#[derive(Debug, Clone, PartialEq)]
pub struct BadgeDefinitionEntity {
    pub id: i64,
    pub background: Option<String>,
    pub border: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub name: Option<String>,
    pub achievement_id: Option<i64>,
}

/// Selects a badge definition's columns, prefixed with `b_`, and its
/// achievement's id as `b_achievement_id`, from `badge_definition b`.
pub(crate) const COLUMNS: &str = "b.ID AS b_ID, b.background AS b_background, b.border AS b_border,
    b.description AS b_description, b.icon AS b_icon, b.name AS b_name,
    (SELECT a.ID FROM achievement a WHERE a.badge_definition_id = b.ID) AS b_achievement_id";

impl BadgeDefinitionEntity {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("b_ID")?,
            background: row.get("b_background")?,
            border: row.get("b_border")?,
            description: row.get("b_description")?,
            icon: row.get("b_icon")?,
            name: row.get("b_name")?,
            achievement_id: row.get("b_achievement_id")?,
        })
    }
}

// port: dao/BadgeDefinitionDAO#findAll()
// port: jpa/BadgeDefinitionEntity@BadgeDefinitionEntity.findAll
pub fn find_all(db: &Connection) -> rusqlite::Result<Vec<BadgeDefinitionEntity>> {
    let mut stmt = db.prepare_cached(&format!("SELECT {COLUMNS} FROM badge_definition b ORDER BY b.ID"))?;
    stmt.query_map([], BadgeDefinitionEntity::from_row)?.collect()
}

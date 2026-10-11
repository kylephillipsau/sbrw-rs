//! `PersonaBadgeEntity` (a badge a persona shows in a slot) and the parts of
//! `PersonaBadgeDAO` ported so far.

// port: dao/PersonaBadgeDAO#<init>()

use rusqlite::Connection;

use crate::dao::badge_definition::{self, BadgeDefinitionEntity};

/// `PersonaBadgeEntity`, with its EAGER badge definition.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaBadgeEntity {
    pub id: i64,
    pub persona_id: i64,
    pub badge_definition: BadgeDefinitionEntity,
    pub slot: Option<i32>,
}

// port: dao/PersonaBadgeDAO#findAllBadgesForPersona(Long)
// port: jpa/PersonaBadgeEntity@PersonaBadgeEntity.findAllBadgesForPersona
/// In primary-key order, as MySQL returns them through the persona index.
pub fn find_all_badges_for_persona(db: &Connection, persona_id: i64) -> rusqlite::Result<Vec<PersonaBadgeEntity>> {
    let mut stmt = db.prepare_cached(&format!(
        "SELECT pb.ID, pb.persona_id, pb.slot, {} FROM persona_badge pb
         JOIN badge_definition b ON b.ID = pb.badge_definition_id
         WHERE pb.persona_id = ?1 ORDER BY pb.ID",
        badge_definition::COLUMNS
    ))?;
    stmt.query_map([persona_id], |row| {
        Ok(PersonaBadgeEntity {
            id: row.get("ID")?,
            persona_id: row.get("persona_id")?,
            badge_definition: BadgeDefinitionEntity::from_row(row)?,
            slot: row.get("slot")?,
        })
    })?
    .collect()
}

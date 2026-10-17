//! `PersonaBadgeEntity` (a badge a persona shows in a slot) and
//! `PersonaBadgeDAO`.

// port: dao/PersonaBadgeDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};

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
    stmt.query_map([persona_id], from_row)?.collect()
}

fn from_row(row: &Row) -> rusqlite::Result<PersonaBadgeEntity> {
    Ok(PersonaBadgeEntity {
        id: row.get("ID")?,
        persona_id: row.get("persona_id")?,
        badge_definition: BadgeDefinitionEntity::from_row(row)?,
        slot: row.get("slot")?,
    })
}

// port: dao/PersonaBadgeDAO#findBadgeInSlotForPersona(Long,Integer)
// port: jpa/PersonaBadgeEntity@PersonaBadgeEntity.findBadgeInSlotForPersona
/// The first badge the persona has in the slot.
pub fn find_badge_in_slot_for_persona(
    db: &Connection,
    persona_id: i64,
    slot: i32,
) -> rusqlite::Result<Option<PersonaBadgeEntity>> {
    db.prepare_cached(&format!(
        "SELECT pb.ID, pb.persona_id, pb.slot, {} FROM persona_badge pb
         JOIN badge_definition b ON b.ID = pb.badge_definition_id
         WHERE pb.persona_id = ?1 AND pb.slot = ?2 ORDER BY pb.ID LIMIT 1",
        badge_definition::COLUMNS
    ))?
    .query_row(params![persona_id, slot], from_row)
    .optional()
}

/// `PersonaBadgeDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, b: &mut PersonaBadgeEntity) -> rusqlite::Result<()> {
    db.prepare_cached("INSERT INTO persona_badge (slot, badge_definition_id, persona_id) VALUES (?1, ?2, ?3)")?
        .execute(params![b.slot, b.badge_definition.id, b.persona_id])?;
    b.id = db.last_insert_rowid();
    Ok(())
}

/// `PersonaBadgeDAO.update` (`merge`): every mapped column.
pub fn update(db: &Connection, b: &PersonaBadgeEntity) -> rusqlite::Result<()> {
    db.prepare_cached("UPDATE persona_badge SET slot = ?2, badge_definition_id = ?3, persona_id = ?4 WHERE ID = ?1")?
        .execute(params![b.id, b.slot, b.badge_definition.id, b.persona_id])?;
    Ok(())
}

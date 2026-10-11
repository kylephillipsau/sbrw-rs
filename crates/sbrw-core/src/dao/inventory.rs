//! `InventoryEntity` (a persona's item capacities and items) and the parts of
//! `InventoryDAO` ported so far.

// port: dao/InventoryDAO#<init>()

use rusqlite::{Connection, params};

use crate::dao::inventory_item::InventoryItemEntity;

/// `InventoryEntity`. `Default` is `new InventoryEntity()`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InventoryEntity {
    pub id: i64,
    pub persona_id: Option<i64>,
    /// EAGER.
    pub inventory_items: Vec<InventoryItemEntity>,
    pub performance_parts_capacity: i32,
    pub performance_parts_used_slot_count: i32,
    pub skill_mod_parts_capacity: i32,
    pub skill_mod_parts_used_slot_count: i32,
    pub visual_parts_capacity: i32,
    pub visual_parts_used_slot_count: i32,
}

/// `InventoryDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, inv: &mut InventoryEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO inventory (performancePartsCapacity, performancePartsUsedSlotCount, skillModPartsCapacity,
         skillModPartsUsedSlotCount, visualPartsCapacity, visualPartsUsedSlotCount, personaId)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        inv.performance_parts_capacity,
        inv.performance_parts_used_slot_count,
        inv.skill_mod_parts_capacity,
        inv.skill_mod_parts_used_slot_count,
        inv.visual_parts_capacity,
        inv.visual_parts_used_slot_count,
        inv.persona_id,
    ])?;
    inv.id = db.last_insert_rowid();
    Ok(())
}

/// `InventoryDAO.update` (`merge`), and what Hibernate flushes for a changed
/// managed inventory: every mapped column, from this copy.
pub fn update(db: &Connection, inv: &InventoryEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE inventory SET performancePartsCapacity = ?2, performancePartsUsedSlotCount = ?3,
         skillModPartsCapacity = ?4, skillModPartsUsedSlotCount = ?5, visualPartsCapacity = ?6,
         visualPartsUsedSlotCount = ?7, personaId = ?8 WHERE id = ?1",
    )?
    .execute(params![
        inv.id,
        inv.performance_parts_capacity,
        inv.performance_parts_used_slot_count,
        inv.skill_mod_parts_capacity,
        inv.skill_mod_parts_used_slot_count,
        inv.visual_parts_capacity,
        inv.visual_parts_used_slot_count,
        inv.persona_id,
    ])?;
    Ok(())
}

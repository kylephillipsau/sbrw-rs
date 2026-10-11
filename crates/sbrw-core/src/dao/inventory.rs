//! `InventoryEntity` (a persona's item capacities and items) and the parts of
//! `InventoryDAO` ported so far.

// port: dao/InventoryDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::dao::inventory_item::{self, InventoryItemEntity};

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

impl InventoryEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            persona_id: row.get("personaId")?,
            inventory_items: Vec::new(),
            performance_parts_capacity: row.get("performancePartsCapacity")?,
            performance_parts_used_slot_count: row.get("performancePartsUsedSlotCount")?,
            skill_mod_parts_capacity: row.get("skillModPartsCapacity")?,
            skill_mod_parts_used_slot_count: row.get("skillModPartsUsedSlotCount")?,
            visual_parts_capacity: row.get("visualPartsCapacity")?,
            visual_parts_used_slot_count: row.get("visualPartsUsedSlotCount")?,
        })
    }
}

// port: dao/InventoryDAO#findByPersonaId(Long)
// port: jpa/InventoryEntity@InventoryEntity.findByPersonaId
/// The persona's inventory, with its items (EAGER).
pub fn find_by_persona_id(db: &Connection, persona_id: i64) -> rusqlite::Result<Option<InventoryEntity>> {
    let Some(mut inventory) = db
        .prepare_cached("SELECT * FROM inventory WHERE personaId = ?1 ORDER BY id LIMIT 1")?
        .query_row([persona_id], InventoryEntity::from_row)
        .optional()?
    else {
        return Ok(None);
    };
    inventory.inventory_items = inventory_item::find_by_inventory_id(db, inventory.id)?;
    Ok(Some(inventory))
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

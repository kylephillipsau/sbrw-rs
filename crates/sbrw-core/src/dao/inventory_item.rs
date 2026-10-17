//! `InventoryItemEntity` (one item a persona owns) and the parts of
//! `InventoryItemDAO` ported so far.

// port: dao/InventoryItemDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::datetime;

/// `InventoryItemEntity`. Its product is held by `productId`, the column the
/// `@ManyToOne` joins on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InventoryItemEntity {
    pub id: i64,
    pub inventory_id: i64,
    pub expiration_date: Option<PrimitiveDateTime>,
    pub product_id: String,
    pub remaining_use_count: i32,
    pub resell_price: i32,
    pub status: Option<String>,
}

impl InventoryItemEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            inventory_id: row.get("inventoryEntity_id")?,
            expiration_date: datetime::column(row, "expirationDate")?,
            product_id: row.get("productId")?,
            remaining_use_count: row.get("remainingUseCount")?,
            resell_price: row.get("resellPrice")?,
            status: row.get("status")?,
        })
    }
}

// port: dao/InventoryItemDAO#findAllByPersonaIdAndEntitlementTag(Long,String)
// port: jpa/InventoryItemEntity@InventoryItemEntity.findAllByPersonaIdAndTag
/// The persona's items of products with this entitlement tag.
pub fn find_all_by_persona_id_and_entitlement_tag(
    db: &Connection,
    persona_id: i64,
    entitlement_tag: &str,
) -> rusqlite::Result<Vec<InventoryItemEntity>> {
    db.prepare_cached(
        "SELECT ii.* FROM inventory_item ii JOIN inventory i ON i.id = ii.inventoryEntity_id
         JOIN product p ON p.productId = ii.productId
         WHERE i.personaId = ?1 AND p.entitlementTag = ?2 ORDER BY ii.id",
    )?
    .query_map(rusqlite::params![persona_id, entitlement_tag], InventoryItemEntity::from_row)?
    .collect()
}

// port: dao/InventoryItemDAO#findByPersonaIdAndEntitlementTag(Long,String)
/// The first of the persona's items with this entitlement tag.
pub fn find_by_persona_id_and_entitlement_tag(
    db: &Connection,
    persona_id: i64,
    entitlement_tag: &str,
) -> rusqlite::Result<Option<InventoryItemEntity>> {
    Ok(find_all_by_persona_id_and_entitlement_tag(db, persona_id, entitlement_tag)?.into_iter().next())
}

// port: dao/InventoryItemDAO#findByInventoryIdAndEntitlementTag(Long,String)
// port: jpa/InventoryItemEntity@InventoryItemEntity.findAllByInventoryAndTag
/// The first item in the inventory with this entitlement tag; a null tag
/// matches nothing.
pub fn find_by_inventory_id_and_entitlement_tag(
    db: &Connection,
    inventory_id: i64,
    entitlement_tag: Option<&str>,
) -> rusqlite::Result<Option<InventoryItemEntity>> {
    db.prepare_cached(
        "SELECT ii.* FROM inventory_item ii JOIN product p ON p.productId = ii.productId
         WHERE ii.inventoryEntity_id = ?1 AND p.entitlementTag = ?2 ORDER BY ii.id LIMIT 1",
    )?
    .query_row(rusqlite::params![inventory_id, entitlement_tag], InventoryItemEntity::from_row)
    .optional()
}

/// `InventoryItemDAO.find`.
pub fn find(db: &Connection, id: i64) -> rusqlite::Result<Option<InventoryItemEntity>> {
    db.prepare_cached("SELECT * FROM inventory_item WHERE id = ?1")?
        .query_row([id], InventoryItemEntity::from_row)
        .optional()
}

/// `InventoryEntity.inventoryItems`: a bag with no `@OrderBy`, which MySQL
/// reads through the foreign key's index, so in id order.
pub(crate) fn find_by_inventory_id(db: &Connection, inventory_id: i64) -> rusqlite::Result<Vec<InventoryItemEntity>> {
    db.prepare_cached("SELECT * FROM inventory_item WHERE inventoryEntity_id = ?1 ORDER BY id")?
        .query_map([inventory_id], InventoryItemEntity::from_row)?
        .collect()
}

/// `InventoryItemDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, item: &mut InventoryItemEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO inventory_item (expirationDate, remainingUseCount, resellPrice, status, inventoryEntity_id, productId)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?
    .execute(params![
        datetime::opt_to_sql(item.expiration_date),
        item.remaining_use_count,
        item.resell_price,
        item.status,
        item.inventory_id,
        item.product_id,
    ])?;
    item.id = db.last_insert_rowid();
    Ok(())
}

/// `InventoryItemDAO.update` (`merge`), and what Hibernate flushes for a
/// changed managed item: every mapped column, from this copy.
pub fn update(db: &Connection, item: &InventoryItemEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE inventory_item SET expirationDate = ?2, remainingUseCount = ?3, resellPrice = ?4, status = ?5,
         inventoryEntity_id = ?6, productId = ?7 WHERE id = ?1",
    )?
    .execute(params![
        item.id,
        datetime::opt_to_sql(item.expiration_date),
        item.remaining_use_count,
        item.resell_price,
        item.status,
        item.inventory_id,
        item.product_id,
    ])?;
    Ok(())
}

/// `InventoryItemDAO.delete`.
pub fn delete(db: &Connection, item: &InventoryItemEntity) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM inventory_item WHERE id = ?1")?.execute([item.id])?;
    Ok(())
}

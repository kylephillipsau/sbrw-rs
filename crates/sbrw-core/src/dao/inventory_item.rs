//! `InventoryItemEntity` (one item a persona owns) and the parts of
//! `InventoryItemDAO` ported so far.

// port: dao/InventoryItemDAO#<init>()

use rusqlite::{Connection, params};
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

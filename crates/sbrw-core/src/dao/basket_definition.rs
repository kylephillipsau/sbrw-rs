//! `BasketDefinitionEntity` (the car a car product gives, static data) and
//! `BasketDefinitionDAO`.

// port: dao/BasketDefinitionDAO#<init>()

use rusqlite::{Connection, OptionalExtension};

/// `BasketDefinitionEntity`: a product id and its car as `OwnedCarTrans` XML.
#[derive(Debug, Clone, PartialEq)]
pub struct BasketDefinitionEntity {
    pub product_id: String,
    pub owned_car_trans: Option<String>,
}

/// `BasketDefinitionDAO.find(productId)`.
pub fn find(db: &Connection, product_id: &str) -> rusqlite::Result<Option<BasketDefinitionEntity>> {
    db.prepare_cached("SELECT productId, ownedCarTrans FROM basketdefinition WHERE productId = ?1")?
        .query_row([product_id], |row| {
            Ok(BasketDefinitionEntity { product_id: row.get(0)?, owned_car_trans: row.get(1)? })
        })
        .optional()
}

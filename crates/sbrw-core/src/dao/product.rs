//! `ProductEntity` (anything the game sells or awards) and the parts of
//! `ProductDAO` ported so far. Static data.

// port: dao/ProductDAO#<init>()

use anyhow::anyhow;
use rusqlite::{Connection, Row};

use crate::Fault;

/// `ProductEntity`: every mapped column. `bundleItems` is the inverse side of
/// `parentProductId`, not a column; the table's `bundleItems`, `rewardTitle`
/// and `isGift` columns aren't mapped.
#[derive(Debug, Clone, PartialEq)]
pub struct ProductEntity {
    pub id: i64,
    pub parent_product_id: Option<i64>,
    pub category_id: Option<String>,
    pub currency: String,
    pub description: Option<String>,
    pub duration_minute: i32,
    pub hash: Option<i32>,
    pub icon: String,
    pub level: i32,
    pub long_description: Option<String>,
    pub price: f32,
    pub resale_price: f32,
    pub priority: i32,
    pub product_id: String,
    pub product_title: Option<String>,
    pub product_type: String,
    pub secondary_icon: Option<String>,
    pub use_count: i32,
    pub visual_style: Option<String>,
    pub web_icon: Option<String>,
    pub web_location: Option<String>,
    pub category_name: Option<String>,
    pub enabled: bool,
    pub min_level: i32,
    pub premium: bool,
    pub is_dropable: bool,
    pub top_speed: Option<i32>,
    pub accel: Option<i32>,
    pub handling: Option<i32>,
    pub skill_value: Option<f32>,
    pub entitlement_tag: Option<String>,
    pub sub_type: Option<String>,
    pub brand: Option<String>,
    pub rarity: Option<i32>,
    pub drop_weight: Option<f64>,
}

impl ProductEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let float = |name: &str| row.get::<_, f64>(name).map(|v| v as f32);
        Ok(Self {
            id: row.get("id")?,
            parent_product_id: row.get("parentProductId")?,
            category_id: row.get("categoryId")?,
            currency: row.get("currency")?,
            description: row.get("description")?,
            duration_minute: row.get("durationMinute")?,
            hash: row.get("hash")?,
            icon: row.get("icon")?,
            level: row.get("level")?,
            long_description: row.get("longDescription")?,
            price: float("price")?,
            resale_price: float("resalePrice")?,
            priority: row.get("priority")?,
            product_id: row.get("productId")?,
            product_title: row.get("productTitle")?,
            product_type: row.get("productType")?,
            secondary_icon: row.get("secondaryIcon")?,
            use_count: row.get("useCount")?,
            visual_style: row.get("visualStyle")?,
            web_icon: row.get("webIcon")?,
            web_location: row.get("webLocation")?,
            category_name: row.get("categoryName")?,
            enabled: row.get("enabled")?,
            min_level: row.get("minLevel")?,
            premium: row.get("premium")?,
            is_dropable: row.get("isDropable")?,
            top_speed: row.get("topSpeed")?,
            accel: row.get("accel")?,
            handling: row.get("handling")?,
            skill_value: row.get::<_, Option<f64>>("skillValue")?.map(|v| v as f32),
            entitlement_tag: row.get("entitlementTag")?,
            sub_type: row.get("subType")?,
            brand: row.get("brand")?,
            rarity: row.get("rarity")?,
            drop_weight: row.get("dropWeight")?,
        })
    }
}

// port: dao/ProductDAO#findByProductId(String)
// port: jpa/ProductEntity@ProductEntity.findByProductId
/// The first product with this id. Unlike most finders, a missing product
/// throws (a `RuntimeException`) rather than returning null.
pub fn find_by_product_id(db: &Connection, product_id: &str) -> Result<ProductEntity, Fault> {
    let mut stmt = db.prepare_cached("SELECT * FROM product WHERE productId = ?1 ORDER BY id LIMIT 1")?;
    let found = stmt.query_map([product_id], ProductEntity::from_row)?.next().transpose()?;
    found.ok_or_else(|| Fault::Internal(anyhow!("Could not find product with ID: {product_id}")))
}

// port: dao/ProductDAO#findByEntitlementTag(String)
// port: jpa/ProductEntity@ProductEntity.findByEntitlementTag
/// The first product with this entitlement tag; a missing one throws.
pub fn find_by_entitlement_tag(db: &Connection, entitlement_tag: &str) -> Result<ProductEntity, Fault> {
    let mut stmt = db.prepare_cached("SELECT * FROM product WHERE entitlementTag = ?1 ORDER BY id LIMIT 1")?;
    let found = stmt.query_map([entitlement_tag], ProductEntity::from_row)?.next().transpose()?;
    found.ok_or_else(|| Fault::Internal(anyhow!("Could not find product with tag: {entitlement_tag}")))
}

// port: dao/ProductDAO#findByHash(Integer)
// port: jpa/ProductEntity@ProductEntity.findByHash
/// The first product with this hash; a missing one throws.
pub fn find_by_hash(db: &Connection, hash: i32) -> Result<ProductEntity, Fault> {
    let mut stmt = db.prepare_cached("SELECT * FROM product WHERE hash = ?1 ORDER BY id LIMIT 1")?;
    let found = stmt.query_map([hash], ProductEntity::from_row)?.next().transpose()?;
    found.ok_or_else(|| Fault::Internal(anyhow!("Could not find product with hash: {hash}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::store;

    #[test]
    fn finds_starting_items_and_throws_for_unknown_ones() {
        let db = store::open(None, &BTreeMap::new()).unwrap();
        let powerup = find_by_product_id(&db, "SRV-POWERUP0").unwrap();
        assert_eq!(powerup.product_type, "POWERUP");
        assert_eq!(powerup.duration_minute, 0);
        let insurance = find_by_product_id(&db, "srv-amp-insurance").unwrap();
        assert_eq!(insurance.product_id, "SRV-AMP-INSURANCE");
        assert_eq!(insurance.duration_minute, 10080);
        let missing = find_by_product_id(&db, "NOPE").unwrap_err();
        assert_eq!(missing.to_string(), "Could not find product with ID: NOPE");
    }
}

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

// port: dao/ProductDAO#findByLevelEnabled(String,String,int,boolean,boolean)
// port: jpa/ProductEntity@ProductEntity.findByLevelEnabled
/// A category's products of one type up to a level, enabled or not, and
/// premium ones only for `premium`. The query has no order; MySQL scans the
/// table, so the rows come in key order (`commerce/catalog`, where a product
/// moved to level 60 keeps its place).
pub fn find_by_level_enabled(
    db: &Connection,
    category_name: &str,
    product_type: &str,
    min_level: i32,
    enabled: bool,
    premium: bool,
) -> rusqlite::Result<Vec<ProductEntity>> {
    db.prepare_cached(
        "SELECT * FROM product WHERE enabled = ?3 AND minLevel <= ?4 AND (premium = 0 OR premium = ?5)
         AND categoryName = ?1 AND productType = ?2 ORDER BY id",
    )?
    .query_map(rusqlite::params![category_name, product_type, enabled, min_level, premium], ProductEntity::from_row)?
    .collect()
}

// port: dao/ProductDAO#findByTraits(String,String,String,Integer)
/// The products matching each trait given (category, type, sub-type,
/// rarity); every product when none is. A criteria query with no order, which
/// MySQL answers by scanning the table, so in key order.
pub fn find_by_traits(
    db: &Connection,
    category: Option<&str>,
    product_type: Option<&str>,
    sub_type: Option<&str>,
    rating: Option<i32>,
) -> rusqlite::Result<Vec<ProductEntity>> {
    db.prepare_cached(
        "SELECT * FROM product WHERE (?1 IS NULL OR categoryName = ?1) AND (?2 IS NULL OR productType = ?2)
         AND (?3 IS NULL OR subType = ?3) AND (?4 IS NULL OR rarity = ?4) ORDER BY id",
    )?
    .query_map(rusqlite::params![category, product_type, sub_type, rating], ProductEntity::from_row)?
    .collect()
}

// port: dao/ProductDAO#findDropsByType(String)
// port: jpa/ProductEntity@ProductEntity.findDropsByType
/// The enabled, droppable products of a type with a positive drop weight,
/// in key order.
pub fn find_drops_by_type(db: &Connection, product_type: &str) -> rusqlite::Result<Vec<ProductEntity>> {
    db.prepare_cached(
        "SELECT * FROM product WHERE productType = ?1 AND isDropable = 1 AND enabled = 1 AND dropWeight IS NOT NULL
         AND dropWeight > 0.0 ORDER BY id",
    )?
    .query_map([product_type], ProductEntity::from_row)?
    .collect()
}

/// `ProductEntity.getBundleItems()`: the products whose parent this is. The
/// set's order is Hibernate's; no product in the data has a parent.
pub fn find_bundle_items(db: &Connection, parent_id: i64) -> rusqlite::Result<Vec<ProductEntity>> {
    db.prepare_cached("SELECT * FROM product WHERE parentProductId = ?1 ORDER BY id")?
        .query_map([parent_id], ProductEntity::from_row)?
        .collect()
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

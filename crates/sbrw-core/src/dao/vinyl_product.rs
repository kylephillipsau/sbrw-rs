//! `VinylProductEntity` (a vinyl the shop sells, in a category) and
//! `VinylProductDAO`. Static data.

// port: dao/VinylProductDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row};

/// `VinylProductEntity`: every mapped column; its category is the
/// `parentCategoryId` it joins on.
#[derive(Debug, Clone, PartialEq)]
pub struct VinylProductEntity {
    pub id: i64,
    pub bundle_items: Option<String>,
    pub category_id: Option<String>,
    pub currency: Option<String>,
    pub description: Option<String>,
    pub duration_minute: i32,
    pub hash: Option<i32>,
    pub icon: Option<String>,
    pub level: i32,
    pub long_description: Option<String>,
    pub price: f32,
    pub priority: i32,
    pub product_id: Option<String>,
    pub product_title: Option<String>,
    pub product_type: Option<String>,
    pub secondary_icon: Option<String>,
    pub use_count: i32,
    pub visual_style: Option<String>,
    pub web_icon: Option<String>,
    pub web_location: Option<String>,
    pub parent_category_id: Option<i64>,
    pub category_name: Option<String>,
    pub enabled: bool,
    pub min_level: i32,
    pub premium: bool,
    pub entitlement_tag: Option<String>,
}

impl VinylProductEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            bundle_items: row.get("bundleItems")?,
            category_id: row.get("categoryId")?,
            currency: row.get("currency")?,
            description: row.get("description")?,
            duration_minute: row.get("durationMinute")?,
            hash: row.get("hash")?,
            icon: row.get("icon")?,
            level: row.get("level")?,
            long_description: row.get("longDescription")?,
            price: row.get::<_, f64>("price")? as f32,
            priority: row.get("priority")?,
            product_id: row.get("productId")?,
            product_title: row.get("productTitle")?,
            product_type: row.get("productType")?,
            secondary_icon: row.get("secondaryIcon")?,
            use_count: row.get("useCount")?,
            visual_style: row.get("visualStyle")?,
            web_icon: row.get("webIcon")?,
            web_location: row.get("webLocation")?,
            parent_category_id: row.get("parentCategoryId")?,
            category_name: row.get("categoryName")?,
            enabled: row.get("enabled")?,
            min_level: row.get("minLevel")?,
            premium: row.get("premium")?,
            entitlement_tag: row.get("entitlementTag")?,
        })
    }
}

// port: dao/VinylProductDAO#findByHash(Integer)
// port: jpa/VinylProductEntity@VinylProductEntity.findByHash
/// The first vinyl with this hash, or none.
pub fn find_by_hash(db: &Connection, hash: i32) -> rusqlite::Result<Option<VinylProductEntity>> {
    db.prepare_cached("SELECT * FROM vinylproduct WHERE hash = ?1 ORDER BY id LIMIT 1")?
        .query_row([hash], VinylProductEntity::from_row)
        .optional()
}

// port: dao/VinylProductDAO#findByCategoryLevelEnabled(CategoryEntity,int,Boolean,Boolean)
// port: jpa/VinylProductEntity@VinylProductEntity.findByCategoryLevelEnabled
/// A category's vinyls up to a level, enabled or not, and premium ones only
/// for `premium`, in key order within the category.
pub fn find_by_category_level_enabled(
    db: &Connection,
    category_id: i64,
    min_level: i32,
    enabled: bool,
    premium: bool,
) -> rusqlite::Result<Vec<VinylProductEntity>> {
    db.prepare_cached(
        "SELECT * FROM vinylproduct WHERE parentCategoryId = ?1 AND ?2 >= minLevel
         AND (premium = 0 OR premium = ?4) AND enabled = ?3 ORDER BY id",
    )?
    .query_map(rusqlite::params![category_id, min_level, enabled, premium], VinylProductEntity::from_row)?
    .collect()
}

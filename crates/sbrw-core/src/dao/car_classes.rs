//! `CarClassesEntity` (a car model's maker, stock stats and class hash, static
//! data) and the parts of `CarClassesDAO` ported so far.

// port: dao/CarClassesDAO#<init>()
// port: dao/util/StringKeyedDAO#<init>(Class)

use rusqlite::{Connection, OptionalExtension, Row};

/// `CarClassesEntity`, keyed by `store_name`, the car's name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CarClassesEntity {
    pub store_name: String,
    pub product_id: Option<String>,
    pub hash: Option<i32>,
    pub full_name: Option<String>,
    pub manufactor: Option<String>,
    pub model: Option<String>,
    pub ts_stock: Option<i32>,
    pub ts_var1: Option<i32>,
    pub ts_var2: Option<i32>,
    pub ts_var3: Option<i32>,
    pub ac_stock: Option<i32>,
    pub ac_var1: Option<i32>,
    pub ac_var2: Option<i32>,
    pub ac_var3: Option<i32>,
    pub ha_stock: Option<i32>,
    pub ha_var1: Option<i32>,
    pub ha_var2: Option<i32>,
    pub ha_var3: Option<i32>,
}

impl CarClassesEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            store_name: row.get("store_name")?,
            product_id: row.get("product_id")?,
            hash: row.get("hash")?,
            full_name: row.get("full_name")?,
            manufactor: row.get("manufactor")?,
            model: row.get("model")?,
            ts_stock: row.get("ts_stock")?,
            ts_var1: row.get("ts_var1")?,
            ts_var2: row.get("ts_var2")?,
            ts_var3: row.get("ts_var3")?,
            ac_stock: row.get("ac_stock")?,
            ac_var1: row.get("ac_var1")?,
            ac_var2: row.get("ac_var2")?,
            ac_var3: row.get("ac_var3")?,
            ha_stock: row.get("ha_stock")?,
            ha_var1: row.get("ha_var1")?,
            ha_var2: row.get("ha_var2")?,
            ha_var3: row.get("ha_var3")?,
        })
    }
}

// port: dao/util/StringKeyedDAO#find(String)
/// `CarClassesDAO.find(name)`: by store name, compared as MySQL's collation does.
pub fn find(db: &Connection, store_name: &str) -> rusqlite::Result<Option<CarClassesEntity>> {
    db.prepare_cached("SELECT * FROM car_classes WHERE store_name = ?1")?
        .query_row([store_name], CarClassesEntity::from_row)
        .optional()
}

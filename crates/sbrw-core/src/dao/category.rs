//! `CategoryEntity` (a shop category, which holds vinyls) and `CategoryDAO`.
//! Static data.

// port: dao/CategoryDAO#<init>()

use rusqlite::{Connection, Row};

/// `CategoryEntity`. Its id is `idcategory`; the `id` column is a separate
/// value (`getIdentifiant()`). `listOfVinyls` is the inverse side of a
/// vinyl's category, not a column.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CategoryEntity {
    pub idcategory: i64,
    pub catalog_version: Option<String>,
    pub categories: Option<String>,
    pub display_name: Option<String>,
    pub filter_type: Option<i32>,
    pub icon: Option<String>,
    pub identifiant: Option<i64>,
    pub long_description: Option<String>,
    pub name: Option<String>,
    pub priority: Option<i16>,
    pub short_description: Option<String>,
    pub show_in_navigation_pane: Option<bool>,
    pub show_promo_page: Option<bool>,
    pub web_icon: Option<String>,
}

impl CategoryEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            idcategory: row.get("idcategory")?,
            catalog_version: row.get("catalogVersion")?,
            categories: row.get("categories")?,
            display_name: row.get("displayName")?,
            filter_type: row.get("filterType")?,
            icon: row.get("icon")?,
            identifiant: row.get("id")?,
            long_description: row.get("longDescription")?,
            name: row.get("name")?,
            priority: row.get("priority")?,
            short_description: row.get("shortDescription")?,
            show_in_navigation_pane: row.get("showInNavigationPane")?,
            show_promo_page: row.get("showPromoPage")?,
            web_icon: row.get("webIcon")?,
        })
    }
}

// port: dao/CategoryDAO#getAll()
// port: jpa/CategoryEntity@CategoryEntity.getAll
/// Every category, in key order as MySQL scans the table.
pub fn get_all(db: &Connection) -> rusqlite::Result<Vec<CategoryEntity>> {
    db.prepare_cached("SELECT * FROM category ORDER BY idcategory")?.query_map([], CategoryEntity::from_row)?.collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::store;

    #[test]
    fn reads_every_category() {
        let db = store::open(None, &BTreeMap::new()).unwrap();
        let all = get_all(&db).unwrap();
        assert_eq!(all.len(), 54);
        assert!(all.windows(2).all(|w| w[0].idcategory < w[1].idcategory));
    }
}

//! `RewardTableEntity` (a table of reward scripts to draw from) and
//! `RewardTableDAO`. Static data.

// port: dao/RewardTableDAO#<init>()

use anyhow::anyhow;
use rusqlite::{Connection, OptionalExtension, Row};

use crate::Fault;

/// `RewardTableEntity`, with its LAZY `items`.
#[derive(Debug, Clone, PartialEq)]
pub struct RewardTableEntity {
    pub id: i64,
    pub name: Option<String>,
    pub items: Vec<RewardTableItemEntity>,
}

/// `RewardTableItemEntity`: a reward script and its weight in the table.
#[derive(Debug, Clone, PartialEq)]
pub struct RewardTableItemEntity {
    pub id: i64,
    pub script: String,
    pub drop_weight: Option<f64>,
}

impl RewardTableItemEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self { id: row.get("ID")?, script: row.get("script")?, drop_weight: row.get("dropWeight")? })
    }
}

// port: dao/RewardTableDAO#findByName(String)
// port: jpa/RewardTableEntity@RewardTableEntity.findByName
/// The table with this name, with its items in key order (the bag has no
/// order; MySQL reads it through the foreign key's index). `getSingleResult`
/// throws when there is none.
pub fn find_by_name(db: &Connection, name: &str) -> Result<RewardTableEntity, Fault> {
    let tables: Vec<(i64, Option<String>)> = db
        .prepare_cached("SELECT ID, name FROM reward_table WHERE name = ?1")?
        .query_map([name], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (id, name) = match tables.as_slice() {
        [table] => table.clone(),
        [] => return Err(Fault::Internal(anyhow!("NoResultException: No entity found for query"))),
        _ => return Err(Fault::Internal(anyhow!("NonUniqueResultException: query did not return a unique result"))),
    };
    let items = db
        .prepare_cached("SELECT * FROM reward_table_item WHERE rewardTableEntity_ID = ?1 ORDER BY ID")?
        .query_map([id], RewardTableItemEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(RewardTableEntity { id, name, items })
}

/// `RewardTableDAO.find` (`LongKeyedDAO.find`), with its items in key order.
pub fn find(db: &Connection, id: i64) -> rusqlite::Result<Option<RewardTableEntity>> {
    let Some(name) = db
        .prepare_cached("SELECT name FROM reward_table WHERE ID = ?1")?
        .query_row([id], |r| r.get::<_, Option<String>>(0))
        .optional()?
    else {
        return Ok(None);
    };
    let items = db
        .prepare_cached("SELECT * FROM reward_table_item WHERE rewardTableEntity_ID = ?1 ORDER BY ID")?
        .query_map([id], RewardTableItemEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(Some(RewardTableEntity { id, name, items }))
}

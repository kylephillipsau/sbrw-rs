//! `CardPackEntity` (a card pack: one reward script per card) and
//! `CardPackDAO`. Static data.

// port: dao/CardPackDAO#<init>()

use anyhow::anyhow;
use rusqlite::{Connection, Row};

use crate::Fault;

/// `CardPackEntity`, with its LAZY `items`.
#[derive(Debug, Clone, PartialEq)]
pub struct CardPackEntity {
    pub id: i64,
    pub entitlement_tag: Option<String>,
    pub items: Vec<CardPackItemEntity>,
}

/// `CardPackItemEntity`: one card.
#[derive(Debug, Clone, PartialEq)]
pub struct CardPackItemEntity {
    pub id: i64,
    pub script: String,
}

impl CardPackItemEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self { id: row.get("ID")?, script: row.get("script")? })
    }
}

// port: dao/CardPackDAO#findByEntitlementTag(String)
// port: jpa/CardPackEntity@CardPackEntity.findByEntitlementTag
/// The pack with this tag, with its cards in key order (the bag has no
/// order; MySQL reads it through the foreign key's index). `getSingleResult`
/// throws when there is none, or more than one.
pub fn find_by_entitlement_tag(db: &Connection, entitlement_tag: &str) -> Result<CardPackEntity, Fault> {
    let packs: Vec<(i64, Option<String>)> = db
        .prepare_cached("SELECT ID, entitlementTag FROM card_pack WHERE entitlementTag = ?1")?
        .query_map([entitlement_tag], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let (id, entitlement_tag) = match packs.as_slice() {
        [pack] => pack.clone(),
        [] => return Err(Fault::Internal(anyhow!("NoResultException: No entity found for query"))),
        _ => return Err(Fault::Internal(anyhow!("NonUniqueResultException: query did not return a unique result"))),
    };
    let items = db
        .prepare_cached("SELECT * FROM card_pack_item WHERE cardPackEntity_ID = ?1 ORDER BY ID")?
        .query_map([id], CardPackItemEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(CardPackEntity { id, entitlement_tag, items })
}

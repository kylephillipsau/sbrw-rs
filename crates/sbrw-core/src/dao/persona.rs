//! `PersonaEntity` (a driver) and the parts of `PersonaDAO` ported so far.

use rusqlite::{Connection, Row};
use time::PrimitiveDateTime;

use crate::datetime;

/// `PersonaEntity`. `badges` and `numCarSlots` exist in the table but aren't
/// mapped upstream, so they keep their column defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonaEntity {
    pub persona_id: i64,
    pub boost: f64,
    pub cash: f64,
    pub icon_index: i32,
    pub level: i32,
    pub motto: Option<String>,
    pub name: Option<String>,
    pub percent_to_level: f32,
    pub rating: f64,
    pub rep: f64,
    pub rep_at_current_level: i32,
    pub score: i32,
    pub cur_car_index: i32,
    pub user_id: Option<i64>,
    pub created: Option<PrimitiveDateTime>,
    pub first_login: Option<PrimitiveDateTime>,
    pub last_login: Option<PrimitiveDateTime>,
}

impl PersonaEntity {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            persona_id: row.get("ID")?,
            boost: row.get("boost")?,
            cash: row.get("cash")?,
            icon_index: row.get("iconIndex")?,
            level: row.get("level")?,
            motto: row.get("motto")?,
            name: row.get("name")?,
            percent_to_level: row.get::<_, f64>("percentToLevel")? as f32,
            rating: row.get("rating")?,
            rep: row.get("rep")?,
            rep_at_current_level: row.get("repAtCurrentLevel")?,
            score: row.get("score")?,
            cur_car_index: row.get("curCarIndex")?,
            user_id: row.get("USERID")?,
            created: datetime::column(row, "created")?,
            first_login: datetime::column(row, "first_login")?,
            last_login: datetime::column(row, "last_login")?,
        })
    }
}

/// The personas of a user, as `UserEntity.personas` (EAGER, no `@OrderBy`;
/// MySQL returns them by primary key).
pub fn find_by_user_id(db: &Connection, user_id: i64) -> rusqlite::Result<Vec<PersonaEntity>> {
    let mut stmt = db.prepare_cached("SELECT * FROM persona WHERE USERID = ?1 ORDER BY ID")?;
    stmt.query_map([user_id], PersonaEntity::from_row)?.collect()
}

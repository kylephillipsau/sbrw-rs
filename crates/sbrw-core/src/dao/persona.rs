//! `PersonaEntity` (a driver) and the parts of `PersonaDAO` ported so far.

// port: dao/PersonaDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::PrimitiveDateTime;

use crate::datetime;

/// `PersonaEntity`. `badges` and `numCarSlots` exist in the table but aren't
/// mapped upstream, so they keep their column defaults. `Default` is
/// `new PersonaEntity()`.
#[derive(Debug, Clone, Default, PartialEq)]
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

/// `PersonaDAO.find`.
pub fn find(db: &Connection, persona_id: i64) -> rusqlite::Result<Option<PersonaEntity>> {
    db.prepare_cached("SELECT * FROM persona WHERE ID = ?1")?
        .query_row([persona_id], PersonaEntity::from_row)
        .optional()
}

// port: dao/PersonaDAO#findByName(String)
// port: jpa/PersonaEntity@PersonaEntity.findByName
/// Names compare case-insensitively, as in upstream's MySQL collation. A
/// null name matches nothing.
pub fn find_by_name(db: &Connection, name: Option<&str>) -> rusqlite::Result<Option<PersonaEntity>> {
    db.prepare_cached("SELECT * FROM persona WHERE name = ?1 ORDER BY ID LIMIT 1")?
        .query_row([name], PersonaEntity::from_row)
        .optional()
}

/// `PersonaDAO.insert` (`persist`): every mapped column; sets the generated id.
pub fn insert(db: &Connection, p: &mut PersonaEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO persona (boost, cash, curCarIndex, iconIndex, level, motto, name, percentToLevel, rating, rep,
         repAtCurrentLevel, score, USERID, created, first_login, last_login)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
    )?
    .execute(params![
        p.boost,
        p.cash,
        p.cur_car_index,
        p.icon_index,
        p.level,
        p.motto,
        p.name,
        f64::from(p.percent_to_level),
        p.rating,
        p.rep,
        p.rep_at_current_level,
        p.score,
        p.user_id,
        datetime::opt_to_sql(p.created),
        datetime::opt_to_sql(p.first_login),
        datetime::opt_to_sql(p.last_login),
    ])?;
    p.persona_id = db.last_insert_rowid();
    Ok(())
}

// port: dao/util/BaseDAO#delete(TE)
/// `PersonaDAO.delete` (`remove(merge(entity))`). The persona maps no
/// cascades; the database's foreign keys delete its cars, inventory and the rest.
pub fn delete(db: &Connection, p: &PersonaEntity) -> rusqlite::Result<()> {
    db.prepare_cached("DELETE FROM persona WHERE ID = ?1")?.execute([p.persona_id])?;
    Ok(())
}

/// `PersonaDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, p: &PersonaEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE persona SET boost = ?2, cash = ?3, curCarIndex = ?4, iconIndex = ?5, level = ?6, motto = ?7, name = ?8,
         percentToLevel = ?9, rating = ?10, rep = ?11, repAtCurrentLevel = ?12, score = ?13, USERID = ?14,
         created = ?15, first_login = ?16, last_login = ?17 WHERE ID = ?1",
    )?
    .execute(params![
        p.persona_id,
        p.boost,
        p.cash,
        p.cur_car_index,
        p.icon_index,
        p.level,
        p.motto,
        p.name,
        f64::from(p.percent_to_level),
        p.rating,
        p.rep,
        p.rep_at_current_level,
        p.score,
        p.user_id,
        datetime::opt_to_sql(p.created),
        datetime::opt_to_sql(p.first_login),
        datetime::opt_to_sql(p.last_login),
    ])?;
    Ok(())
}

// port: dao/PersonaDAO#countPersonas()
// port: jpa/PersonaEntity@PersonaEntity.countPersonas
/// How many personas there are.
pub fn count_personas(db: &Connection) -> rusqlite::Result<i64> {
    db.prepare_cached("SELECT count(*) FROM persona")?.query_row([], |r| r.get(0))
}

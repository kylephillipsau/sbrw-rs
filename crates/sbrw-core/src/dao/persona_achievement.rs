//! `PersonaAchievementEntity` (a persona's progress on one achievement) and the
//! parts of `PersonaAchievementDAO` ported so far.

// port: dao/PersonaAchievementDAO#<init>()

use rusqlite::{Connection, params};

use crate::dao::persona_achievement_rank::{self, PersonaAchievementRankEntity};

/// `PersonaAchievementEntity`. `ranks` is EAGER.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersonaAchievementEntity {
    pub id: i64,
    pub persona_id: i64,
    pub achievement_id: i64,
    pub can_progress: bool,
    pub current_value: Option<i64>,
    pub ranks: Vec<PersonaAchievementRankEntity>,
}

// port: dao/PersonaAchievementDAO#findAllByPersonaId(Long)
// port: jpa/PersonaAchievementEntity@PersonaAchievementEntity.findAllByPersonaId
/// In primary-key order, as MySQL returns them through the persona index; each
/// with its ranks in the order they were created.
pub fn find_all_by_persona_id(db: &Connection, persona_id: i64) -> rusqlite::Result<Vec<PersonaAchievementEntity>> {
    let mut stmt = db.prepare_cached("SELECT * FROM persona_achievement WHERE persona_id = ?1 ORDER BY ID")?;
    let rows = stmt.query_map([persona_id], |row| {
        Ok(PersonaAchievementEntity {
            id: row.get("ID")?,
            persona_id: row.get("persona_id")?,
            achievement_id: row.get("achievement_id")?,
            // A primitive boolean: Hibernate reads SQL NULL as false.
            can_progress: row.get::<_, Option<bool>>("can_progress")?.unwrap_or(false),
            current_value: row.get("current_value")?,
            ranks: Vec::new(),
        })
    })?;
    rows.map(|pa| {
        let mut pa = pa?;
        pa.ranks = persona_achievement_rank::find_by_persona_achievement(db, pa.id)?;
        Ok(pa)
    })
    .collect()
}

/// `PersonaAchievementDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, pa: &mut PersonaAchievementEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO persona_achievement (can_progress, current_value, achievement_id, persona_id) VALUES (?1, ?2, ?3, ?4)",
    )?
    .execute(params![pa.can_progress, pa.current_value, pa.achievement_id, pa.persona_id])?;
    pa.id = db.last_insert_rowid();
    Ok(())
}

/// `PersonaAchievementDAO.update` (`merge`): every mapped column. Its ranks
/// map no cascade to merge.
pub fn update(db: &Connection, pa: &PersonaAchievementEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE persona_achievement SET can_progress = ?2, current_value = ?3, achievement_id = ?4, persona_id = ?5
         WHERE ID = ?1",
    )?
    .execute(params![pa.id, pa.can_progress, pa.current_value, pa.achievement_id, pa.persona_id])?;
    Ok(())
}

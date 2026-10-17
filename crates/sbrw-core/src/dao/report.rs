//! `ReportEntity` (a report against a driver: by a player, or by the server
//! for a race that wasn't legit) and `ReportDAO`.

// port: dao/ReportDAO#<init>()

use rusqlite::{Connection, params};

/// `ReportEntity`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportEntity {
    pub id: i64,
    pub persona_id: Option<i64>,
    pub abuser_persona_id: Option<i64>,
    pub description: Option<String>,
    pub petition_type: Option<i32>,
    pub custom_car_id: Option<i32>,
    pub chat_minutes: Option<i32>,
    pub hacksdetected: Option<i64>,
}

/// `ReportDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, report: &mut ReportEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO report (abuserPersonaId, chatMinutes, customCarID, description, hacksdetected, personaId,
         petitionType) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?
    .execute(params![
        report.abuser_persona_id,
        report.chat_minutes,
        report.custom_car_id,
        report.description,
        report.hacksdetected,
        report.persona_id,
        report.petition_type
    ])?;
    report.id = db.last_insert_rowid();
    Ok(())
}

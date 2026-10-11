//! `PersonaAchievementRankEntity` (a persona's state on one rank of an
//! achievement) and the parts of `PersonaAchievementRankDAO` ported so far.

// port: dao/PersonaAchievementRankDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row};
use time::PrimitiveDateTime;

use crate::dao::achievement::AchievementRankEntity;
use crate::datetime;

/// `PersonaAchievementRankEntity`. Its achievement rank is held by id; finders
/// that need the rank itself return it alongside.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersonaAchievementRankEntity {
    pub id: i64,
    pub persona_achievement_id: i64,
    pub achievement_rank_id: i64,
    pub state: Option<String>,
    pub achieved_on: Option<PrimitiveDateTime>,
}

impl PersonaAchievementRankEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            persona_achievement_id: row.get("persona_achievement_id")?,
            achievement_rank_id: row.get("achievement_rank_id")?,
            state: row.get("state")?,
            achieved_on: datetime::column(row, "achieved_on")?,
        })
    }
}

/// `PersonaAchievementEntity.ranks`, in the order they were created.
pub(crate) fn find_by_persona_achievement(
    db: &Connection,
    persona_achievement_id: i64,
) -> rusqlite::Result<Vec<PersonaAchievementRankEntity>> {
    let mut stmt =
        db.prepare_cached("SELECT * FROM persona_achievement_rank WHERE persona_achievement_id = ?1 ORDER BY ID")?;
    stmt.query_map([persona_achievement_id], PersonaAchievementRankEntity::from_row)?.collect()
}

// port: dao/PersonaAchievementRankDAO#findHighestCompletedRankOfAchievementByPersona(Long,Long)
// port: jpa/PersonaAchievementRankEntity@PersonaAchievementRankEntity.findHighestCompletedRankOfAchievementByPersona
/// The persona's completed (or reward-waiting) rank of an achievement with the
/// highest rank number, with that achievement rank.
pub fn find_highest_completed_rank_of_achievement_by_persona(
    db: &Connection,
    persona_id: i64,
    achievement_id: i64,
) -> rusqlite::Result<Option<(PersonaAchievementRankEntity, AchievementRankEntity)>> {
    db.prepare_cached(
        "SELECT r.*, ar.ID AS ar_ID, ar.achievement_id AS ar_achievement_id, ar.points AS ar_points, ar.rank AS ar_rank,
                ar.rarity AS ar_rarity, ar.reward_description AS ar_reward_description, ar.reward_type AS ar_reward_type,
                ar.reward_visual_style AS ar_reward_visual_style, ar.threshold_value AS ar_threshold_value
         FROM persona_achievement_rank r
         JOIN persona_achievement pa ON pa.ID = r.persona_achievement_id
         JOIN achievement_rank ar ON ar.ID = r.achievement_rank_id
         WHERE pa.persona_id = ?1 AND pa.achievement_id = ?2 AND (r.state = 'Completed' OR r.state = 'RewardWaiting')
         ORDER BY ar.rank DESC, r.ID LIMIT 1",
    )?
    .query_row([persona_id, achievement_id], |row| {
        Ok((PersonaAchievementRankEntity::from_row(row)?, AchievementRankEntity::from_row(row, "ar_")?))
    })
    .optional()
}

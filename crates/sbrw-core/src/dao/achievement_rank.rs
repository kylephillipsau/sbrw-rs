//! `AchievementRankDAO`: updates to the static achievement ranks.

// port: dao/AchievementRankDAO#<init>()

use rusqlite::{Connection, params};

// port: dao/AchievementRankDAO#updateRankRarity(Long,float)
// port: jpa/AchievementRankEntity@AchievementRankEntity.updateRarity
/// Set a rank's rarity: in the static row, and in `achievement_rank_rarity`,
/// which keeps it across restarts as MySQL kept the static row.
pub fn update_rank_rarity(db: &Connection, rank_id: i64, rarity: f32) -> rusqlite::Result<()> {
    db.prepare_cached("UPDATE achievement_rank SET rarity = ?2 WHERE ID = ?1")?
        .execute(params![rank_id, f64::from(rarity)])?;
    db.prepare_cached("INSERT OR REPLACE INTO achievement_rank_rarity (achievement_rank_id, rarity) VALUES (?1, ?2)")?
        .execute(params![rank_id, f64::from(rarity)])?;
    Ok(())
}

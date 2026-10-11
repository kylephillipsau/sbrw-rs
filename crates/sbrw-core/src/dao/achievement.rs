//! `AchievementEntity` and `AchievementRankEntity` (static achievement
//! definitions) and the parts of `AchievementDAO` ported so far.

// port: dao/AchievementDAO#<init>()

use rusqlite::{Connection, Row};

/// `AchievementEntity`. `ranks` is EAGER; the LAZY badge definition is held by id.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementEntity {
    pub id: i64,
    pub visible: Option<bool>,
    pub progress_text: Option<String>,
    pub stat_conversion: Option<String>,
    pub category: Option<String>,
    pub auto_update: Option<bool>,
    pub name: Option<String>,
    pub update_trigger: Option<String>,
    pub update_value: Option<String>,
    pub ranks: Vec<AchievementRankEntity>,
    pub badge_definition_id: i64,
    pub should_overwrite_progress: Option<bool>,
}

/// `AchievementRankEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementRankEntity {
    pub id: i64,
    pub achievement_id: i64,
    pub points: Option<i32>,
    pub rank: Option<i32>,
    pub rarity: Option<f32>,
    pub reward_description: Option<String>,
    pub reward_type: Option<String>,
    pub reward_visual_style: Option<String>,
    pub threshold_value: Option<i32>,
}

impl AchievementRankEntity {
    pub(crate) fn from_row(row: &Row<'_>, prefix: &str) -> rusqlite::Result<Self> {
        let col = |name: &str| format!("{prefix}{name}");
        Ok(Self {
            id: row.get(col("ID").as_str())?,
            achievement_id: row.get(col("achievement_id").as_str())?,
            points: row.get(col("points").as_str())?,
            rank: row.get(col("rank").as_str())?,
            rarity: row.get::<_, Option<f64>>(col("rarity").as_str())?.map(|v| v as f32),
            reward_description: row.get(col("reward_description").as_str())?,
            reward_type: row.get(col("reward_type").as_str())?,
            reward_visual_style: row.get(col("reward_visual_style").as_str())?,
            threshold_value: row.get(col("threshold_value").as_str())?,
        })
    }

    /// `isRare()`: `getRarity() <= 0.05f`, so a null rarity throws.
    pub fn is_rare(&self) -> Option<bool> {
        self.rarity.map(|r| r <= 0.05)
    }
}

// port: dao/AchievementDAO#findAll()
// port: jpa/AchievementEntity@AchievementEntity.findAll
/// Every achievement with its ranks, in primary-key order as MySQL returns the
/// unordered queries.
pub fn find_all(db: &Connection) -> rusqlite::Result<Vec<AchievementEntity>> {
    let mut ranks = db.prepare_cached("SELECT * FROM achievement_rank WHERE achievement_id = ?1 ORDER BY ID")?;
    let mut stmt = db.prepare_cached("SELECT * FROM achievement ORDER BY ID")?;
    let rows = stmt.query_map([], |row| {
        Ok(AchievementEntity {
            id: row.get("ID")?,
            visible: row.get("visible")?,
            progress_text: row.get("progress_text")?,
            stat_conversion: row.get("stat_conversion")?,
            category: row.get("category")?,
            auto_update: row.get("auto_update")?,
            name: row.get("name")?,
            update_trigger: row.get("update_trigger")?,
            update_value: row.get("update_value")?,
            ranks: Vec::new(),
            badge_definition_id: row.get("badge_definition_id")?,
            should_overwrite_progress: row.get("should_overwrite_progress")?,
        })
    })?;
    rows.map(|a| {
        let mut a = a?;
        a.ranks = ranks.query_map([a.id], |r| AchievementRankEntity::from_row(r, ""))?.collect::<Result<_, _>>()?;
        Ok(a)
    })
    .collect()
}

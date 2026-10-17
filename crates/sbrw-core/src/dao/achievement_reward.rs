//! `AchievementRewardEntity` (the reward script behind an achievement rank's
//! reward description) and `AchievementRewardDAO`. Static data.

// port: dao/AchievementRewardDAO#<init>()

use anyhow::anyhow;
use rusqlite::{Connection, Row};

use crate::Fault;

/// `AchievementRewardEntity`.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementRewardEntity {
    pub id: i64,
    pub reward_script: String,
    pub reward_description: Option<String>,
    pub internal_reward_description: Option<String>,
}

impl AchievementRewardEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            reward_script: row.get("rewardScript")?,
            reward_description: row.get("reward_description")?,
            internal_reward_description: row.get("internal_reward_description")?,
        })
    }
}

// port: dao/AchievementRewardDAO#findByDescription(String)
// port: jpa/AchievementRewardEntity@AchievementRewardEntity.findByDescription
/// The reward with this internal description. Like `getSingleResult`, it
/// fails unless there is exactly one.
pub fn find_by_description(db: &Connection, description: Option<&str>) -> Result<AchievementRewardEntity, Fault> {
    let found: Vec<AchievementRewardEntity> = db
        .prepare_cached("SELECT * FROM achievement_reward WHERE internal_reward_description = ?1 LIMIT 2")?
        .query_map([description], AchievementRewardEntity::from_row)?
        .collect::<rusqlite::Result<_>>()?;
    match <[AchievementRewardEntity; 1]>::try_from(found) {
        Ok([reward]) => Ok(reward),
        Err(found) if found.is_empty() => Err(anyhow!("NoResultException: No entity found for query").into()),
        Err(_) => Err(anyhow!("NonUniqueResultException: query did not return a unique result").into()),
    }
}

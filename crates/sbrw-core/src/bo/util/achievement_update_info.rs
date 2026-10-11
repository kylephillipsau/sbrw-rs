//! `AchievementUpdateInfo`: what one achievement update did, for the push that
//! reports it.

use time::PrimitiveDateTime;

use crate::dao::achievement::{AchievementEntity, AchievementRankEntity};

#[derive(Debug, Clone, PartialEq)]
pub struct AchievementUpdateInfo {
    // port: bo/util/AchievementUpdateInfo#getAchievementEntity()
    pub achievement_entity: AchievementEntity,
    // port: bo/util/AchievementUpdateInfo#getProgressedAchievement()
    // port: bo/util/AchievementUpdateInfo#setProgressedAchievement(ProgressedAchievement)
    pub progressed_achievement: Option<ProgressedAchievement>,
    // port: bo/util/AchievementUpdateInfo#getCompletedAchievementRanks()
    pub completed_achievement_ranks: Vec<CompletedAchievementRank>,
    // port: bo/util/AchievementUpdateInfo#getPointsGiven()
    // port: bo/util/AchievementUpdateInfo#setPointsGiven(Integer)
    pub points_given: i32,
}

impl AchievementUpdateInfo {
    // port: bo/util/AchievementUpdateInfo#<init>(AchievementEntity)
    pub fn new(achievement_entity: AchievementEntity) -> Self {
        Self {
            achievement_entity,
            progressed_achievement: None,
            completed_achievement_ranks: Vec::new(),
            points_given: 0,
        }
    }
}

/// `AchievementUpdateInfo.ProgressedAchievement`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressedAchievement {
    pub achievement_definition_id: Option<i64>,
    pub value: Option<i64>,
}

/// `AchievementUpdateInfo.CompletedAchievementRank`.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletedAchievementRank {
    pub achievement_entity: AchievementEntity,
    pub achievement_rank_entity: AchievementRankEntity,
    pub achieved_on: Option<PrimitiveDateTime>,
}

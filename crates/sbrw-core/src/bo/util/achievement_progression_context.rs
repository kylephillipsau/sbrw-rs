//! `AchievementProgressionContext`: what PROGRESSION achievement scripts see
//! about a persona's progress.

/// Scripts read it through its getters (`bo::scripting`).
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementProgressionContext {
    pub cash: Option<i32>,
    pub rep: Option<i32>,
    pub level: Option<i32>,
    pub score: Option<i32>,
    pub treasure_streak: Option<i32>,
    pub level_changed: bool,
    pub score_changed: bool,
    pub treasure_streak_changed: bool,
    pub in_event: bool,
}

impl AchievementProgressionContext {
    // port: bo/util/AchievementProgressionContext#<init>(Integer,Integer,Integer,Integer,Integer,boolean,boolean,boolean,boolean)
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cash: Option<i32>,
        rep: Option<i32>,
        level: Option<i32>,
        score: Option<i32>,
        treasure_streak: Option<i32>,
        level_changed: bool,
        score_changed: bool,
        treasure_streak_changed: bool,
        in_event: bool,
    ) -> Self {
        Self {
            cash,
            rep,
            level,
            score,
            treasure_streak,
            level_changed,
            score_changed,
            treasure_streak_changed,
            in_event,
        }
    }
}

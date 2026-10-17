//! `EventRewardEntity` (how an event pays) and its lookup. Static data.

use rusqlite::{Connection, OptionalExtension, Row};

/// `EventRewardEntity`. Its EAGER reward tables, one per rank, are kept as
/// the ids they join on; `reward_table::find` loads one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventRewardEntity {
    pub id: String,
    pub base_rep_reward: i32,
    pub level_rep_reward_multiplier: f32,
    pub final_rep_reward_multiplier: f32,
    pub perfect_start_rep_multiplier: f32,
    pub top_speed_rep_multiplier: f32,
    /// `rank1RepMultiplier` to `rank8RepMultiplier`.
    pub rank_rep_multipliers: [f32; 8],
    pub base_cash_reward: i32,
    pub level_cash_reward_multiplier: f32,
    pub final_cash_reward_multiplier: f32,
    pub perfect_start_cash_multiplier: f32,
    pub top_speed_cash_multiplier: f32,
    /// `rank1CashMultiplier` to `rank8CashMultiplier`.
    pub rank_cash_multipliers: [f32; 8],
    pub min_top_speed_trigger: f32,
    /// `rewardTableRank1` to `rewardTableRank8`.
    pub reward_table_ranks: [Option<i64>; 8],
}

impl EventRewardEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        // MySQL FLOAT columns.
        let float = |name: &str| row.get::<_, f64>(name).map(|v| v as f32);
        let mut entity = Self {
            id: row.get("ID")?,
            base_rep_reward: row.get("baseRepReward")?,
            level_rep_reward_multiplier: float("levelRepRewardMultiplier")?,
            final_rep_reward_multiplier: float("finalRepRewardMultiplier")?,
            perfect_start_rep_multiplier: float("perfectStartRepMultiplier")?,
            top_speed_rep_multiplier: float("topSpeedRepMultiplier")?,
            base_cash_reward: row.get("baseCashReward")?,
            level_cash_reward_multiplier: float("levelCashRewardMultiplier")?,
            final_cash_reward_multiplier: float("finalCashRewardMultiplier")?,
            perfect_start_cash_multiplier: float("perfectStartCashMultiplier")?,
            top_speed_cash_multiplier: float("topSpeedCashMultiplier")?,
            min_top_speed_trigger: float("minTopSpeedTrigger")?,
            ..Default::default()
        };
        for rank in 1..=8 {
            entity.rank_rep_multipliers[rank - 1] = float(&format!("rank{rank}RepMultiplier"))?;
            entity.rank_cash_multipliers[rank - 1] = float(&format!("rank{rank}CashMultiplier"))?;
            entity.reward_table_ranks[rank - 1] = row.get(format!("rewardTable_rank{rank}_id").as_str())?;
        }
        Ok(entity)
    }
}

/// The configuration with this id, as Hibernate loads an event's LAZY
/// reward configuration (there is no DAO for it).
pub fn find(db: &Connection, id: &str) -> rusqlite::Result<Option<EventRewardEntity>> {
    db.prepare_cached("SELECT * FROM event_reward WHERE ID = ?1")?
        .query_row([id], EventRewardEntity::from_row)
        .optional()
}

//! Upstream's `@Schedule` methods: the jobs the container ran on a clock.
//! Each run is its own transaction, as each timer call was.

use anyhow::anyhow;

use crate::bo::{achievement, ban, car_slot, inventory, online_users};
use crate::{Fault, World};

/// The jobs upstream ran every minute (`@Schedule(minute = "*", hour = "*")`)
/// that are ported, by `Class#method`.
pub const EVERY_MINUTE: &[&str] = &[
    "BanBO#checkExpiredBans",
    "CarSlotBO#scheduledRemoval",
    "InventoryBO#scheduledDeleteExpiredItems",
    "OnlineUsersBO#insertOnlineStats",
];

/// The jobs upstream ran every half hour (`@Schedule(minute = "*/30", hour = "*")`).
pub const EVERY_HALF_HOUR: &[&str] = &["AchievementBO#updateRankRarities"];

/// Run one job by its `Class#method` name.
pub fn run(w: &mut World, name: &str) -> Result<(), Fault> {
    match name {
        "BanBO#checkExpiredBans" => ban::check_expired_bans(w),
        "CarSlotBO#scheduledRemoval" => car_slot::scheduled_removal(w),
        "InventoryBO#scheduledDeleteExpiredItems" => inventory::scheduled_delete_expired_items(w),
        "OnlineUsersBO#insertOnlineStats" => online_users::insert_online_stats(w),
        "AchievementBO#updateRankRarities" => achievement::update_rank_rarities(w),
        other => Err(anyhow!("no scheduled job {other}").into()),
    }
}

//! `OnlineUsersBO`: a row of player counts at startup and every minute.

use crate::dao::online_users::{self, OnlineUsersEntity};
use crate::dao::user;
use crate::{Fault, World};

/// The singleton's state.
#[derive(Debug, Default)]
pub struct OnlineUsers {
    last_recorded_stats: Option<OnlineUsersEntity>,
}

// port: bo/OnlineUsersBO#init()
pub fn init(w: &mut World) -> Result<(), Fault> {
    insert_online_stats(w)
}

// port: bo/OnlineUsersBO#insertOnlineStats()
/// Scheduled every minute upstream (`@Schedule(minute = "*")`).
pub fn insert_online_stats(w: &mut World) -> Result<(), Fault> {
    let time_record = (w.env.now().unix_timestamp_nanos() / 1_000_000_000) as i32;
    let stats = OnlineUsersEntity {
        number_of_online: i64::from(w.openfire.get_total_online_users(&w.effects)),
        number_of_registered: user::count_users(w.db())?,
        time_record,
    };
    online_users::insert(w.db(), &stats)?;
    w.online_users.last_recorded_stats = Some(stats);
    Ok(())
}

// port: bo/OnlineUsersBO#getOnlineUsersStats()
pub fn get_online_users_stats(w: &World) -> Option<OnlineUsersEntity> {
    w.online_users.last_recorded_stats
}

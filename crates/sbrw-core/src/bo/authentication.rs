//! `AuthenticationBO`.

use crate::dao::ban::{self, BanEntity};
use crate::dao::user::UserEntity;
use crate::{Fault, World, datetime};

// port: bo/AuthenticationBO#checkUserBan(UserEntity)
pub fn check_user_ban(w: &World, user: &UserEntity) -> Result<Option<BanEntity>, Fault> {
    Ok(ban::find_by_user(w.db(), user.id, datetime::now(&w.env))?)
}

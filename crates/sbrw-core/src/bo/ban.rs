//! `BanBO`: banning users, their hardware with them, and ending bans.

use anyhow::anyhow;
use time::PrimitiveDateTime;

use crate::dao::ban::{self, BanEntity};
use crate::dao::hardware_info;
use crate::dao::persona::PersonaEntity;
use crate::dao::user::{self, UserEntity};
use crate::{Fault, World, datetime};

// port: bo/BanBO#checkExpiredBans()
/// Every minute: end the bans whose time is up.
pub fn check_expired_bans(w: &mut World) -> Result<(), Fault> {
    for ban_entity in ban::find_all_expired(w.db(), datetime::now(&w.env))? {
        expire_ban(w, ban_entity)?;
    }
    Ok(())
}

// port: bo/BanBO#getUserBan(UserEntity)
pub fn get_user_ban(w: &World, user_entity: &UserEntity) -> Result<Option<BanEntity>, Fault> {
    Ok(ban::find_by_user(w.db(), user_entity.id, datetime::now(&w.env))?)
}

// port: bo/BanBO#isBanned(UserEntity)
pub fn is_banned(w: &World, user_entity: &UserEntity) -> Result<bool, Fault> {
    Ok(get_user_ban(w, user_entity)?.is_some())
}

// port: bo/BanBO#unbanUser(UserEntity)
pub fn unban_user(w: &mut World, user_entity: &UserEntity) -> Result<(), Fault> {
    if let Some(ban_entity) = ban::find_by_user(w.db(), user_entity.id, datetime::now(&w.env))? {
        expire_ban(w, ban_entity)?;
    }
    Ok(())
}

// port: bo/BanBO#banUser(UserEntity,PersonaEntity,String,LocalDateTime)
/// A ban from now, for good without an end; the user's hardware is flagged.
pub fn ban_user(
    w: &mut World,
    user_to_ban: &UserEntity,
    admin_persona: Option<&PersonaEntity>,
    ban_reason: Option<String>,
    ban_expiration: Option<PrimitiveDateTime>,
) -> Result<(), Fault> {
    let mut ban_entity = BanEntity {
        user_id: Some(user_to_ban.id),
        ends_at: ban_expiration,
        started: Some(datetime::now(&w.env)),
        reason: ban_reason,
        banned_by_id: admin_persona.map(|p| p.persona_id),
        active: Some(true),
        ..Default::default()
    };
    ban::insert(w.db(), &mut ban_entity)?;

    if let Some(mut hardware_info_entity) = hardware_info::find_by_user_id(w.db(), user_to_ban.id)? {
        hardware_info_entity.banned = true;
        hardware_info::update(w.db(), &hardware_info_entity)?;
    }
    Ok(())
}

// port: bo/BanBO#expireBan(BanEntity)
/// The ban ends, and the hardware the user last played on is cleared.
pub fn expire_ban(w: &mut World, mut ban_entity: BanEntity) -> Result<(), Fault> {
    let user_id = ban_entity.user_id.ok_or_else(|| Fault::null_pointer("BanEntity.userEntity"))?;
    let user_entity =
        user::find(w.db(), user_id)?.ok_or_else(|| anyhow!("NullPointerException: BanEntity.userEntity"))?;
    if let Some(hash) = &user_entity.game_hardware_hash {
        let mut hardware_info_entity = hardware_info::find_by_hardware_hash(w.db(), hash)?
            .ok_or_else(|| Fault::null_pointer("HardwareInfoEntity"))?;
        hardware_info_entity.banned = false;
        hardware_info::update(w.db(), &hardware_info_entity)?;
    }
    ban_entity.active = Some(false);
    ban::update(w.db(), &ban_entity)?;
    Ok(())
}

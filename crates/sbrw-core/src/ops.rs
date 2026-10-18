//! sbrw-rs's own operator commands (`sbrw admin …`), which upstream did with
//! SQL, its web pages or an admin's in-game commands. Each answers with a
//! line for the operator, or an error for one that can't be done.

use time::Duration;

use crate::bo::util::misc_utils;
use crate::bo::{admin, ban, invite_ticket};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::user::{self, UserEntity};
use crate::{Fault, World, datetime, password};

/// What an operator names: an account by email (anything with an `@`), or
/// the account that owns a driver.
fn find_target(w: &World, target: &str) -> Result<(UserEntity, Vec<PersonaEntity>), String> {
    let found = if target.contains('@') {
        user::find_by_email(w.db(), target).map_err(|e| e.to_string())?.map(|user| {
            let personas = user.personas.clone();
            (user, personas)
        })
    } else {
        match persona::find_by_name(w.db(), Some(target)).map_err(|e| e.to_string())? {
            Some(driver) => {
                let user_id = driver.user_id.ok_or("the driver has no account")?;
                user::find(w.db(), user_id).map_err(|e| e.to_string())?.map(|user| (user, vec![driver]))
            }
            None => None,
        }
    };
    found.ok_or_else(|| format!("no account or driver called {target}"))
}

fn fault(f: Fault) -> String {
    format!("{f:#}")
}

/// Ban the account, as an admin's `/ban` does: for the length (`1d2h`, as
/// `/ban` reads it) or for good, then kick it.
pub fn ban(w: &mut World, target: &str, length: Option<&str>, reason: Option<String>) -> Result<String, String> {
    let (user, drivers) = find_target(w, target)?;
    if ban::is_banned(w, &user).map_err(fault)? {
        return Err("User is already banned!".into());
    }
    let ends_at = match length {
        Some(length) => {
            let millis = misc_utils::length_to_miliseconds(length).map_err(fault)?;
            if millis == 0 {
                return Err(format!("{length} isn't a length like 1d2h30m"));
            }
            Some(datetime::now(&w.env) + Duration::seconds(millis / 1000))
        }
        None => None,
    };
    ban::ban_user(w, &user, None, reason, ends_at).map_err(fault)?;
    kick_drivers(w, &user, &drivers)?;
    Ok(match ends_at {
        Some(ends_at) => format!("Banned user! The ban ends at {} UTC.", datetime::to_sql(ends_at)),
        None => "Banned user!".into(),
    })
}

pub fn unban(w: &mut World, target: &str) -> Result<String, String> {
    let (user, _) = find_target(w, target)?;
    let existing = ban::get_user_ban(w, &user).map_err(fault)?.ok_or("User is not banned!")?;
    ban::expire_ban(w, existing).map_err(fault)?;
    Ok("Unbanned user!".into())
}

/// End the account's sessions, as an admin's `/kick` does.
pub fn kick(w: &mut World, target: &str) -> Result<String, String> {
    let (user, drivers) = find_target(w, target)?;
    kick_drivers(w, &user, &drivers)?;
    Ok("Kicked user!".into())
}

/// `/kick` tells one driver; an account named by email has each told.
fn kick_drivers(w: &mut World, user: &UserEntity, drivers: &[PersonaEntity]) -> Result<(), String> {
    if drivers.is_empty() {
        crate::bo::token_session::delete_by_user_id(w, user.id).map_err(fault)?;
    }
    for driver in drivers {
        admin::send_kick(w, user.id, driver.persona_id).map_err(fault)?;
    }
    Ok(())
}

/// A message to every player, as `SendAnnouncement` sends it.
pub fn announce(w: &mut World, message: &str) -> Result<String, String> {
    w.openfire.send_chat_announcement(Some(message), &w.effects);
    Ok("SUCCESS! sent announcement".into())
}

/// The name's invite ticket, as `CreateTicket` makes it.
pub fn ticket(w: &mut World, discord_name: &str) -> Result<String, String> {
    let ticket = invite_ticket::create_ticket(w, Some(discord_name)).map_err(fault)?;
    Ok(ticket.ticket.unwrap_or_default())
}

/// Make the account an admin, or not. Its in-game `/` commands work from its
/// next login.
pub fn set_admin(w: &mut World, email: &str, is_admin: bool) -> Result<String, String> {
    let mut user = user::find_by_email(w.db(), email).map_err(|e| e.to_string())?.ok_or("no such account")?;
    user.is_admin = is_admin;
    user::update(w.db(), &user).map_err(|e| e.to_string())?;
    Ok(if is_admin {
        format!("{email} is an admin from their next login.")
    } else {
        format!("{email} is not an admin.")
    })
}

/// Set the password the player types in the launcher, as a reset link would.
pub fn set_password(w: &mut World, email: &str, new_password: &str) -> Result<String, String> {
    if new_password.is_empty() {
        return Err("the password is empty".into());
    }
    let mut user = user::find_by_email(w.db(), email).map_err(|e| e.to_string())?.ok_or("no such account")?;
    use sha1::{Digest, Sha1};
    user.password = Some(password::hash(&hex::encode(Sha1::digest(new_password.as_bytes()))));
    user::update(w.db(), &user).map_err(|e| e.to_string())?;
    Ok("Password changed!".into())
}

/// The tokens the original's endpoints and pages take, as this run has them.
pub fn tokens(w: &World) -> Vec<(&'static str, Option<String>)> {
    ["ADMIN_AUTH", "ANNOUNCEMENT_AUTH", "TICKET_TOKEN"].into_iter().map(|n| (n, w.params.get_str_param(n))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::ban as ban_dao;

    fn world_with_driver() -> World {
        let w = World::test(&[]);
        let mut user = UserEntity::new();
        user.email = Some("player@example.com".into());
        user::insert(w.db(), &mut user).unwrap();
        let mut driver = PersonaEntity { user_id: Some(user.id), name: Some("DRIVER".into()), ..Default::default() };
        persona::insert(w.db(), &mut driver).unwrap();
        w
    }

    #[test]
    fn bans_by_driver_or_email_and_ends_them() {
        let mut w = world_with_driver();
        assert_eq!(ban(&mut w, "nobody", None, None), Err("no account or driver called nobody".into()));
        assert_eq!(ban(&mut w, "DRIVER", Some("1x"), None), Err("1x isn't a length like 1d2h30m".into()));
        assert_eq!(
            ban(&mut w, "DRIVER", Some("1d2h"), Some("cheating".into())),
            Ok("Banned user! The ban ends at 2026-01-02 14:00:00 UTC.".into())
        );
        let kicks: Vec<_> = w.effects.drain().xmpp.into_iter().map(|p| (p.to, p.body)).collect();
        assert_eq!(kicks, [(100, "<NewsArticleTrans><ExpiryTime><".to_owned())], "a ban kicks the driver");
        let banned = ban_dao::find_by_user(w.db(), 1, datetime::now(&w.env)).unwrap().unwrap();
        assert_eq!((banned.reason.as_deref(), banned.banned_by_id), (Some("cheating"), None));
        assert_eq!(ban(&mut w, "player@example.com", None, None), Err("User is already banned!".into()));
        assert_eq!(unban(&mut w, "player@example.com"), Ok("Unbanned user!".into()));
        assert_eq!(unban(&mut w, "DRIVER"), Err("User is not banned!".into()));
    }

    #[test]
    fn passwords_and_admin_rights() {
        let mut w = world_with_driver();
        set_password(&mut w, "player@example.com", "hunter2").unwrap();
        let user = user::find_by_email(w.db(), "player@example.com").unwrap().unwrap();
        assert!(password::matches("f3bbbd66a63d4bf1747940578ec3d0103530e21d", user.password.as_deref().unwrap()));
        set_admin(&mut w, "player@example.com", true).unwrap();
        assert!(user::find_by_email(w.db(), "player@example.com").unwrap().unwrap().is_admin);
        assert_eq!(set_admin(&mut w, "nobody@example.com", true), Err("no such account".into()));
    }
}

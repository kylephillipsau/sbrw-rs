//! `AdminBO`: an admin's commands, sent as a report whose description starts
//! with `/`: `/kick`, `/ban [length] [reason]` and `/unban`. The admin hears
//! back in chat.

use time::{Duration, PrimitiveDateTime};

use crate::bo::util::misc_utils;
use crate::bo::{ban, token_session};
use crate::dao::persona::{self, PersonaEntity};
use crate::dao::user;
use crate::openfire::create_system_message;
use crate::{Fault, World, datetime};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CmdAction {
    Kick,
    Ban,
    Unban,
    Unknown,
}

/// `AdminBO.CommandInfo`: what a command asks for.
#[derive(Debug, Clone, PartialEq)]
struct CommandInfo {
    action: CmdAction,
    reason: Option<String>,
    time_end: Option<PrimitiveDateTime>,
}

/// `String.split(" ")`: no match gives the string itself; otherwise trailing
/// empty strings are dropped.
fn split_spaces(s: &str) -> Vec<&str> {
    if !s.contains(' ') {
        return vec![s];
    }
    let mut parts: Vec<&str> = s.split(' ').collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// `AdminBO.CommandInfo.parse`. `/ban` takes a length (`1d2h`) then a reason; with no length, every word
/// is the reason, and the ban is for good.
fn parse(w: &World, cmd: &str) -> Result<CommandInfo, Fault> {
    let cmd = cmd.replacen('/', "", 1);
    let split = split_spaces(&cmd);
    let first = split
        .first()
        .ok_or_else(|| anyhow::anyhow!("ArrayIndexOutOfBoundsException: Index 0 out of bounds for length 0"))?;
    let action = match first.to_lowercase().trim() {
        "ban" => CmdAction::Ban,
        "kick" => CmdAction::Kick,
        "unban" => CmdAction::Unban,
        _ => CmdAction::Unknown,
    };
    let mut info = CommandInfo { action, reason: None, time_end: None };
    if action == CmdAction::Ban && split.len() >= 2 {
        let given_time = misc_utils::length_to_miliseconds(split[1])?;
        if given_time != 0 {
            info.time_end = Some(datetime::now(&w.env) + Duration::seconds(given_time / 1000));
            if split.len() > 2 {
                info.reason = Some(misc_utils::args_to_string(&split, 2, split.len())?);
            }
        } else {
            info.reason = Some(misc_utils::args_to_string(&split, 1, split.len())?);
        }
    }
    Ok(info)
}

fn reply(w: &World, message: &str, persona_id: i64) {
    w.openfire.send_string(&create_system_message(message), persona_id, &w.effects);
}

// port: bo/AdminBO#sendCommand(Long,Long,String)
/// Run the admin's command against the driver. A driver that doesn't exist
/// or a command that isn't one does nothing.
pub fn send_command(w: &mut World, persona_id: i64, abuser_persona_id: i64, command: &str) -> Result<(), Fault> {
    let command_info = parse(w, command)?;
    let Some(persona_entity) = persona::find(w.db(), abuser_persona_id)? else { return Ok(()) };
    let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    let user_entity = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;

    match command_info.action {
        CmdAction::Ban => {
            if ban::is_banned(w, &user_entity)? {
                reply(w, "User is already banned!", persona_id);
                return Ok(());
            }
            let banned_by = persona::find(w.db(), persona_id)?;
            send_ban(w, &persona_entity, banned_by.as_ref(), command_info.time_end, command_info.reason)?;
            reply(w, "Banned user!", persona_id);
        }
        CmdAction::Kick => {
            send_kick(w, user_entity.id, persona_entity.persona_id)?;
            reply(w, "Kicked user!", persona_id);
        }
        CmdAction::Unban => {
            let Some(existing_ban) = ban::get_user_ban(w, &user_entity)? else {
                reply(w, "User is not banned!", persona_id);
                return Ok(());
            };
            ban::expire_ban(w, existing_ban)?;
            reply(w, "Unbanned user!", persona_id);
        }
        CmdAction::Unknown => {}
    }
    Ok(())
}

// port: bo/AdminBO#sendBan(PersonaEntity,PersonaEntity,LocalDateTime,String)
fn send_ban(
    w: &mut World,
    persona_entity: &PersonaEntity,
    banned_by: Option<&PersonaEntity>,
    ends_on: Option<PrimitiveDateTime>,
    reason: Option<String>,
) -> Result<(), Fault> {
    let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    let user_entity = user::find(w.db(), user_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    ban::ban_user(w, &user_entity, banned_by, reason, ends_on)?;
    send_kick(w, user_entity.id, persona_entity.persona_id)
}

// port: bo/AdminBO#sendKick(Long,Long)
/// A push the game can't read, which makes it leave, and the end of the
/// user's session.
fn send_kick(w: &mut World, user_id: i64, persona_id: i64) -> Result<(), Fault> {
    w.openfire.send_string("<NewsArticleTrans><ExpiryTime><", persona_id, &w.effects);
    token_session::delete_by_user_id(w, user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_read_as_upstream_reads_them() {
        let w = World::test(&[]);
        let now = datetime::now(&w.env);
        let read = |cmd: &str| parse(&w, cmd).unwrap();
        assert_eq!(read("/ban 1d Being rude").time_end, Some(now + Duration::days(1)));
        assert_eq!(read("/ban 1d Being rude").reason.as_deref(), Some("Being rude"));
        assert_eq!(read("/ban 1d").reason, None);
        let forever = read("/ban Cheating badly");
        assert_eq!((forever.time_end, forever.reason.as_deref()), (None, Some("Cheating badly")));
        assert_eq!(read("/ban").reason, None);
        assert_eq!(read("/KICK").action, CmdAction::Kick);
        assert_eq!(read("/ban  two spaces").reason.as_deref(), Some(" two spaces"));
        assert_eq!(read("/dance").action, CmdAction::Unknown);
        assert_eq!(read("/unban trailing   ").action, CmdAction::Unban);
        assert!(parse(&w, "/ ").is_err(), "nothing left after the split");
    }
}

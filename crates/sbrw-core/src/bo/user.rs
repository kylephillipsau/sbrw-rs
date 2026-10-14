//! `UserBO`: accounts, and what the game does with them once logged in.

use sbrw_java::validator::is_valid_email;

use sbrw_xml::dto::http::{ArrayOfProfileData, ProfileData, User, UserInfo};

use crate::bo::achievement;
use crate::bo::achievement_transaction::{Binding, Bindings};
use crate::dao::invite_ticket::{self, InviteTicketEntity};
use crate::dao::persona;
use crate::dao::user::{self, UserEntity};
use crate::vo::LoginStatus;
use crate::{Fault, World, datetime, password};

// port: bo/UserBO#createXmppUser(UserInfo)
/// Give each persona an XMPP account whose password is the first 16
/// characters of the session token.
pub fn create_xmpp_user(w: &mut World, user_info: &UserInfo) -> Result<(), Fault> {
    let security_token = user_info
        .user
        .as_ref()
        .and_then(|u| u.security_token.as_deref())
        .ok_or_else(|| Fault::null_pointer("UserInfo.user.securityToken"))?;
    let xmpp_passwd = security_token.get(..16).ok_or_else(|| {
        anyhow::anyhow!("StringIndexOutOfBoundsException: begin 0, end 16, length {}", security_token.len())
    })?;
    let personas = user_info.personas.as_ref().ok_or_else(|| Fault::null_pointer("UserInfo.personas"))?;
    for persona in &personas.profile_data {
        create_xmpp_user_for_persona(w, persona.persona_id, xmpp_passwd);
    }
    Ok(())
}

// port: bo/UserBO#createXmppUser(Long,String)
pub fn create_xmpp_user_for_persona(w: &mut World, persona_id: i64, xmpp_passwd: &str) {
    w.openfire.create_update_persona(persona_id, xmpp_passwd, &w.effects);
}

// port: bo/UserBO#createUser(String,String,String)
pub fn create_user(w: &mut World, email: Option<&str>, passwd: Option<&str>, ip: &str) -> Result<UserEntity, Fault> {
    let now = datetime::now(&w.env);
    let mut user = UserEntity::new();
    user.email = email.map(str::to_owned);
    user.password = passwd.map(password::hash);
    user.ip_address = Some(ip.to_owned());
    user.created = Some(now);
    user.last_login = Some(now);
    user::insert(w.db(), &mut user)?;
    Ok(user)
}

// port: bo/UserBO#createUserWithTicket(String,String,String,String)
pub fn create_user_with_ticket(
    w: &mut World,
    email: Option<&str>,
    passwd: Option<&str>,
    ip: &str,
    ticket: Option<&str>,
) -> Result<LoginStatus, Fault> {
    let mut status = LoginStatus::new(0, "", false);
    if !email.is_some_and(is_valid_email) {
        status.vo.description = Some("Registration Error: Invalid Email Format!".into());
        return Ok(status);
    }
    let email = email.expect("validated above");

    let mut invite = InviteTicketEntity { ticket: Some("empty-ticket".into()), ..Default::default() };
    let ticket_token = w.params.get_str_param("TICKET_TOKEN");
    if ticket_token.is_some_and(|t| t != "null") {
        match invite_ticket::find_by_ticket(w.db(), ticket)? {
            Some(found) if found.ticket.as_deref().is_some_and(|t| !t.is_empty()) => invite = found,
            _ => {
                status.vo.description = Some("Registration Error: Invalid Ticket!".into());
                return Ok(status);
            }
        }
        if invite.user(w.db())?.is_some() {
            status.vo.description = Some("Registration Error: Ticket already in use!".into());
            return Ok(status);
        }
    }

    if user::find_by_email(w.db(), email)?.is_some() {
        status.vo.description = Some("Registration Error: Registration limit reached for email!".into());
        return Ok(status);
    }

    let user = create_user(w, Some(email), passwd, ip)?;
    invite.user_id = Some(user.id);
    if invite.id == 0 {
        invite_ticket::insert(w.db(), &mut invite)?;
    } else {
        // Upstream persists the ticket it loaded in this transaction: a no-op
        // on a managed entity, whose new user is flushed at commit.
        invite_ticket::update(w.db(), &invite)?;
    }
    Ok(LoginStatus::new(user.id, "", true))
}

// port: bo/UserBO#secureLoginPersona(Long,Long)
/// The game picked a driver: record the login, run the LOGIN achievements,
/// and remember which driver the account last used.
pub fn secure_login_persona(w: &mut World, user_id: i64, persona_id: i64) -> Result<(), Fault> {
    let Some(mut persona_entity) = persona::find(w.db(), persona_id)? else { return Ok(()) };
    let owner = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    let mut user = user::find(w.db(), owner)?.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
    if user.id != user_id {
        return Ok(());
    }
    let now = datetime::now(&w.env);
    if persona_entity.first_login.is_none() {
        persona_entity.first_login = Some(now);
    }
    persona_entity.last_login = Some(now);
    persona::update(w.db(), &persona_entity)?;
    let mut transaction = achievement::create_transaction(persona_id);
    transaction.add("LOGIN", Bindings::from([("persona".into(), Binding::Persona(persona_entity.clone()))]))?;
    achievement::commit_transaction(w, &mut persona_entity, &mut transaction)?;
    let index = user.personas.iter().position(|p| p.persona_id == persona_id).map_or(-1, |i| i as i32);
    user.selected_persona_index = Some(index);
    user::update(w.db(), &user)?;
    Ok(())
}

// port: bo/UserBO#getUserInfo(UserEntity)
/// What `GetPermanentSession` tells the game about the account: its personas
/// as `user` holds them, and which one to select.
pub fn get_user_info(user: &UserEntity) -> Result<UserInfo, Fault> {
    let profile_data = user
        .personas
        .iter()
        .map(|p| ProfileData {
            name: p.name.clone(),
            cash: p.cash,
            icon_index: p.icon_index,
            persona_id: p.persona_id,
            level: p.level,
            boost: p.boost,
            ..Default::default()
        })
        .collect();
    Ok(UserInfo {
        personas: Some(ArrayOfProfileData { profile_data }),
        user: Some(User { user_id: user.id, ..Default::default() }),
        default_persona_idx: user
            .selected_persona_index
            .ok_or_else(|| Fault::null_pointer("UserEntity.selectedPersonaIndex"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dao::persona::PersonaEntity;
    use sbrw_xml::ToXml;

    fn persona(persona_id: i64, name: &str) -> PersonaEntity {
        PersonaEntity {
            persona_id,
            name: Some(name.into()),
            cash: 250_000.0,
            boost: 12.5,
            icon_index: 3,
            level: 7,
            motto: Some("not sent".into()),
            rep: 99.0,
            score: 5,
            ..Default::default()
        }
    }

    #[test]
    fn user_info_lists_the_personas_the_user_was_loaded_with() {
        let mut user = UserEntity::new();
        user.id = 4;
        user.selected_persona_index = Some(1);
        user.personas = vec![persona(100, "Alpha"), persona(101, "Bravo")];
        let info = get_user_info(&user).unwrap();
        assert_eq!(info.default_persona_idx, 1);
        let profiles = &info.personas.as_ref().unwrap().profile_data;
        assert_eq!(
            profiles[1],
            ProfileData {
                name: Some("Bravo".into()),
                cash: 250_000.0,
                boost: 12.5,
                icon_index: 3,
                level: 7,
                persona_id: 101,
                ..Default::default()
            },
            "only these six properties are copied"
        );
        assert!(info.to_xml().starts_with("<UserInfo><defaultPersonaIdx>1</defaultPersonaIdx><personas><ProfileData>"));

        user.selected_persona_index = None;
        assert!(matches!(get_user_info(&user), Err(Fault::Internal(_))));
    }

    #[test]
    fn xmpp_accounts_use_the_first_16_characters_of_the_token() {
        let mut w = World::test(&[]);
        let mut user = UserEntity::new();
        user.personas = vec![persona(100, "Alpha"), persona(101, "Bravo")];
        let mut info = get_user_info(&user).unwrap();
        info.user.as_mut().unwrap().security_token = Some("00000000-0000-4000-8000-000000000002".into());
        create_xmpp_user(&mut w, &info).unwrap();
        let calls: Vec<String> =
            w.effects.drain().openfire.into_iter().map(|c| serde_json::json!([c.call, c.args]).to_string()).collect();
        assert_eq!(
            calls,
            [
                r#"["createUpdatePersona",[100,"00000000-0000-40"]]"#,
                r#"["createUpdatePersona",[101,"00000000-0000-40"]]"#,
            ]
        );
        assert_eq!(w.openfire.password("sbrw.101").as_deref(), Some("00000000-0000-40"));
    }

    #[test]
    fn secure_login_persona_ignores_personas_of_other_users() {
        let mut w = World::test(&[]);
        let mut owner = UserEntity::new();
        user::insert(w.db(), &mut owner).unwrap();
        w.db()
            .execute_batch(
                "INSERT INTO persona (ID, boost, cash, curCarIndex, iconIndex, level, percentToLevel, rating, rep,
                 repAtCurrentLevel, score, USERID) VALUES (100, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1)",
            )
            .unwrap();
        let untouched = persona::find(w.db(), 100).unwrap();
        w.transaction(|w| secure_login_persona(w, 2, 100)).unwrap();
        w.transaction(|w| secure_login_persona(w, 1, 555)).unwrap();
        assert_eq!(persona::find(w.db(), 100).unwrap(), untouched);

        assert!(w.effects.drain().is_empty());

        // The owner's login: first and last login set, the LOGIN achievement
        // push, and the driver's index remembered.
        w.db().execute_batch("UPDATE user SET selectedPersonaIndex = 5 WHERE ID = 1").unwrap();
        w.transaction(|w| secure_login_persona(w, 1, 100)).unwrap();
        let logged_in = persona::find(w.db(), 100).unwrap().unwrap();
        let now = Some(time::macros::datetime!(2026-01-01 12:00));
        assert_eq!((logged_in.first_login, logged_in.last_login), (now, now));
        assert_eq!(user::find(w.db(), 1).unwrap().unwrap().selected_persona_index, Some(0));
        let pushes = w.effects.drain().xmpp;
        assert_eq!(pushes.iter().map(|p| p.to).collect::<Vec<_>>(), [100]);
    }
}

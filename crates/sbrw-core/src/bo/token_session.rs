//! `TokenSessionBO`: login, and the in-memory sessions behind every
//! authenticated request.

use std::collections::HashMap;

use anyhow::anyhow;
use sbrw_java::time::format_full_utc;

use crate::bo::{authentication, hardware_info};
use crate::dao::user::{self, UserEntity};
use crate::engine::EngineExceptionCode;
use crate::vo::LoginStatus;
use crate::{Fault, World, datetime, password};
use sbrw_xml::dto::login::LoginStatusVOBan;

/// `TokenSessionEntity`. Like upstream it holds a copy of the user (with its
/// personas) as of when the session was created, not a live view.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenSession {
    pub security_token: String,
    /// `Date.getTime()` of the expiry.
    pub expiration_millis: i64,
    pub user: UserEntity,
    pub premium: bool,
    pub client_host_ip: String,
    pub active_persona_id: Option<i64>,
    pub event_session_id: Option<i64>,
    pub active_lobby_id: Option<i64>,
    pub relay_crypto_ticket: Option<String>,
    pub allowed_persona_ids: Vec<i64>,
}

/// The singleton's two maps.
#[derive(Debug, Default)]
pub struct TokenSessions {
    session_key_to_token: HashMap<String, TokenSession>,
    user_id_to_session_key: HashMap<i64, String>,
}

impl TokenSessions {
    /// The live session for a token, to change it as upstream changes the
    /// shared `TokenSessionEntity`.
    pub fn get_mut(&mut self, security_token: &str) -> Option<&mut TokenSession> {
        self.session_key_to_token.get_mut(security_token)
    }
}

fn now_millis(w: &World) -> i64 {
    (w.env.now().unix_timestamp_nanos() / 1_000_000) as i64
}

// port: bo/TokenSessionBO#createToken(UserEntity,String)
pub fn create_token(w: &mut World, user: &UserEntity, client_host_name: &str) -> Result<String, Fault> {
    let expiration_millis = get_minutes(w, w.params.get_int_param_or("SESSION_LENGTH_MINUTES", 130)?);
    let random_uuid = w.env.uuid().to_string();
    let session = TokenSession {
        security_token: random_uuid.clone(),
        expiration_millis,
        user: user.clone(),
        premium: user.premium,
        client_host_ip: client_host_name.to_owned(),
        active_persona_id: Some(0),
        event_session_id: None,
        active_lobby_id: None,
        relay_crypto_ticket: None,
        allowed_persona_ids: user.personas.iter().map(|p| p.persona_id).collect(),
    };
    w.tokens.session_key_to_token.insert(random_uuid.clone(), session);
    w.tokens.user_id_to_session_key.insert(user.id, random_uuid.clone());
    Ok(random_uuid)
}

// port: bo/TokenSessionBO#validateToken(Long,String)
/// The session for `security_token` if it belongs to `user_id` and hasn't expired.
pub fn validate_token(w: &mut World, user_id: i64, security_token: &str) -> Result<TokenSession, Fault> {
    let session = match w.tokens.session_key_to_token.get(security_token) {
        Some(s) if s.user.id == user_id => s.clone(),
        _ => return Err(Fault::NotAuthorized("Invalid Token".into())),
    };
    if now_millis(w) > session.expiration_millis {
        delete_by_user_id(w, user_id)?;
        return Err(Fault::NotAuthorized("Expired Token".into()));
    }
    Ok(session)
}

// port: bo/TokenSessionBO#findByUserId(Long)
pub fn find_by_user_id(w: &World, user_id: i64) -> Result<Option<TokenSession>, Fault> {
    match w.tokens.user_id_to_session_key.get(&user_id) {
        None => Ok(None),
        Some(key) => w
            .tokens
            .session_key_to_token
            .get(key)
            .cloned()
            .map(Some)
            .ok_or_else(|| anyhow!("User {user_id} has session key, but session is missing!").into()),
    }
}

// port: bo/TokenSessionBO#removeSession(String)
pub fn remove_session(w: &mut World, session_key: &str) -> Result<(), Fault> {
    w.tokens
        .session_key_to_token
        .remove(session_key)
        .map(drop)
        .ok_or_else(|| anyhow!("Tried to remove session {session_key}, but it could not be found in the store!").into())
}

// port: bo/TokenSessionBO#deleteByUserId(Long)
pub fn delete_by_user_id(w: &mut World, user_id: i64) -> Result<(), Fault> {
    match w.tokens.user_id_to_session_key.remove(&user_id) {
        Some(key) => remove_session(w, &key),
        None => Ok(()),
    }
}

// port: bo/TokenSessionBO#login(String,String,HttpServletRequest)
/// The launcher's login. `remote_host` is the client address
/// (`HttpServletRequest.getRemoteHost()`).
pub fn login(
    w: &mut World,
    email: Option<&str>,
    password: Option<&str>,
    remote_host: &str,
) -> Result<LoginStatus, Fault> {
    let mut status = LoginStatus::new(0, "", false);
    if let (Some(email), Some(password)) = (email.filter(|e| !e.is_empty()), password.filter(|p| !p.is_empty()))
        && let Some(mut user) = user::find_by_email(w.db(), email)?
        && user.password.as_deref().is_some_and(|stored| password::matches(password, stored))
    {
        if user.is_locked {
            status.vo.description = Some("Account locked. Contact an administrator.".into());
            return Ok(status);
        }
        if let Some(hash) = &user.game_hardware_hash
            && hardware_info::is_hardware_hash_banned(w, hash)?
        {
            status.vo.ban = Some(LoginStatusVOBan {
                reason: Some("Your computer is banned from this server.".into()),
                expires: None,
            });
            return Ok(status);
        }
        if let Some(ban) = authentication::check_user_ban(w, &user)? {
            status.vo.ban = Some(LoginStatusVOBan { reason: ban.reason, expires: ban.ends_at.map(format_full_utc) });
            return Ok(status);
        }
        user.last_login = Some(datetime::now(&w.env));
        user::update(w.db(), &user)?;
        delete_by_user_id(w, user.id)?;
        let token = create_token(w, &user, remote_host)?;
        let mut ok = LoginStatus::new(user.id, &token, true);
        ok.vo.description = Some(String::new());
        return Ok(ok);
    }
    status.vo.description = Some("LOGIN ERROR".into());
    Ok(status)
}

// port: bo/TokenSessionBO#verifyPersonaOwnership(TokenSessionEntity,Long)
pub fn verify_persona_ownership(session: &TokenSession, persona_id: i64) -> Result<(), Fault> {
    if session.allowed_persona_ids.contains(&persona_id) {
        Ok(())
    } else {
        Err(Fault::engine(EngineExceptionCode::RemotePersonaDoesNotBelongToUser, true))
    }
}

// port: bo/TokenSessionBO#setActivePersonaId(TokenSessionEntity,Long)
pub fn set_active_persona_id(w: &mut World, security_token: &str, persona_id: i64) -> Result<(), Fault> {
    let Some(session) = w.tokens.get_mut(security_token) else { return Ok(()) };
    if persona_id != 0 {
        verify_persona_ownership(session, persona_id)?;
    }
    session.active_persona_id = Some(persona_id);
    Ok(())
}

// port: bo/TokenSessionBO#setActiveLobbyId(TokenSessionEntity,Long)
pub fn set_active_lobby_id(w: &mut World, security_token: &str, lobby_id: Option<i64>) {
    if let Some(session) = w.tokens.get_mut(security_token) {
        session.active_lobby_id = lobby_id;
    }
}

// port: bo/TokenSessionBO#setEventSessionId(TokenSessionEntity,Long)
pub fn set_event_session_id(w: &mut World, security_token: &str, event_session_id: Option<i64>) {
    if let Some(session) = w.tokens.get_mut(security_token) {
        session.event_session_id = event_session_id;
    }
}

// port: bo/TokenSessionBO#setRelayCryptoTicket(TokenSessionEntity,String)
pub fn set_relay_crypto_ticket(w: &mut World, security_token: &str, ticket: Option<String>) {
    if let Some(session) = w.tokens.get_mut(security_token) {
        session.relay_crypto_ticket = ticket;
    }
}

// port: bo/TokenSessionBO#getMinutes(int)
fn get_minutes(w: &World, minutes: i32) -> i64 {
    now_millis(w) + i64::from(minutes) * 60_000
}

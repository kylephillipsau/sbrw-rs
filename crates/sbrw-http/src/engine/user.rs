//! `api/User`: the launcher's login and registration, and the game's session
//! calls. As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::bo::{matchmaking, online_users, presence, token_session, user};
use sbrw_core::{EngineExceptionCode, Fault};
use sbrw_xml::ToXml;

use super::{Call, entity};
use crate::AppState;

// port: api/User#authenticateUser(String,String)
pub(super) async fn authenticate_user(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (email, password) = (call.query("email"), call.query("password"));
    let remote_host = call.remote_addr.to_string();
    let status = state
        .world()
        .call(move |w| token_session::login(w, email.as_deref(), password.as_deref(), &remote_host))
        .await?;
    let code = if status.login_ok { StatusCode::OK } else { StatusCode::INTERNAL_SERVER_ERROR };
    Ok(entity(call, code, status.vo.to_xml()))
}

// port: api/User#createUser(String,String,String)
pub(super) async fn create_user(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (email, password, ticket) = (call.query("email"), call.query("password"), call.query("inviteTicket"));
    let ip = call.remote_addr.to_string();
    let world = state.world();
    let status = {
        let (email, password, ip) = (email.clone(), password.clone(), ip.clone());
        world
            .call(move |w| {
                user::create_user_with_ticket(w, email.as_deref(), password.as_deref(), &ip, ticket.as_deref())
            })
            .await?
    };
    if status.login_ok {
        // Registration logs the new account in, in a second transaction.
        let status = world.call(move |w| token_session::login(w, email.as_deref(), password.as_deref(), &ip)).await?;
        return Ok(entity(call, StatusCode::OK, status.vo.to_xml()));
    }
    Ok(entity(call, StatusCode::INTERNAL_SERVER_ERROR, status.vo.to_xml()))
}

// port: api/User#getPermanentSession()
/// The game's first call: swaps the launcher's token for a new one.
pub(super) async fn get_permanent_session(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let world = state.world();
    let number_of_users_online_now = world
        .call(|w| {
            online_users::get_online_users_stats(w)
                .map(|stats| stats.number_of_online)
                .ok_or_else(|| Fault::null_pointer("OnlineUsersBO.getOnlineUsersStats()"))
        })
        .await?;
    let max_online_players = world.call(|w| w.params.get_int_param_or("MAX_ONLINE_PLAYERS", -1)).await?;
    if max_online_players != -1 && number_of_users_online_now >= i64::from(max_online_players) {
        return Err(Fault::engine(EngineExceptionCode::MaximumUsersLoggedInHardCapReached, true));
    }

    let user_entity = call.session().user.clone();
    let user_id = user_entity.id;
    world.call(move |w| token_session::delete_by_user_id(w, user_id)).await?;
    let random_uuid = {
        let (user_entity, remote_host) = (user_entity.clone(), call.remote_addr.to_string());
        world.call(move |w| token_session::create_token(w, &user_entity, &remote_host)).await?
    };
    let mut user_info = user::get_user_info(&user_entity)?;
    user_info.user.as_mut().expect("getUserInfo sets the user").security_token = Some(random_uuid);
    let user_info = world
        .call(move |w| {
            user::create_xmpp_user(w, &user_info)?;
            Ok(user_info)
        })
        .await?;
    Ok(entity(call, StatusCode::OK, user_info.to_xml()))
}

// port: api/User#secureLoginPersona(Long)
pub(super) async fn secure_login_persona(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = call.query_long("personaId")?.ok_or_else(|| Fault::null_pointer("personaId"))?;
    let world = state.world();
    let session = call.session();
    let token = session.security_token.clone();
    world.call(move |w| token_session::set_active_persona_id(w, &token, persona_id)).await?;
    let user_id = session.user.id;
    world.call(move |w| user::secure_login_persona(w, user_id, persona_id)).await?;
    // Upstream: "Why is this here? Weird things happen sometimes."
    world
        .call(move |w| {
            matchmaking::remove_player_from_queue(w, persona_id);
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/User#secureLogoutPersona(Long)
pub(super) async fn secure_logout_persona(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = call.query_long("personaId")?.ok_or_else(|| Fault::null_pointer("personaId"))?;
    let world = state.world();
    let session = call.session();
    if let Some(active_persona_id) = session.active_persona_id.filter(|id| *id == persona_id) {
        world.call(move |w| presence::remove_presence(w, active_persona_id)).await?;
        world
            .call(move |w| {
                matchmaking::remove_player_from_queue(w, active_persona_id);
                Ok(())
            })
            .await?;
    }
    let token = session.security_token.clone();
    world.call(move |w| token_session::set_active_persona_id(w, &token, 0)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/User#secureLogout()
pub(super) async fn secure_logout(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let world = state.world();
    let session = call.session();
    if let Some(active_persona_id) = session.active_persona_id.filter(|id| *id != 0) {
        let token = session.security_token.clone();
        world.call(move |w| token_session::set_active_persona_id(w, &token, 0)).await?;
        world.call(move |w| presence::remove_presence(w, active_persona_id)).await?;
    }
    let user_id = session.user.id;
    world.call(move |w| token_session::delete_by_user_id(w, user_id)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

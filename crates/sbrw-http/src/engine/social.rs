//! Friends and blocks: `api/AddFriendRequest`, `ResolveFriendsRequest`,
//! `RemoveFriend`, `GetFriendListFromUserId`, `BlockPlayer`, `UnblockPlayer`,
//! `GetBlockedUserList` and `GetBlockersByUsers`, one resource each upstream;
//! and reports (`api/Social`), through which admins send commands.

use anyhow::anyhow;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sbrw_core::Fault;
use sbrw_core::bo::{admin, social, social_relationship};
use sbrw_xml::ToXml;

use super::{Call, entity};
use crate::AppState;

/// `RequestSessionInfo.getActivePersonaId()`, which the BO looks up: no
/// driver selected is a lookup by a null id.
fn active_persona_id(call: &Call) -> Result<i64, Fault> {
    Ok(call.session().active_persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?)
}

/// The `userId` header, which `AuthenticationFilter` has checked.
fn user_id_header(call: &Call) -> Result<i64, Fault> {
    call.header_long("userId")?.ok_or_else(|| Fault::null_pointer("userId"))
}

// port: api/AddFriendRequest#addFriendRequest(String,String)
pub(super) async fn add_friend_request(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let display_name = call.query("displayName");
    let active_persona_id = active_persona_id(call)?;
    let friend_result = state
        .world()
        .call(move |w| social_relationship::add_friend(w, active_persona_id, display_name.as_deref()))
        .await?;
    Ok(entity(call, StatusCode::OK, friend_result.to_xml()))
}

// port: api/ResolveFriendsRequest#resolveFriendsRequest(Long,int)
/// A rejection answers `Response.ok(null)`: no body and no content type.
pub(super) async fn resolve_friends_request(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let friend_persona_id = call.query_long("friendPersonaId")?;
    let resolution = call.query_int("resolution")?.unwrap_or_default();
    let active_persona_id =
        call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
    let persona_base = state
        .world()
        .call(move |w| {
            social_relationship::resolve_friends_request(w, active_persona_id, friend_persona_id, resolution)
        })
        .await?;
    Ok(match persona_base {
        Some(persona_base) => entity(call, StatusCode::OK, persona_base.to_xml()),
        None => StatusCode::OK.into_response(),
    })
}

// port: api/RemoveFriend#removefriend(Long)
pub(super) async fn removefriend(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let friend_persona_id = call.query_long("friendPersonaId")?;
    let active_persona_id = active_persona_id(call)?;
    let persona_base = state
        .world()
        .call(move |w| social_relationship::remove_friend(w, active_persona_id, friend_persona_id))
        .await?;
    Ok(entity(call, StatusCode::OK, persona_base.to_xml()))
}

// port: api/GetFriendListFromUserId#getFriendsList(Long)
pub(super) async fn get_friend_list_from_user_id(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let user_id = user_id_header(call)?;
    let list = state.world().call(move |w| social_relationship::get_friends_list(w, user_id)).await?;
    Ok(entity(call, StatusCode::OK, list.to_xml()))
}

// port: api/BlockPlayer#blockPlayer(String,Long)
pub(super) async fn block_player(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let other_persona_id = call.query_long("otherPersonaId")?;
    let user_id = call.session().user.id;
    let active_persona_id = active_persona_id(call)?;
    let persona_base = state
        .world()
        .call(move |w| social_relationship::block_player(w, user_id, active_persona_id, other_persona_id))
        .await?;
    Ok(entity(call, StatusCode::OK, persona_base.to_xml()))
}

// port: api/UnblockPlayer#unblockPlayer(Long,String,Long)
pub(super) async fn unblock_player(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let other_persona_id = call.query_long("otherPersonaId")?;
    let user_id = user_id_header(call)?;
    let persona_base =
        state.world().call(move |w| social_relationship::unblock_player(w, user_id, other_persona_id)).await?;
    Ok(entity(call, StatusCode::OK, persona_base.to_xml()))
}

// port: api/GetBlockedUserList#getBlockedUserList(Long)
pub(super) async fn get_blocked_user_list(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let user_id = user_id_header(call)?;
    let list = state.world().call(move |w| social_relationship::get_blocked_user_list(w, user_id)).await?;
    Ok(entity(call, StatusCode::OK, list.to_xml()))
}

// port: api/GetBlockersByUsers#getBlockedUserList(Long,String)
pub(super) async fn get_blockers_by_users(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = active_persona_id(call)?;
    let blockers =
        state.world().call(move |w| social_relationship::get_blockers_by_users(w, active_persona_id)).await?;
    Ok(entity(call, StatusCode::OK, blockers.to_xml()))
}

// port: api/Social#petition(String,Long,Long,Integer,String,Integer,Integer)
/// A report against a driver; from an admin, a description starting with `/`
/// is a command instead.
pub(super) async fn petition(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = call.query_long("personaId")?;
    let abuser_persona_id = call.query_long("abuserPersonaId")?;
    let petition_type = call.query_int("petitionType")?;
    let description = call.query("description");
    let custom_car_id = call.query_int("customCarID")?;
    let chat_minutes = call.query_int("chatMinutes")?;
    let session = call.session();
    let is_command = session.user.is_admin
        && description.as_deref().ok_or_else(|| Fault::null_pointer("description"))?.starts_with('/');
    if is_command {
        let active_persona_id =
            session.active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
        let abuser_persona_id = abuser_persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
        let command = description.unwrap_or_default();
        state.world().call(move |w| admin::send_command(w, active_persona_id, abuser_persona_id, &command)).await?;
    } else {
        state
            .world()
            .call(move |w| {
                social::send_report(
                    w,
                    persona_id,
                    abuser_persona_id,
                    petition_type,
                    description,
                    custom_car_id,
                    chat_minutes,
                    0,
                )
            })
            .await?;
    }
    Ok(entity(call, StatusCode::OK, String::new()))
}

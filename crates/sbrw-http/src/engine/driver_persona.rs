//! `api/DriverPersona`: creating and deleting drivers, their profiles,
//! presence and motto. As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::bo::{driver_persona, presence, token_session, user};
use sbrw_core::dao::persona::PersonaEntity;
use sbrw_core::{EngineExceptionCode, Fault};
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{PersonaIdArray, PersonaMotto};

use super::{Call, entity};
use crate::AppState;

// port: api/DriverPersona#getExpLevelPointsMap()
pub(super) async fn get_exp_level_points_map(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let map = state.world().call(|w| driver_persona::get_exp_level_points_map(w)).await?;
    Ok(entity(call, StatusCode::OK, map.to_xml()))
}

// port: api/DriverPersona#getPersonaInfo(Long)
pub(super) async fn get_persona_info(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = call.query_long("personaId")?;
    let profile_data = state
        .world()
        .call(move |w| {
            // `personaDao.find(null)`: Hibernate refuses a null id.
            let persona_id = persona_id.ok_or_else(|| anyhow::anyhow!("id to load is required for loading"))?;
            driver_persona::get_persona_info(w, persona_id)
        })
        .await?;
    Ok(entity(call, StatusCode::OK, profile_data.to_xml()))
}

// port: api/DriverPersona#reserveName(String)
pub(super) async fn reserve_name(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let name = call.query("name");
    let result = state.world().call(move |w| driver_persona::reserve_name(w, name.as_deref())).await?;
    Ok(entity(call, StatusCode::OK, result.to_xml()))
}

// port: api/DriverPersona#UnreserveName(String)
pub(super) fn unreserve_name(call: &Call) -> Result<Response, Fault> {
    Ok(entity(call, StatusCode::OK, String::new()))
}

/// `NAME_PATTERN`: `^[A-Z0-9]{3,15}$`, matching the whole name.
fn is_allowed_name(name: &str) -> bool {
    (3..=15).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

// port: api/DriverPersona#createPersona(Long,String,String,int,String,String)
/// A new driver for the account. The clan parameters are ignored.
pub(super) async fn create_persona(state: &AppState, call: &Call) -> Result<Response, Fault> {
    // RESTEasy reads every parameter before the method runs. The filter has
    // already checked both headers; `iconIndex` is a primitive int, 0 when absent.
    let session = call.session();
    let (user_id, security_token) = (session.user.id, session.security_token.clone());
    let name = call.query("name");
    let icon_index = call.query_int("iconIndex")?.unwrap_or(0);

    let name = name.ok_or_else(|| Fault::null_pointer("name"))?;
    if !is_allowed_name(&name) {
        return Err(Fault::engine(EngineExceptionCode::DisplayNameNotAllowed, true));
    }

    let world = state.world();
    let name_reserve_result = {
        let name = name.clone();
        world.call(move |w| driver_persona::reserve_name(w, Some(&name))).await?
    };
    if !name_reserve_result.string.is_empty() {
        return Err(Fault::engine(EngineExceptionCode::DisplayNameDuplicate, true));
    }

    let persona_entity = PersonaEntity { name: Some(name), icon_index, ..Default::default() };
    // Upstream then checks for a null result, which createPersona never returns.
    let persona = world.call(move |w| driver_persona::create_persona(w, user_id, persona_entity)).await?;

    let persona_id = persona.persona_id;
    let token = security_token.clone();
    world
        .call(move |w| {
            // The request's session is the live one: the new driver may log in.
            if let Some(session) = w.tokens.get_mut(&token)
                && !session.allowed_persona_ids.contains(&persona_id)
            {
                session.allowed_persona_ids.push(persona_id);
            }
            Ok(())
        })
        .await?;
    world
        .call(move |w| {
            let xmpp_passwd = security_token.get(..16).ok_or_else(|| {
                anyhow::anyhow!("StringIndexOutOfBoundsException: begin 0, end 16, length {}", security_token.len())
            })?;
            user::create_xmpp_user_for_persona(w, persona_id, xmpp_passwd);
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, persona.to_xml()))
}

/// `verifyPersonaOwnership` with a `Long` that may be null: no session owns null.
fn verify_persona_ownership(call: &Call, persona_id: Option<i64>) -> Result<i64, Fault> {
    match persona_id {
        Some(persona_id) => token_session::verify_persona_ownership(call.session(), persona_id).map(|()| persona_id),
        None => Err(Fault::engine(EngineExceptionCode::RemotePersonaDoesNotBelongToUser, true)),
    }
}

// port: api/DriverPersona#deletePersona(Long)
/// Delete one of the session's drivers; it may no longer be selected.
pub(super) async fn delete_persona(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = verify_persona_ownership(call, call.query_long("personaId")?)?;
    let world = state.world();
    world.call(move |w| driver_persona::delete_persona(w, persona_id)).await?;
    let token = call.session().security_token.clone();
    world
        .call(move |w| {
            if let Some(session) = w.tokens.get_mut(&token) {
                session.allowed_persona_ids.retain(|id| *id != persona_id);
            }
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, "<long>0</long>".into()))
}

// port: api/DriverPersona#getPersonaBaseFromList(InputStream)
/// Public profiles; no session needed.
pub(super) async fn get_persona_base_from_list(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id_array: PersonaIdArray = call.unmarshal()?;
    let persona_ids = persona_id_array.persona_ids.ok_or_else(|| Fault::null_pointer("PersonaIdArray.personaIds"))?;
    let array = state.world().call(move |w| driver_persona::get_persona_base_from_list(w, &persona_ids._long)).await?;
    Ok(entity(call, StatusCode::OK, array.to_xml()))
}

// port: api/DriverPersona#updatePersonaPresence(Long,Long)
/// The driver's presence; `PresenceBO.updatePresence` is asynchronous, so it
/// runs after the answer.
pub(super) async fn update_persona_presence(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = call.query_long("personaId")?;
    let new_presence = call.query_long("presence")?;
    let persona_id = verify_persona_ownership(call, persona_id)?;
    state
        .world()
        .call(move |w| {
            w.queue_async(move |w| match new_presence {
                Some(p) => presence::update_presence(w, persona_id, p),
                None if w.redis.is_none() => Ok(()),
                None => Err(Fault::null_pointer("presence")),
            });
            Ok(())
        })
        .await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/DriverPersona#getPersonaPresenceByName(String)
pub(super) async fn get_persona_presence_by_name(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let display_name = call.query("displayName");
    let presence =
        state.world().call(move |w| driver_persona::get_persona_presence_by_name(w, display_name.as_deref())).await?;
    Ok(entity(call, StatusCode::OK, presence.to_xml()))
}

// port: api/DriverPersona#updateStatusMessage(InputStream)
/// Set a driver's motto; the answer echoes the request.
pub(super) async fn update_status_message(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_motto: PersonaMotto = call.unmarshal()?;
    let persona_id = verify_persona_ownership(call, Some(persona_motto.persona_id))?;
    let message = persona_motto.message.clone();
    state.world().call(move |w| driver_persona::update_status_message(w, message, persona_id)).await?;
    Ok(entity(call, StatusCode::OK, persona_motto.to_xml()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_3_to_15_upper_case_letters_or_digits() {
        for ok in ["ABC", "DRIVER1", "123", "ABCDEFGHIJKLMNO"] {
            assert!(is_allowed_name(ok), "{ok}");
        }
        for bad in ["AB", "abc", "ABCDEFGHIJKLMNOP", "AB C", "ABC\n", "ÄBC", ""] {
            assert!(!is_allowed_name(bad), "{bad:?}");
        }
    }
}

//! `api/Badges`: the badges a driver shows.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::bo::persona;
use sbrw_core::{EngineExceptionCode, Fault};
use sbrw_xml::dto::http::BadgeBundle;

use super::{Call, entity};
use crate::AppState;

// port: api/Badges#set(InputStream,String)
pub(super) async fn set(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id =
        call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
    if active_persona_id == 0 {
        return Err(Fault::engine(EngineExceptionCode::FailedSessionSecurityPolicy, false));
    }
    let badge_bundle: BadgeBundle = call.unmarshal()?;
    state.world().call(move |w| persona::update_badges(w, active_persona_id, &badge_bundle)).await?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

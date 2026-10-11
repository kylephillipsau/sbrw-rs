//! `api/Reporting`: what the game reports about itself. Only the hardware
//! report does anything.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::{hardware_info, token_session};
use sbrw_core::dao::user;
use sbrw_xml::dto::http::HardwareInfo;

use super::{Call, entity};
use crate::AppState;

// port: api/Reporting#sendHardwareInfo(InputStream)
/// Store the client's hardware and link it to the account; a CPU that is
/// neither Intel's nor AMD's ends the session instead.
pub(super) async fn send_hardware_info(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let hardware_info: HardwareInfo = call.unmarshal()?;
    let cpuid0 = hardware_info.cpuid0.as_deref().ok_or_else(|| Fault::null_pointer("HardwareInfo.cpuid0"))?;
    let world = state.world();
    let mut user_entity = call.session().user.clone();
    if cpuid0 == "GenuineIntel" || cpuid0 == "AuthenticAMD" {
        let entity = world.call(move |w| hardware_info::save(w, hardware_info)).await?;
        // The session's copy of the user, as upstream merges it.
        user_entity.game_hardware_hash = entity.hardware_hash;
        world.call(move |w| Ok(user::update(w.db(), &user_entity)?)).await?;
    } else {
        let user_id = user_entity.id;
        world.call(move |w| token_session::delete_by_user_id(w, user_id)).await?;
    }
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/Reporting#sendUserSettings()
// port: api/Reporting#sendMultiplayerConnect()
// port: api/Reporting#sendClientPingTime()
// port: api/Reporting#loginAnnouncementClicked()
// port: api/Reporting#genericEmptyPut(String)
/// Every other report is accepted and ignored.
pub(super) fn ignored(call: &Call) -> Result<Response, Fault> {
    Ok(entity(call, StatusCode::OK, String::new()))
}

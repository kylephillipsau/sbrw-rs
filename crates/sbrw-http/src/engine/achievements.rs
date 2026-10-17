//! `api/Achievements` and `api/ReloadAchievements`: the achievement list, a
//! rank's reward, and reloading the definitions. As upstream, each call into
//! a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::achievement;
use sbrw_xml::ToXml;

use super::{Call, entity};
use crate::AppState;

/// `RequestSessionInfo.getActivePersonaId()`, which BOs dereference.
fn active_persona_id(call: &Call) -> Result<i64, Fault> {
    call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))
}

// port: api/Achievements#loadall()
pub(super) async fn loadall(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id = active_persona_id(call)?;
    let achievements_packet = state.world().call(move |w| achievement::load_all(w, persona_id)).await?;
    Ok(entity(call, StatusCode::OK, achievements_packet.to_xml()))
}

// port: api/Achievements#redeemreward(Long)
pub(super) async fn redeemreward(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let achievement_rank_id = call.query_long("achievementRankId")?;
    let persona_id = active_persona_id(call)?;
    let achievement_rewards =
        state.world().call(move |w| achievement::redeem_reward(w, persona_id, achievement_rank_id)).await?;
    Ok(entity(call, StatusCode::OK, achievement_rewards.to_xml()))
}

// port: api/ReloadAchievements#reloadAchievements(String)
/// With the admin token, read the achievement definitions again, rarities
/// and all.
pub(super) async fn reload_achievements(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let token = call.form.iter().find(|(k, _)| k == "adminAuth").map(|(_, v)| v.clone());
    let world = state.world();
    let Some(admin_token) = world.call(|w| Ok(w.params.get_str_param("ADMIN_AUTH"))).await? else {
        return Ok(entity(call, StatusCode::OK, "ERROR! no admin token set in DB".into()));
    };
    if Some(admin_token) == token {
        world
            .call(|w| {
                w.achievements = achievement::load_data(w.db())?;
                Ok(())
            })
            .await?;
        Ok(entity(call, StatusCode::OK, "SUCCESS! reloaded achievements data".into()))
    } else {
        Ok(entity(call, StatusCode::OK, "ERROR! invalid admin token".into()))
    }
}

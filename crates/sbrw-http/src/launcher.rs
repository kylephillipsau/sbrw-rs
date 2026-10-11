//! What the SBRW launcher calls before the game starts: the server list entry,
//! ModNet, and `LaunchFilter`, which runs before login and registration.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sbrw_core::Fault;
use sbrw_core::bo::{get_server_information, modding};
use sbrw_core::dao::user;
use sbrw_core::vo::LoginStatus;
use sbrw_xml::ToXml;

use crate::AppState;
use crate::engine::{Call, entity};

// port: api/GetServerInformation#getServerInformation()
/// `GET /Engine.svc/GetServerInformation`: the launcher's server list entry,
/// as JSON.
pub(crate) async fn server_information(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let info = state.world().call(|w| get_server_information::get_server_information(w)).await?;
    Ok(entity(call, StatusCode::OK, serde_json::to_string(&info).map_err(anyhow::Error::from)?))
}

// port: api/Modding#getModInfo()
/// `GET /Engine.svc/Modding/GetModInfo`. A 404 tells the launcher ModNet is
/// off; otherwise it installs its client patches from the SBRW CDN and syncs
/// this server's mods from `basePath`.
pub(crate) async fn mod_info(state: &AppState, call: &Call) -> Result<Response, Fault> {
    match state.world().call(|w| Ok(modding::get_mod_info(w))).await? {
        None => Ok(StatusCode::NOT_FOUND.into_response()),
        Some(mod_info) => {
            Ok(entity(call, StatusCode::OK, serde_json::to_string(&mod_info).map_err(anyhow::Error::from)?))
        }
    }
}

// port: api/util/LaunchFilter#filter(ContainerRequestContext)
/// Runs before `authenticateUser` and `createUser`: records the launcher's
/// hardware id on the account (clearing it when the header is absent, as
/// upstream does), then optionally turns away outdated launchers. `Some` is
/// the response that aborts the request.
pub(crate) async fn launch_filter(state: &AppState, call: &Call) -> Result<Option<Response>, Fault> {
    let hwid = call.header("X-HWID");
    let email = call.parameter("email");
    let world = state.world();
    // Two DAO calls, so two transactions, as upstream.
    let found = world.call(move |w| Ok(email.map(|e| user::find_by_email(w.db(), &e)).transpose()?.flatten())).await?;
    if let Some(mut found) = found {
        found.hwid = hwid;
        world.call(move |w| Ok(user::update(w.db(), &found)?)).await?;
    }

    let (enabled, whitelist) = world
        .call(|w| {
            Ok((
                w.params.get_bool_param("ENABLE_WHITELISTED_LAUNCHERS_ONLY"),
                w.params.get_str_param_or("WHITELISTED_LAUNCHERS_ONLY", None),
            ))
        })
        .await?;
    if !enabled {
        return Ok(None);
    }
    let Some(whitelist) = whitelist else { return Ok(None) };
    let whitelist: serde_json::Value = serde_json::from_str(&whitelist).map_err(anyhow::Error::from)?;
    let minimum = |key: &str| -> Result<String, Fault> {
        // org.json's getString: missing or not a string throws.
        whitelist
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or_else(|| anyhow::anyhow!("JSONObject[\"{key}\"] not a string.").into())
    };
    let lock = if let Some(agent) = call.header("X-UserAgent") {
        let parts = sbrw_java::lang::split(&agent, ' ');
        let version = || parts.get(1).cloned().ok_or_else(|| Fault::from(anyhow::anyhow!("Index 1 out of bounds")));
        if agent.starts_with("LegacyLauncher") {
            compare_versions(&version()?, &minimum("legacy")?)? == -1
        } else if agent.starts_with("GameLauncherReborn") {
            compare_versions(&version()?, &minimum("sbrw")?)? == -1
        } else {
            true
        }
    } else if let Some(agent) = call.header("user-agent") {
        let parts = sbrw_java::lang::split(&agent, '/');
        let version = parts.get(1).cloned().ok_or_else(|| Fault::from(anyhow::anyhow!("Index 1 out of bounds")))?;
        compare_versions(&version, &minimum("electron")?)? == -1
    } else {
        true
    };
    if !lock {
        return Ok(None);
    }
    let mut status = LoginStatus::new(0, "", false);
    status.vo.description = Some(
        "You're using the wrong launcher, please update to the latest one:\n\n    SBRW Launcher: https://git.io/Download_NFSW\n    Electron Launcher: https://launcher.sparkserver.eu/"
            .into(),
    );
    Ok(Some(entity(call, StatusCode::UNAUTHORIZED, status.vo.to_xml())))
}

// port: api/util/LaunchFilter#compareVersions(String,String)
/// Dotted versions compared numerically, -1/0/1; a shorter equal prefix is older.
fn compare_versions(v1: &str, v2: &str) -> Result<i32, Fault> {
    let (a, b) = (sbrw_java::lang::split(v1, '.'), sbrw_java::lang::split(v2, '.'));
    for (x, y) in a.iter().zip(&b) {
        let parse = |s: &str| s.parse::<i32>().map_err(|_| Fault::from(anyhow::anyhow!("For input string: \"{s}\"")));
        match parse(x)?.cmp(&parse(y)?) {
            std::cmp::Ordering::Equal => {}
            order => return Ok(order as i32),
        }
    }
    Ok(a.len().cmp(&b.len()) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_like_upstream() {
        assert_eq!(compare_versions("2.2.0", "2.1.9").unwrap(), 1);
        assert_eq!(compare_versions("2.1", "2.1.0").unwrap(), -1);
        assert_eq!(compare_versions("2.1.0", "2.1.0").unwrap(), 0);
        assert_eq!(compare_versions("10.0", "9.9").unwrap(), 1);
        assert!(compare_versions("2.x", "2.1").is_err());
    }
}

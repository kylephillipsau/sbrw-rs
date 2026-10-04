//! What the SBRW launcher calls before the game starts: the server list entry,
//! ModNet, and `LaunchFilter`, which runs before login and registration.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sbrw_core::Fault;
use sbrw_core::dao::user;
use sbrw_core::vo::login_status::LoginStatusVO;
use sbrw_xml::ToXml;
use serde::Serialize;

use crate::AppState;
use crate::engine::{Call, entity};

/// `GET /Engine.svc/GetServerInformation`: the launcher's server list entry.
/// Field names follow upstream's `ServerInformationVO`; absent optional
/// strings are omitted, as upstream's JSON-B does with nulls. The launcher's
/// model has non-nullable ints for the counters, so those are always sent.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInformation {
    #[serde(skip_serializing_if = "Option::is_none")]
    message_srv: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    home_page_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    facebook_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    discord_url: Option<String>,
    server_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    country: Option<String>,
    timezone: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    banner_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    admin_list: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    owner_list: Option<String>,
    number_of_registered: i64,
    seconds_to_shut_down: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_countries: Option<String>,
    activated_holiday_scenery_groups: Vec<String>,
    disactivated_holiday_scenery_groups: Vec<String>,
    online_number: i64,
    /// A JSON boolean even though the launcher models it as a string; it
    /// compares `ToLower() == "true"`, which Newtonsoft's coercion satisfies.
    require_ticket: bool,
    server_version: String,
    /// When true the launcher switches to `/User/modernAuth`, which upstream
    /// never implemented; stay on the SHA-1 query-string login.
    modern_auth_support: bool,
}

pub async fn server_information(state: &AppState) -> Json<ServerInformation> {
    let launcher = &state.config.launcher;
    Json(ServerInformation {
        message_srv: launcher.message.clone(),
        home_page_url: launcher.homepage_url.clone(),
        facebook_url: launcher.facebook_url.clone(),
        discord_url: launcher.discord_url.clone(),
        server_name: launcher.server_name.clone(),
        country: launcher.country.clone(),
        timezone: launcher.timezone,
        banner_url: launcher.banner_url.clone(),
        admin_list: launcher.admins.clone(),
        owner_list: launcher.owners.clone(),
        // Accounts don't exist until the world store lands.
        number_of_registered: 0,
        seconds_to_shut_down: launcher.seconds_to_shutdown,
        allowed_countries: launcher.allowed_countries.clone(),
        activated_holiday_scenery_groups: launcher.enabled_scenery.clone(),
        disactivated_holiday_scenery_groups: launcher.disabled_scenery.clone(),
        online_number: 0,
        require_ticket: false,
        server_version: concat!("sbrw-rs ", env!("CARGO_PKG_VERSION")).into(),
        modern_auth_support: false,
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModInfo {
    base_path: String,
    #[serde(rename = "serverID")]
    server_id: String,
    features: Vec<String>,
}

/// `GET /Engine.svc/Modding/GetModInfo`. A 404 tells the launcher ModNet is
/// off; otherwise it installs its client patches from the SBRW CDN and syncs
/// this server's mods from `basePath`.
pub async fn mod_info(state: &AppState) -> Response {
    let modding = &state.config.modding;
    if !modding.enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(ModInfo {
        base_path: state.config.mods_base_url(),
        server_id: modding.server_id.clone(),
        features: modding.features.clone(),
    })
    .into_response()
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
        let parts = java_split(&agent, ' ');
        let version = || parts.get(1).cloned().ok_or_else(|| Fault::from(anyhow::anyhow!("Index 1 out of bounds")));
        if agent.starts_with("LegacyLauncher") {
            compare_versions(&version()?, &minimum("legacy")?)? == -1
        } else if agent.starts_with("GameLauncherReborn") {
            compare_versions(&version()?, &minimum("sbrw")?)? == -1
        } else {
            true
        }
    } else if let Some(agent) = call.header("user-agent") {
        let parts = java_split(&agent, '/');
        let version = parts.get(1).cloned().ok_or_else(|| Fault::from(anyhow::anyhow!("Index 1 out of bounds")))?;
        compare_versions(&version, &minimum("electron")?)? == -1
    } else {
        true
    };
    if !lock {
        return Ok(None);
    }
    let mut status = LoginStatusVO::new(0, "", false);
    status.description = Some(
        "You're using the wrong launcher, please update to the latest one:\n\n    SBRW Launcher: https://git.io/Download_NFSW\n    Electron Launcher: https://launcher.sparkserver.eu/"
            .into(),
    );
    Ok(Some(entity(call, StatusCode::UNAUTHORIZED, status.to_xml())))
}

// port: api/util/LaunchFilter#compareVersions(String,String)
/// Dotted versions compared numerically, -1/0/1; a shorter equal prefix is older.
fn compare_versions(v1: &str, v2: &str) -> Result<i32, Fault> {
    let (a, b) = (java_split(v1, '.'), java_split(v2, '.'));
    for (x, y) in a.iter().zip(&b) {
        let parse = |s: &str| s.parse::<i32>().map_err(|_| Fault::from(anyhow::anyhow!("For input string: \"{s}\"")));
        match parse(x)?.cmp(&parse(y)?) {
            std::cmp::Ordering::Equal => {}
            order => return Ok(order as i32),
        }
    }
    Ok(a.len().cmp(&b.len()) as i32)
}

/// `String.split`: trailing empty strings are dropped.
fn java_split(s: &str, c: char) -> Vec<String> {
    let mut parts: Vec<String> = s.split(c).map(str::to_owned).collect();
    while parts.len() > 1 && parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        parts.clear();
    }
    parts
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

//! JSON endpoints the SBRW launcher calls before the game starts.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::AppState;

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

pub async fn server_information(State(state): State<AppState>) -> Json<ServerInformation> {
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
pub async fn mod_info(State(state): State<AppState>) -> Response {
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

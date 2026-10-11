//! What the game asks for while it starts up and on a timer afterwards: server
//! and account settings, chat and network endpoints, news, and stubs upstream
//! answers with fixed values. As upstream, each call into a BO is its own
//! world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::bo::{get_server_information, login_announcement, news_article, scenery_util, session};
use sbrw_core::{Fault, World};
use sbrw_java::time::xml_gregorian_calendar;
use sbrw_java::util::base64_encode;
use sbrw_xml::dto::http::{
    ArrayOfCarClass, ArrayOfLevelGiftDefinition, ArrayOfLong, ArrayOfString, ArrayOfUdpRelayInfo, CarClass, ChatServer,
    ClientServerCryptoTicket, FraudConfig, HeartBeat, RegionInfo, SocialNetworkInfo, SocialSettings, SystemInfo,
    UdpRelayCryptoTicket, UdpRelayInfo, UserSettings,
};
use sbrw_xml::{DateTime, ToXml};

use super::{Call, entity};
use crate::AppState;

// port: api/SystemInfo#systemInfo()
pub(super) async fn system_info(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let system_info = state
        .world()
        .call(|w| {
            let build = &w.build_info;
            Ok(SystemInfo {
                branch: Some(build.branch().into()),
                change_list: Some(build.long_commit_id().into()),
                client_version: Some("1614b".into()),
                client_version_check: true,
                deployed: Some(build.time().into()),
                entitlements_to_download: true,
                force_permanent_session: true,
                jid_prepender: Some("sbrw".into()),
                launcher_service_url: Some("http://127.0.0.1".into()),
                nucleus_namespace: Some("sbrw-live".into()),
                nucleus_namespace_web: Some("sbr_web".into()),
                persona_cache_timeout: 900,
                portal_domain: w.params.get_str_param_or("PORTAL_DOMAIN", Some("soapboxrace.world")),
                portal_store_failure_page: w
                    .params
                    .get_str_param_or("PORTAL_FAILURE_PAGE", Some("soapboxrace.world/fail")),
                portal_time_out: Some("6000".into()),
                shard_name: Some("CORE".into()),
                time: Some(DateTime(xml_gregorian_calendar(w.env.now()))),
                version: Some(build.commit_id().into()),
                ..Default::default()
            })
        })
        .await?;
    Ok(entity(call, StatusCode::OK, system_info.to_xml()))
}

// port: api/GetUserSettings#getUserSettingsGet()
pub(super) async fn get_user_settings(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let world = state.world();
    let server_information = world.call(|w| get_server_information::get_server_information(w)).await?;
    let activated = server_information.activated_holiday_scenery_groups.unwrap_or_default();
    let disactivated = server_information.disactivated_holiday_scenery_groups.unwrap_or_default();
    // Every enabled group gets an id, even one the game doesn't know (0).
    let scenery_ids = activated.iter().map(|s| scenery_util::get_scenery_id(s)).collect();

    let user = call.session().user.clone();
    let user_id = user.id;
    let max_level = world.call(move |w| w.params.get_max_level(&user)).await?;
    let user_settings = UserSettings {
        car_cache_age_limit: 600,
        is_race_now_enabled: true,
        max_car_cache_size: 250,
        min_race_now_level: 2,
        voip_available: false,
        activated_holiday_scenery_groups: Some(ArrayOfString {
            string: activated.into_iter().filter(|s| scenery_util::is_valid(s)).collect(),
        }),
        active_holiday_ids: Some(ArrayOfLong { _long: scenery_ids }),
        disactivated_holiday_scenery_groups: Some(ArrayOfString {
            string: disactivated.into_iter().filter(|s| scenery_util::is_valid(&s.replace("_DISABLE", ""))).collect(),
        }),
        first_time_login: false,
        max_level,
        starter_pack_applied: false,
        user_id,
    };
    Ok(entity(call, StatusCode::OK, user_settings.to_xml()))
}

// port: api/CarClasses#carClasses()
/// The rating range of each car class, by class hash.
pub(super) fn car_classes(call: &Call) -> Result<Response, Fault> {
    let class = |car_class_hash, max_rating, min_rating| CarClass { car_class_hash, max_rating, min_rating };
    let array = ArrayOfCarClass {
        car_class: vec![
            class(-2142411446, 999, 750),
            class(-406473455, 599, 500),
            class(-405837480, 749, 600),
            class(415909161, 399, 250),
            class(872416321, 249, 0),
            class(1866825865, 499, 400),
        ],
    };
    Ok(entity(call, StatusCode::OK, array.to_xml()))
}

// port: api/Session#getChatInfo()
/// Where the XMPP server is, and the public chat rooms.
pub(super) async fn get_chat_info(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let world = state.world();
    let (ip, port) =
        world.call(|w| Ok((w.params.get_str_param("XMPP_IP"), w.params.get_int_param("XMPP_PORT")?))).await?;
    let rooms = world.call(|w| session::get_all_chat_room(w)).await?;
    let chat_server = ChatServer { ip, port, prefix: Some("sbrw".into()), rooms: Some(rooms) };
    Ok(entity(call, StatusCode::OK, chat_server.to_xml()))
}

// port: api/GetRebroadcasters#getRebroadcasters()
/// Where the free-roam UDP server is.
pub(super) async fn get_rebroadcasters(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let (host, port) = state
        .world()
        .call(|w| Ok((w.params.get_str_param("UDP_FREEROAM_IP"), w.params.get_int_param("UDP_FREEROAM_PORT")?)))
        .await?;
    let array = ArrayOfUdpRelayInfo { udp_relay_info: vec![UdpRelayInfo { host, port }] };
    Ok(entity(call, StatusCode::OK, array.to_xml()))
}

// port: api/GetRegionInfo#getRegionInfo()
pub(super) fn get_region_info(call: &Call) -> Result<Response, Fault> {
    let region_info = RegionInfo {
        countdown_proposal_in_milliseconds: 3000,
        direct_connect_timeout_in_milliseconds: 1000,
        drop_out_time_in_milliseconds: 15000,
        event_load_timeout_in_milliseconds: 30000,
        heartbeat_interval_in_milliseconds: 1000,
        udp_relay_bandwidth_in_bps: 9600,
        udp_relay_timeout_in_milliseconds: 60000,
    };
    Ok(entity(call, StatusCode::OK, region_info.to_xml()))
}

// port: api/util/UUIDGen#getRandomUUIDBytes()
/// A fresh UUID's 16 bytes, most significant first.
fn random_uuid_bytes(w: &World) -> [u8; 16] {
    *w.env.uuid().as_bytes()
}

// port: api/Crypto#relayCryptoTicket(String,Long)
/// The race relay's ticket for this session, set when a race starts.
pub(super) async fn relay_crypto_ticket(state: &AppState, call: &Call) -> Result<Response, Fault> {
    call.path_long("personaId")?;
    let ticket_iv = state.world().call(|w| Ok(base64_encode(&random_uuid_bytes(w)))).await?;
    let ticket = UdpRelayCryptoTicket {
        crypto_ticket: call.session().relay_crypto_ticket.clone(),
        session_key: Some("AAAAAAAAAAAAAAAAAAAAAA==".into()),
        ticket_iv: Some(ticket_iv),
    };
    Ok(entity(call, StatusCode::OK, ticket.to_xml()))
}

// port: api/Crypto#cryptoticket()
/// A ticket whose hello bytes are 10, 11, 12, 13 and an all-zero key: what
/// the ModNet-patched client expects when UDP encryption is off.
pub(super) async fn cryptoticket(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let ticket_iv = state.world().call(|w| Ok(base64_encode(&random_uuid_bytes(w)))).await?;
    let mut crypto_ticket_bytes = [0u8; 32];
    crypto_ticket_bytes[..4].copy_from_slice(&[10, 11, 12, 13]);
    let ticket = ClientServerCryptoTicket {
        crypto_ticket: Some(base64_encode(&crypto_ticket_bytes)),
        session_key: Some("AAAAAAAAAAAAAAAAAAAAAA==".into()),
        ticket_iv: Some(ticket_iv),
    };
    Ok(entity(call, StatusCode::OK, ticket.to_xml()))
}

// port: api/LoginAnnouncements#loginAnnouncements(String)
pub(super) async fn login_announcements(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let language = call.query("language");
    let definition =
        state.world().call(move |w| login_announcement::get_login_announcements(w, language.as_deref())).await?;
    Ok(entity(call, StatusCode::OK, definition.to_xml()))
}

// port: api/NewsArticles#newsArticles()
/// News for the selected driver (none yet: 0) and for everyone.
pub(super) async fn news_articles(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let persona_id =
        call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))?;
    let array = state.world().call(move |w| news_article::get_news_articles(w, persona_id)).await?;
    Ok(entity(call, StatusCode::OK, array.to_xml()))
}

// port: api/GetSocialNetworkInfo#getSocialNetworkInfo()
pub(super) fn get_social_network_info(call: &Call) -> Result<Response, Fault> {
    let info = SocialNetworkInfo { facebook_name: Some("test".into()) };
    Ok(entity(call, StatusCode::OK, info.to_xml()))
}

// port: api/GetSocialSettings#getSocialSettings()
pub(super) fn get_social_settings(call: &Call) -> Result<Response, Fault> {
    let settings = SocialSettings {
        appear_offline: false,
        decline_group_invite: 0,
        decline_incomming_friend_requests: false,
        decline_private_invite: 0,
        hide_offline_friends: false,
        show_news_on_sign_in: false,
        show_only_players_in_same_chat_channel: false,
    };
    Ok(entity(call, StatusCode::OK, settings.to_xml()))
}

// port: api/Security#fraudConfig(Long)
pub(super) fn fraud_config(call: &Call) -> Result<Response, Fault> {
    let user_id = call.header_long("userId")?.ok_or_else(|| Fault::null_pointer("userId"))?;
    let config = FraudConfig {
        enabled_bit_field: 12,
        game_file_freq: 1000000,
        module_freq: 360000,
        start_up_freq: 1000000,
        user_id,
    };
    Ok(entity(call, StatusCode::OK, config.to_xml()))
}

// port: api/Security#generateWebToken()
pub(super) async fn generate_web_token(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let token = state.world().call(|w| Ok(w.env.uuid().to_string())).await?;
    Ok(entity(call, StatusCode::OK, format!("<string>{token}</string>")))
}

// port: api/HeartBeat#getPermanentSession(Long)
pub(super) fn heart_beat(call: &Call) -> Result<Response, Fault> {
    let heart_beat = HeartBeat { enabled_bit_field: 0, metagame_flags: 2 };
    Ok(entity(call, StatusCode::OK, heart_beat.to_xml()))
}

// port: api/Logging#client()
pub(super) fn logging_client(call: &Call) -> Result<Response, Fault> {
    Ok(entity(call, StatusCode::OK, "<ClientConfigTrans/>".into()))
}

// port: api/Gifts#getAndTriggerAvailableLevelGifts()
pub(super) fn get_and_trigger_available_level_gifts(call: &Call) -> Result<Response, Fault> {
    Ok(entity(call, StatusCode::OK, ArrayOfLevelGiftDefinition::default().to_xml()))
}

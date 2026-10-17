//! `/Engine.svc`: upstream's JAX-RS application, routed the way RESTEasy routes it.
//!
//! Recorded behaviour (`scenarios/engine/routing.json`) this reproduces:
//! - the resource class is chosen by path prefix first; a path under a known
//!   prefix with no matching method is not found, so `Generic`'s catch-all only
//!   sees paths outside every resource class;
//! - not found and wrong method both reach upstream's catch-everything
//!   exception mapper: 503 with error code -500;
//! - paths are case-sensitive and a trailing slash is ignored;
//! - `@Secured` methods run `AuthenticationFilter`, `@LauncherChecks` methods
//!   run `LaunchFilter`, before the method itself.
//!
//! Handlers are looked up by upstream unit id. A route whose handler isn't
//! ported yet still gets upstream's routing and filters, then answers like the
//! catch-all.

mod achievements;
mod badges;
mod boot;
mod catalog;
mod driver_persona;
mod event;
mod events;
mod matchmaking;
mod personas;
mod powerups;
mod reporting;
mod routes;
mod send_announcement;
mod social;
mod user;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use sbrw_core::bo::token_session::{self, TokenSession};
use sbrw_core::{EngineExceptionCode, Fault};
use sbrw_xml::ToXml;
use sbrw_xml::dto::engine::{EngineExceptionTrans, EngineInnerExceptionTrans};

use crate::{AppState, launcher};

/// One upstream resource method (see `routes.rs`, generated from the inventory).
#[derive(Debug)]
pub struct Route {
    pub method: &'static str,
    /// The resource class's `@Path`.
    pub class: &'static str,
    /// Class and method `@Path` joined.
    pub path: &'static str,
    pub secured: bool,
    pub launcher_checks: bool,
    pub produces: Option<&'static str>,
    pub unit: &'static str,
}

/// A request resolved to a route, with what handlers read from it.
pub(crate) struct Call {
    pub route: &'static Route,
    pub path_params: Vec<(&'static str, String)>,
    pub query: Vec<(String, String)>,
    pub form: Vec<(String, String)>,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub remote_addr: IpAddr,
    /// The validated session, for `@Secured` routes.
    pub session: Option<TokenSession>,
}

impl Call {
    /// `@QueryParam`: the first value.
    pub fn query(&self, name: &str) -> Option<String> {
        self.query.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    /// A `Long` `@QueryParam`: `None` when absent; a value `Long.valueOf`
    /// rejects fails the request as RESTEasy does (503, -500).
    pub fn query_long(&self, name: &str) -> Result<Option<i64>, Fault> {
        self.query(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    Fault::Internal(anyhow::anyhow!(
                        "Unable to extract parameter from http request: javax.ws.rs.QueryParam(\"{name}\") value is '{v}'"
                    ))
                })
            })
            .transpose()
    }

    /// An `Integer` or `int` `@QueryParam`: `None` when absent (an `int` is then
    /// 0); a value `Integer.valueOf` rejects fails the request as RESTEasy does
    /// (503, -500).
    pub fn query_int(&self, name: &str) -> Result<Option<i32>, Fault> {
        self.query(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    Fault::Internal(anyhow::anyhow!(
                        "Unable to extract parameter from http request: javax.ws.rs.QueryParam(\"{name}\") value is '{v}'"
                    ))
                })
            })
            .transpose()
    }

    /// `JAXBUtility.unMarshal(InputStream, T.class)`: the body as XML, with
    /// namespaces ignored; anything unreadable fails the request (503, -500).
    pub fn unmarshal<T: sbrw_xml::XmlType>(&self) -> Result<T, Fault> {
        sbrw_xml::from_xml(&String::from_utf8_lossy(&self.body)).map_err(|e| {
            Fault::Internal(anyhow::anyhow!(
                "Failed to unmarshal stream to {} instance: {e}",
                std::any::type_name::<T>().rsplit("::").next().unwrap_or_default()
            ))
        })
    }

    /// A `Long` `@HeaderParam`. `AuthenticationFilter` has already checked
    /// `userId` on secured routes.
    pub fn header_long(&self, name: &str) -> Result<Option<i64>, Fault> {
        self.header(name)
            .map(|v| v.parse().map_err(|_| Fault::Internal(anyhow::anyhow!("For input string: \"{v}\""))))
            .transpose()
    }

    /// A `Long` `@PathParam`; one `Long.valueOf` rejects isn't found (503, -500).
    pub fn path_long(&self, name: &str) -> Result<Option<i64>, Fault> {
        self.path_param(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    Fault::Internal(anyhow::anyhow!(
                        "Unable to extract parameter from http request: javax.ws.rs.PathParam(\"{name}\") value is '{v}'"
                    ))
                })
            })
            .transpose()
    }

    /// An `int` `@PathParam`; one `Integer.valueOf` rejects isn't found (503, -500).
    pub fn path_int(&self, name: &str) -> Result<Option<i32>, Fault> {
        self.path_param(name)
            .map(|v| {
                v.parse().map_err(|_| {
                    Fault::Internal(anyhow::anyhow!(
                        "Unable to extract parameter from http request: javax.ws.rs.PathParam(\"{name}\") value is '{v}'"
                    ))
                })
            })
            .transpose()
    }

    /// The session `AuthenticationFilter` validated. Only `@Secured` routes reach
    /// handlers that ask for it.
    pub fn session(&self) -> &TokenSession {
        self.session.as_ref().expect("a @Secured route has a session")
    }

    /// `HttpServletRequest.getParameter`: query string first, then form body.
    pub fn parameter(&self, name: &str) -> Option<String> {
        self.query(name).or_else(|| self.form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
    }

    /// `getHeaderString`: case-insensitive.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
    }

    pub fn path_param(&self, name: &str) -> Option<&str> {
        self.path_params.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    /// What the method `@Produces`, as RESTEasy labels it.
    fn content_type(&self) -> &'static str {
        content_type(self.route.produces)
    }
}

fn content_type(produces: Option<&str>) -> &'static str {
    match produces {
        Some("application/json") => "application/json",
        Some("text/html") => "text/html;charset=UTF-8",
        _ => XML,
    }
}

/// Every XML response upstream sends (RESTEasy adds the charset).
pub const XML: &str = "application/xml;charset=UTF-8";

/// An entity response with the route's content type.
pub(crate) fn entity(call: &Call, status: StatusCode, body: String) -> Response {
    (status, [(CONTENT_TYPE, call.content_type())], body).into_response()
}

enum Resolved {
    Found(&'static Route, Vec<(&'static str, String)>),
    NotFound,
    NotAllowed,
}

fn resolve(method: &str, path: &str) -> Resolved {
    let path = path.strip_suffix('/').unwrap_or(path);
    if path.is_empty() {
        return Resolved::NotFound;
    }
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    let class = routes::ROUTES
        .iter()
        .map(|r| r.class)
        .filter(|c| *c != "/" && class_matches(c, &segments))
        .max_by_key(|c| c.len())
        .unwrap_or("/");
    let mut matching: Vec<(&'static Route, Vec<(&'static str, String)>)> = routes::ROUTES
        .iter()
        .filter(|r| r.class == class)
        .filter_map(|r| template_match(r.path, &segments).map(|params| (r, params)))
        .collect();
    if matching.is_empty() {
        return Resolved::NotFound;
    }
    matching.retain(|(r, _)| r.method == method);
    // The most specific template wins: the most literal characters.
    match matching.into_iter().max_by_key(|(r, _)| literal_len(r.path)) {
        Some((route, params)) => Resolved::Found(route, params),
        None => Resolved::NotAllowed,
    }
}

fn class_matches(class: &str, segments: &[&str]) -> bool {
    let class: Vec<&str> = class.trim_start_matches('/').split('/').collect();
    class.len() <= segments.len() && class.iter().zip(segments).all(|(c, s)| c == s)
}

fn template_match(template: &'static str, segments: &[&str]) -> Option<Vec<(&'static str, String)>> {
    let parts: Vec<&'static str> = template.trim_start_matches('/').split('/').collect();
    let mut params = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if let Some(name) = part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            if let Some(name) = name.strip_suffix(":.*") {
                // A regex parameter that takes the rest of the path.
                params.push((name, segments.get(i..).unwrap_or_default().join("/")));
                return (i <= segments.len()).then_some(params);
            }
            let value = segments.get(i).filter(|s| !s.is_empty())?;
            params.push((name, percent_decode(value)));
        } else if segments.get(i) != Some(part) {
            return None;
        }
    }
    (parts.len() == segments.len()).then_some(params)
}

fn literal_len(template: &str) -> usize {
    template.split('/').filter(|p| !p.starts_with('{')).map(str::len).sum()
}

fn percent_decode(s: &str) -> String {
    form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// The single entry point for everything under `/Engine.svc`. Asynchronous
/// work the request queued runs once it has its answer.
pub async fn dispatch(State(state): State<AppState>, request: Request) -> Response {
    let response = handle(&state, request).await;
    state.world().run_async_jobs().await;
    response
}

async fn handle(state: &AppState, request: Request) -> Response {
    let (parts, body) = request.into_parts();
    let path = parts.uri.path().strip_prefix("/Engine.svc").unwrap_or_default().to_owned();
    let (route, path_params) = match resolve(parts.method.as_str(), &path) {
        Resolved::Found(route, params) => (route, params),
        Resolved::NotFound => {
            return fault_response(Fault::Internal(anyhow::anyhow!(
                "Could not find resource for full path: {}",
                parts.uri
            )));
        }
        Resolved::NotAllowed => {
            return fault_response(Fault::Internal(anyhow::anyhow!("No resource method found for {}", parts.method)));
        }
    };
    let body = body.collect().await.map(|b| b.to_bytes()).unwrap_or_default();
    let is_form =
        parts.headers.get(CONTENT_TYPE).is_some_and(|v| v.as_bytes().starts_with(b"application/x-www-form-urlencoded"));
    let mut call = Call {
        route,
        path_params,
        query: form_urlencoded::parse(parts.uri.query().unwrap_or_default().as_bytes()).into_owned().collect(),
        form: if is_form { form_urlencoded::parse(&body).into_owned().collect() } else { Vec::new() },
        remote_addr: parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map_or(IpAddr::V4(Ipv4Addr::LOCALHOST), |ConnectInfo(addr)| addr.ip()),
        headers: parts.headers,
        body,
        session: None,
    };

    if route.secured {
        match authenticate(state, &call).await {
            Ok(session) => call.session = Some(session),
            Err(fault) => return fault_response(fault),
        }
    }
    if route.launcher_checks
        && let Some(abort) = launcher::launch_filter(state, &call).await.unwrap_or_else(|f| Some(fault_response(f)))
    {
        return abort;
    }
    match invoke(state, &call).await {
        Some(Ok(response)) => response,
        Some(Err(fault)) => fault_response(fault),
        None => {
            tracing::info!(target: "sbrw::unimplemented", method = route.method, path = route.path, unit = route.unit, "not ported yet; answered 200 with an empty body");
            entity(&call, StatusCode::OK, String::new())
        }
    }
}

/// The ported resource methods, by upstream unit id.
async fn invoke(state: &AppState, call: &Call) -> Option<Result<Response, Fault>> {
    Some(match call.route.unit {
        "api/Generic#genericEmptyGet(String)"
        | "api/Generic#genericEmptyPost(String)"
        | "api/Generic#genericEmptyPut(String)" => {
            // port: api/Generic#genericEmptyGet(String)
            // port: api/Generic#genericEmptyPost(String)
            // port: api/Generic#genericEmptyPut(String)
            Ok(entity(call, StatusCode::OK, String::new()))
        }
        "api/GetServerInformation#getServerInformation()" => launcher::server_information(state, call).await,
        "api/Modding#getModInfo()" => launcher::mod_info(state, call).await,
        "api/SystemInfo#systemInfo()" => boot::system_info(state, call).await,
        "api/GetUserSettings#getUserSettingsGet()" => boot::get_user_settings(state, call).await,
        "api/CarClasses#carClasses()" => boot::car_classes(call),
        "api/Session#getChatInfo()" => boot::get_chat_info(state, call).await,
        "api/GetRebroadcasters#getRebroadcasters()" => boot::get_rebroadcasters(state, call).await,
        "api/GetRegionInfo#getRegionInfo()" => boot::get_region_info(call),
        "api/Crypto#relayCryptoTicket(String,Long)" => boot::relay_crypto_ticket(state, call).await,
        "api/Crypto#cryptoticket()" => boot::cryptoticket(state, call).await,
        "api/LoginAnnouncements#loginAnnouncements(String)" => boot::login_announcements(state, call).await,
        "api/NewsArticles#newsArticles()" => boot::news_articles(state, call).await,
        "api/GetSocialNetworkInfo#getSocialNetworkInfo()" => boot::get_social_network_info(call),
        "api/GetSocialSettings#getSocialSettings()" => boot::get_social_settings(call),
        "api/Security#fraudConfig(Long)" => boot::fraud_config(call),
        "api/Security#generateWebToken()" => boot::generate_web_token(state, call).await,
        "api/HeartBeat#getPermanentSession(Long)" => boot::heart_beat(call),
        "api/Logging#client()" => boot::logging_client(call),
        "api/Gifts#getAndTriggerAvailableLevelGifts()" => boot::get_and_trigger_available_level_gifts(call),
        "api/Reporting#sendHardwareInfo(InputStream)" => reporting::send_hardware_info(state, call).await,
        "api/Reporting#sendUserSettings()"
        | "api/Reporting#sendMultiplayerConnect()"
        | "api/Reporting#sendClientPingTime()"
        | "api/Reporting#loginAnnouncementClicked()"
        | "api/Reporting#genericEmptyPut(String)" => reporting::ignored(call),
        "api/DriverPersona#getExpLevelPointsMap()" => driver_persona::get_exp_level_points_map(state, call).await,
        "api/DriverPersona#getPersonaInfo(Long)" => driver_persona::get_persona_info(state, call).await,
        "api/DriverPersona#reserveName(String)" => driver_persona::reserve_name(state, call).await,
        "api/DriverPersona#UnreserveName(String)" => driver_persona::unreserve_name(call),
        "api/DriverPersona#createPersona(Long,String,String,int,String,String)" => {
            driver_persona::create_persona(state, call).await
        }
        "api/DriverPersona#deletePersona(Long)" => driver_persona::delete_persona(state, call).await,
        "api/DriverPersona#getPersonaBaseFromList(InputStream)" => {
            driver_persona::get_persona_base_from_list(state, call).await
        }
        "api/DriverPersona#updatePersonaPresence(Long,Long)" => {
            driver_persona::update_persona_presence(state, call).await
        }
        "api/DriverPersona#getPersonaPresenceByName(String)" => {
            driver_persona::get_persona_presence_by_name(state, call).await
        }
        "api/DriverPersona#updateStatusMessage(InputStream)" => {
            driver_persona::update_status_message(state, call).await
        }
        "api/Achievements#loadall()" => achievements::loadall(state, call).await,
        "api/Achievements#redeemreward(Long)" => achievements::redeemreward(state, call).await,
        "api/ReloadAchievements#reloadAchievements(String)" => achievements::reload_achievements(state, call).await,
        "api/Events#availableAtLevel()" => events::available_at_level(state, call).await,
        "api/Events#instancedAccolades(Long)" => events::instanced_accolades(call),
        "api/Events#getTreasureHuntEventSession()" => events::get_treasure_hunt_event_session(state, call).await,
        "api/Events#notifyCoinCollected(Integer)" => events::notify_coin_collected(state, call).await,
        "api/Events#accolades()" => events::accolades(state, call).await,
        "api/Event#abort(Long)" => event::abort(state, call).await,
        "api/Event#launched(Long)" => event::launched(state, call).await,
        "api/Event#arbitration(InputStream,Long)" => event::arbitration(state, call).await,
        "api/Event#bust(InputStream,String,Long)" => event::bust(state, call).await,
        "api/MatchMaking#launchEvent(int)" => matchmaking::launch_event(state, call).await,
        "api/Badges#set(InputStream,String)" => badges::set(state, call).await,
        "api/Social#petition(String,Long,Long,Integer,String,Integer,Integer)" => social::petition(state, call).await,
        "api/SendAnnouncement#sendAnnouncement(String,String)" => {
            send_announcement::send_announcement(state, call).await
        }
        "api/AddFriendRequest#addFriendRequest(String,String)" => social::add_friend_request(state, call).await,
        "api/ResolveFriendsRequest#resolveFriendsRequest(Long,int)" => {
            social::resolve_friends_request(state, call).await
        }
        "api/RemoveFriend#removefriend(Long)" => social::removefriend(state, call).await,
        "api/GetFriendListFromUserId#getFriendsList(Long)" => social::get_friend_list_from_user_id(state, call).await,
        "api/BlockPlayer#blockPlayer(String,Long)" => social::block_player(state, call).await,
        "api/UnblockPlayer#unblockPlayer(Long,String,Long)" => social::unblock_player(state, call).await,
        "api/GetBlockedUserList#getBlockedUserList(Long)" => social::get_blocked_user_list(state, call).await,
        "api/GetBlockersByUsers#getBlockedUserList(Long,String)" => social::get_blockers_by_users(state, call).await,
        "api/MatchMaking#joinQueueRaceNow()" => matchmaking::join_queue_race_now(state, call).await,
        "api/MatchMaking#joinQueueEvent(int)" => matchmaking::join_queue_event(state, call).await,
        "api/MatchMaking#leaveQueue()" => matchmaking::leave_queue(state, call).await,
        "api/MatchMaking#leavelobby()" => matchmaking::leavelobby(state, call).await,
        "api/MatchMaking#makePrivateLobby(int)" => matchmaking::make_private_lobby(state, call).await,
        "api/MatchMaking#acceptInvite(Long)" => matchmaking::accept_invite(state, call).await,
        "api/MatchMaking#declineInvite(Long)" => matchmaking::decline_invite(state, call).await,
        "api/Catalog#productsInCategory(String,String,String)" => catalog::products_in_category(state, call).await,
        "api/Catalog#categories(String)" => catalog::categories(state, call).await,
        "api/Personas#commerce(InputStream,Long)" => personas::commerce(state, call).await,
        "api/Powerups#activated(Integer,Long,String,Long)" => powerups::activated(state, call).await,
        "api/Personas#baskets(InputStream,Long)" => personas::baskets(state, call).await,
        "api/Personas#carslots(Long)" => personas::carslots(state, call).await,
        "api/Personas#inventoryObjects(String)" => personas::inventory_objects(state, call).await,
        "api/Personas#sellInventoryItem(String,String)" => personas::sell_inventory_item(state, call).await,
        "api/Personas#carsPost(Long,Long)" => personas::cars_post(state, call).await,
        "api/Personas#carsGet(Long)" => personas::cars_get(state, call).await,
        "api/Personas#carsGet(Long,Long)" => personas::car_get(state, call).await,
        "api/Personas#carsPut(Long,InputStream)" => personas::cars_put(state, call).await,
        "api/Personas#defaultcarGet(Long)" => personas::defaultcar_get(state, call).await,
        "api/Personas#defaultcar(Long,Long)" => personas::defaultcar_put(state, call).await,
        "api/User#authenticateUser(String,String)" => user::authenticate_user(state, call).await,
        "api/User#createUser(String,String,String)" => user::create_user(state, call).await,
        "api/User#getPermanentSession()" => user::get_permanent_session(state, call).await,
        "api/User#secureLoginPersona(Long)" => user::secure_login_persona(state, call).await,
        "api/User#secureLogoutPersona(Long)" => user::secure_logout_persona(state, call).await,
        "api/User#secureLogout()" => user::secure_logout(state, call).await,
        _ => return None,
    })
}

// port: api/util/AuthenticationFilter#filter(ContainerRequestContext)
/// `@Secured`: the `userId` and `securityToken` headers must name a live
/// session. Every failure is a 401, except a `userId` that isn't a number:
/// upstream parses it outside the filter's try, so that one is a 503.
async fn authenticate(state: &AppState, call: &Call) -> Result<TokenSession, Fault> {
    let missing = || Fault::NotAuthorized("Authorization header must be provided".into());
    let (Some(user_id), Some(token)) = (call.header("userId"), call.header("securityToken")) else {
        return Err(missing());
    };
    if user_id.is_empty() || token.is_empty() {
        return Err(missing());
    }
    let user_id: i64 = user_id.parse().map_err(|_| anyhow::anyhow!("For input string: \"{user_id}\""))?;
    state
        .world()
        .call(move |w| token_session::validate_token(w, user_id, &token))
        .await
        .map_err(|_| Fault::NotAuthorized("Invalid Token".into()))
}

// port: engine/EngineExceptionHandler#toResponse(Exception)
/// Upstream's mapper for every exception: 401 for `NotAuthorizedException`,
/// otherwise 503 with an `EngineExceptionTrans`.
pub(crate) fn fault_response(fault: Fault) -> Response {
    let (code, trace) = match fault {
        Fault::NotAuthorized(_) => return StatusCode::UNAUTHORIZED.into_response(),
        Fault::Engine(e) => (e.code.error_code(), format!("EngineException {:?}", e.code)),
        Fault::Internal(err) => {
            tracing::warn!("{err:#}");
            (EngineExceptionCode::UnspecifiedError.error_code(), format!("{err:#}"))
        }
    };
    let body = EngineExceptionTrans {
        error_code: code,
        stack_trace: Some(trace.clone()),
        inner_exception: Some(EngineInnerExceptionTrans {
            error_code: code,
            stack_trace: Some(trace),
            ..Default::default()
        }),
        ..Default::default()
    }
    .to_xml();
    (StatusCode::SERVICE_UNAVAILABLE, [(CONTENT_TYPE, XML)], Body::from(body)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(method: &str, path: &str) -> Option<&'static str> {
        match resolve(method, path) {
            Resolved::Found(route, _) => Some(route.unit),
            _ => None,
        }
    }

    #[test]
    fn routes_like_resteasy() {
        assert_eq!(unit("GET", "/User/authenticateUser"), Some("api/User#authenticateUser(String,String)"));
        assert_eq!(unit("GET", "/User/authenticateUser/"), Some("api/User#authenticateUser(String,String)"));
        assert!(matches!(resolve("GET", "/User/Foo"), Resolved::NotFound));
        assert!(matches!(resolve("POST", "/User/authenticateUser"), Resolved::NotAllowed));
        assert_eq!(unit("GET", "/anything/deeper"), Some("api/Generic#genericEmptyGet(String)"));
        assert_eq!(unit("GET", "/user/authenticateUser"), Some("api/Generic#genericEmptyGet(String)"));
        assert!(matches!(resolve("DELETE", "/anything"), Resolved::NotAllowed));
        assert!(matches!(resolve("GET", "/"), Resolved::NotFound));
        assert!(matches!(resolve("GET", ""), Resolved::NotFound));
        assert_eq!(unit("PUT", "/Reporting/Anything/At/All"), Some("api/Reporting#genericEmptyPut(String)"));
        match resolve("GET", "/personas/100/cars/7") {
            Resolved::Found(route, params) => {
                assert_eq!(route.unit, "api/Personas#carsGet(Long,Long)");
                assert_eq!(params, [("personaId", "100".to_owned()), ("carId", "7".to_owned())]);
            }
            _ => panic!("not found"),
        }
        // A literal segment beats a template parameter.
        assert_eq!(unit("GET", "/personas/inventory/objects"), Some("api/Personas#inventoryObjects(String)"));
    }
}

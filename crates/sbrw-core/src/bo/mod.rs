//! Upstream's business objects (`com.soapboxrace.core.bo`).
//!
//! Each upstream `@Stateless`/`@Singleton` bean becomes a module of functions
//! taking the [`World`](crate::World); singleton state lives in a field of the
//! world. A call from a resource into a BO is one world call, so one
//! transaction, as under upstream's container.

pub mod achievement;
pub mod achievement_transaction;
pub mod authentication;
pub mod basket;
pub mod car_damage;
pub mod car_slot;
pub mod commerce;
pub mod dnf_timer;
pub mod driver_persona;
pub mod drop;
pub mod event;
pub mod event_result;
pub mod event_result_drag;
pub mod event_result_pursuit;
pub mod event_result_route;
pub mod event_result_team_escape;
pub mod events;
pub mod get_server_information;
pub mod hardware_info;
pub mod inventory;
pub mod item_reward;
pub mod legit_race;
pub mod lobby;
pub mod lobby_countdown;
pub mod lobby_messaging;
pub mod login_announcement;
pub mod matchmaking;
pub mod modding;
pub mod news_article;
pub mod online_users;
pub mod performance;
pub mod persona;
pub mod powerup_tracking;
pub mod presence;
pub mod product;
pub mod reward;
pub mod reward_drag;
pub mod reward_pursuit;
pub mod reward_route;
pub mod reward_team_escape;
pub mod scenery_util;
pub mod scripting;
pub mod session;
pub mod social;
pub mod social_relationship;
pub mod token_session;
pub mod user;
pub mod util;

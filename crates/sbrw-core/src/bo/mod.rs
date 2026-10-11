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
pub mod driver_persona;
pub mod get_server_information;
pub mod hardware_info;
pub mod inventory;
pub mod login_announcement;
pub mod matchmaking;
pub mod modding;
pub mod news_article;
pub mod online_users;
pub mod performance;
pub mod persona;
pub mod presence;
pub mod scenery_util;
pub mod scripting;
pub mod session;
pub mod token_session;
pub mod user;
pub mod util;

//! Upstream's business objects (`com.soapboxrace.core.bo`).
//!
//! Each upstream `@Stateless`/`@Singleton` bean becomes a module of functions
//! taking the [`World`](crate::World); singleton state lives in a field of the
//! world. A call from a resource into a BO is one world call, so one
//! transaction, as under upstream's container.

pub mod achievement;
pub mod achievement_transaction;
pub mod authentication;
pub mod driver_persona;
pub mod hardware_info;
pub mod inventory;
pub mod matchmaking;
pub mod online_users;
pub mod presence;
pub mod scripting;
pub mod token_session;
pub mod user;
pub mod util;

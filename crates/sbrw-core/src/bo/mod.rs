//! Upstream's business objects (`com.soapboxrace.core.bo`).
//!
//! Each upstream `@Stateless`/`@Singleton` bean becomes a module of functions
//! taking the [`World`](crate::World); singleton state lives in a field of the
//! world. A call from a resource into a BO is one world call, so one
//! transaction, as under upstream's container.

pub mod authentication;
pub mod hardware_info;
pub mod online_users;
pub mod token_session;
pub mod user;

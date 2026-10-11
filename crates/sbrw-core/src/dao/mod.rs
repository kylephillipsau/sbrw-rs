//! Upstream's JPA entities and DAOs.
//!
//! Each module holds an entity (the columns Hibernate maps, nothing more) and
//! the DAO methods and named queries that read or write it. The JPA rules the
//! port keeps:
//! - `insert` (`persist`) writes every mapped column; unmapped columns get their
//!   database defaults;
//! - `update` (`merge`) writes every mapped column from the entity as it is,
//!   so a stale copy overwrites newer values, exactly as upstream does;
//! - an EAGER relationship is loaded with its owner.

pub mod achievement;
pub mod badge_definition;
pub mod ban;
pub mod hardware_info;
pub mod inventory;
pub mod inventory_item;
pub mod invite_ticket;
pub mod level_rep;
pub mod online_users;
pub mod persona;
pub mod persona_achievement;
pub mod persona_achievement_rank;
pub mod persona_badge;
pub mod product;
pub mod treasure_hunt;
pub mod user;

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
pub mod achievement_rank;
pub mod achievement_reward;
pub mod amplifier;
pub mod badge_definition;
pub mod ban;
pub mod basket_definition;
pub mod car;
pub mod car_classes;
pub mod card_pack;
pub mod category;
pub mod chat_room;
pub mod event;
pub mod event_data;
pub mod event_reward;
pub mod event_session;
pub mod hardware_info;
pub mod inventory;
pub mod inventory_item;
pub mod invite_ticket;
pub mod level_rep;
pub mod lobby;
pub mod login_announcement;
pub mod news_article;
pub mod online_users;
pub mod persona;
pub mod persona_achievement;
pub mod persona_achievement_rank;
pub mod persona_badge;
pub mod product;
pub mod report;
pub mod reward_table;
pub mod treasure_hunt;
pub mod used_powerup;
pub mod user;
pub mod vinyl_product;

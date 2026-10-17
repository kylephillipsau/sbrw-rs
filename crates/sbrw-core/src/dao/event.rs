//! `EventEntity` (a race the game offers) and `EventDAO`. Static data.

use rusqlite::{Connection, OptionalExtension, Row};

use crate::Fault;
use crate::engine::EngineExceptionCode;

/// `EventEntity`. Its three LAZY reward configurations are kept as the ids
/// they join on; `event_reward::find` loads one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventEntity {
    pub id: i32,
    pub event_mode_id: i32,
    pub min_level: i32,
    pub max_level: i32,
    pub car_class_hash: i32,
    pub min_car_class_rating: i32,
    pub max_car_class_rating: i32,
    pub max_players: i32,
    pub is_enabled: bool,
    pub is_locked: bool,
    pub is_rotation_enabled: bool,
    pub name: Option<String>,
    pub rewards_time_limit: i64,
    pub legit_time: i64,
    pub track_length: f32,
    pub lobby_countdown_time: i32,
    pub dnf_timer_time: i32,
    pub is_race_again_enabled: bool,
    pub is_dnf_enabled: bool,
    pub singleplayer_reward_config_id: String,
    pub multiplayer_reward_config_id: String,
    pub private_reward_config_id: String,
}

impl EventEntity {
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("ID")?,
            event_mode_id: row.get("eventModeId")?,
            min_level: row.get("minLevel")?,
            max_level: row.get("maxLevel")?,
            car_class_hash: row.get("carClassHash")?,
            min_car_class_rating: row.get("minCarClassRating")?,
            max_car_class_rating: row.get("maxCarClassRating")?,
            max_players: row.get("maxPlayers")?,
            is_enabled: row.get("isEnabled")?,
            is_locked: row.get("isLocked")?,
            is_rotation_enabled: row.get("isRotationEnabled")?,
            name: row.get("name")?,
            rewards_time_limit: row.get("rewardsTimeLimit")?,
            legit_time: row.get("legitTime")?,
            // A MySQL FLOAT: the nearest float to what the data says.
            track_length: row.get::<_, f64>("trackLength")? as f32,
            lobby_countdown_time: row.get("lobbyCountdownTime")?,
            dnf_timer_time: row.get("dnfTimerTime")?,
            is_race_again_enabled: row.get("isRaceAgainEnabled")?,
            is_dnf_enabled: row.get("isDnfEnabled")?,
            singleplayer_reward_config_id: row.get("singleplayer_reward_config_id")?,
            multiplayer_reward_config_id: row.get("multiplayer_reward_config_id")?,
            private_reward_config_id: row.get("private_reward_config_id")?,
        })
    }
}

/// `EventMode`: the kinds of event, by their `eventModeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventMode {
    Sprint,
    Circuit,
    Drag,
    PursuitSp,
    PursuitMp,
    Meetingplace,
}

impl EventMode {
    const VALUES: [EventMode; 6] = [
        EventMode::Sprint,
        EventMode::Circuit,
        EventMode::Drag,
        EventMode::PursuitSp,
        EventMode::PursuitMp,
        EventMode::Meetingplace,
    ];

    pub fn event_mode_id(self) -> i32 {
        match self {
            EventMode::Sprint => 9,
            EventMode::Circuit => 4,
            EventMode::Drag => 19,
            EventMode::PursuitSp => 12,
            EventMode::PursuitMp => 24,
            EventMode::Meetingplace => 22,
        }
    }

    /// `name()`.
    pub fn name(self) -> &'static str {
        match self {
            EventMode::Sprint => "SPRINT",
            EventMode::Circuit => "CIRCUIT",
            EventMode::Drag => "DRAG",
            EventMode::PursuitSp => "PURSUIT_SP",
            EventMode::PursuitMp => "PURSUIT_MP",
            EventMode::Meetingplace => "MEETINGPLACE",
        }
    }

    // port: jpa/EventMode#fromId(int)
    pub fn from_id(id: i32) -> Result<EventMode, Fault> {
        Self::VALUES
            .into_iter()
            .find(|mode| mode.event_mode_id() == id)
            .ok_or_else(|| Fault::engine(EngineExceptionCode::UnspecifiedError, true))
    }
}

// port: dao/EventDAO#find(Integer)
pub fn find(db: &Connection, id: i32) -> rusqlite::Result<Option<EventEntity>> {
    db.prepare_cached("SELECT * FROM event WHERE ID = ?1")?.query_row([id], EventEntity::from_row).optional()
}

// port: dao/EventDAO#findByLevel(int)
// port: jpa/EventEntity@EventEntity.findByLevel
/// The enabled events for a level, in id order: MySQL scans the table
/// rather than use the availability index.
pub fn find_by_level(db: &Connection, level: i32) -> rusqlite::Result<Vec<EventEntity>> {
    db.prepare_cached("SELECT * FROM event WHERE ?1 >= minLevel AND isEnabled = 1 AND ?1 <= maxLevel ORDER BY ID")?
        .query_map([level], EventEntity::from_row)?
        .collect()
}

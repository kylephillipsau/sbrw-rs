//! `AchievementEventContext`: what EVENT achievement scripts see about the
//! event a driver finished.

use crate::bo::util::arbitration::Packet;
use crate::dao::event::EventMode;
use crate::dao::event_session::EventSessionEntity;

/// Scripts read it through its getters (`bo::scripting`). The packet is a
/// copy taken when the context is made, after the BOs' last change to it.
#[derive(Debug, Clone, PartialEq)]
pub struct AchievementEventContext {
    event_mode: EventMode,
    event_mode_int: i32,
    arbitration_packet: Packet,
    event_session_entity: EventSessionEntity,
}

impl AchievementEventContext {
    // port: bo/util/AchievementEventContext#<init>(EventMode,ArbitrationPacket,EventSessionEntity)
    pub fn new(event_mode: EventMode, arbitration_packet: Packet, event_session_entity: EventSessionEntity) -> Self {
        Self { event_mode, event_mode_int: event_mode.event_mode_id(), arbitration_packet, event_session_entity }
    }

    // port: bo/util/AchievementEventContext#getEventMode()
    pub fn event_mode(&self) -> EventMode {
        self.event_mode
    }

    // port: bo/util/AchievementEventContext#getEventModeInt()
    pub fn event_mode_int(&self) -> i32 {
        self.event_mode_int
    }

    // port: bo/util/AchievementEventContext#getArbitrationPacket()
    pub fn arbitration_packet(&self) -> &Packet {
        &self.arbitration_packet
    }

    // port: bo/util/AchievementEventContext#getEventSessionEntity()
    pub fn event_session_entity(&self) -> &EventSessionEntity {
        &self.event_session_entity
    }
}

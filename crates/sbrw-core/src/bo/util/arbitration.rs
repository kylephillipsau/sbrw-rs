//! `ArbitrationPacket`, the base class of what the game reports when it
//! finishes an event. The generated DTOs repeat the base's fields rather than
//! share a type; this trait gives the BOs the base's view of each.

use sbrw_xml::dto::http::{
    ArbitrationPacket, DragArbitrationPacket, PursuitArbitrationPacket, RouteArbitrationPacket,
    TeamEscapeArbitrationPacket,
};

/// The base class's getters and setters.
pub trait Arbitration {
    fn alternate_event_duration_in_milliseconds(&self) -> i64;
    fn car_id(&self) -> i64;
    fn event_duration_in_milliseconds(&self) -> i64;
    fn finish_reason(&self) -> i32;
    fn hacks_detected(&self) -> i64;
    fn set_hacks_detected(&mut self, value: i64);
    fn rank(&self) -> i32;
    fn set_rank(&mut self, value: i32);
    /// The packet as its class, for `instanceof`.
    fn packet(&self) -> PacketRef<'_>;
}

/// A packet by its class.
#[derive(Debug, Clone, Copy)]
pub enum PacketRef<'a> {
    Base(&'a ArbitrationPacket),
    Route(&'a RouteArbitrationPacket),
    Drag(&'a DragArbitrationPacket),
    Pursuit(&'a PursuitArbitrationPacket),
    TeamEscape(&'a TeamEscapeArbitrationPacket),
}

/// A copy of a packet, by its class.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    Base(ArbitrationPacket),
    Route(RouteArbitrationPacket),
    Drag(DragArbitrationPacket),
    Pursuit(PursuitArbitrationPacket),
    TeamEscape(TeamEscapeArbitrationPacket),
}

impl PacketRef<'_> {
    pub fn to_owned(self) -> Packet {
        match self {
            PacketRef::Base(p) => Packet::Base(p.clone()),
            PacketRef::Route(p) => Packet::Route(p.clone()),
            PacketRef::Drag(p) => Packet::Drag(p.clone()),
            PacketRef::Pursuit(p) => Packet::Pursuit(p.clone()),
            PacketRef::TeamEscape(p) => Packet::TeamEscape(p.clone()),
        }
    }
}

macro_rules! arbitration {
    ($($ty:ident => $variant:ident),* $(,)?) => {$(
        impl Arbitration for $ty {
            fn alternate_event_duration_in_milliseconds(&self) -> i64 {
                self.alternate_event_duration_in_milliseconds
            }
            fn car_id(&self) -> i64 {
                self.car_id
            }
            fn event_duration_in_milliseconds(&self) -> i64 {
                self.event_duration_in_milliseconds
            }
            fn finish_reason(&self) -> i32 {
                self.finish_reason
            }
            fn hacks_detected(&self) -> i64 {
                self.hacks_detected
            }
            fn set_hacks_detected(&mut self, value: i64) {
                self.hacks_detected = value;
            }
            fn rank(&self) -> i32 {
                self.rank
            }
            fn set_rank(&mut self, value: i32) {
                self.rank = value;
            }
            fn packet(&self) -> PacketRef<'_> {
                PacketRef::$variant(self)
            }
        }
    )*};
}

arbitration! {
    ArbitrationPacket => Base,
    RouteArbitrationPacket => Route,
    DragArbitrationPacket => Drag,
    PursuitArbitrationPacket => Pursuit,
    TeamEscapeArbitrationPacket => TeamEscape,
}

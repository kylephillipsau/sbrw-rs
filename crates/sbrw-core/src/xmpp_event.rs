//! `XmppEvent`: what an event's other racers are told over XMPP.

use sbrw_xml::dto::xmpp::{
    XMPP_EventTimingOutType, XMPP_ResponseTypeDragEntrantResult, XMPP_ResponseTypeEventTimingOut,
    XMPP_ResponseTypeRouteEntrantResult, XMPP_ResponseTypeTeamEscapeEntrantResult,
};

use crate::World;
use crate::dao::event_session::EventSessionEntity;

/// Messages for one persona.
pub struct XmppEvent {
    persona_id: i64,
}

impl XmppEvent {
    // port: xmpp/XmppEvent#<init>(long,OpenFireSoapBoxCli)
    pub fn new(persona_id: i64) -> Self {
        Self { persona_id }
    }

    // port: xmpp/XmppEvent#sendRaceEnd(XMPP_ResponseTypeRouteEntrantResult)
    pub fn send_race_end(&self, w: &World, route_entrant_result_response: &XMPP_ResponseTypeRouteEntrantResult) {
        w.openfire.send(route_entrant_result_response, self.persona_id, &w.effects);
    }

    // port: xmpp/XmppEvent#sendTeamEscapeEnd(XMPP_ResponseTypeTeamEscapeEntrantResult)
    pub fn send_team_escape_end(
        &self,
        w: &World,
        team_escape_entrant_result_response: &XMPP_ResponseTypeTeamEscapeEntrantResult,
    ) {
        w.openfire.send(team_escape_entrant_result_response, self.persona_id, &w.effects);
    }

    // port: xmpp/XmppEvent#sendDragEnd(XMPP_ResponseTypeDragEntrantResult)
    pub fn send_drag_end(&self, w: &World, drag_entrant_result_response: &XMPP_ResponseTypeDragEntrantResult) {
        w.openfire.send(drag_entrant_result_response, self.persona_id, &w.effects);
    }

    // port: xmpp/XmppEvent#sendEventTimingOut(EventSessionEntity)
    /// The DNF countdown has started: how long the racer has left.
    pub fn send_event_timing_out(&self, w: &World, event_session_entity: &EventSessionEntity) {
        let event_timing_out_response = XMPP_ResponseTypeEventTimingOut {
            event_timing_out: Some(XMPP_EventTimingOutType {
                event_session_id: Some(event_session_entity.id),
                time_in_milliseconds: Some(event_session_entity.event.dnf_timer_time),
            }),
            ..Default::default()
        };
        w.openfire.send(&event_timing_out_response, self.persona_id, &w.effects);
    }
}

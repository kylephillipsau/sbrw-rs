//! `DNFTimerBO`: when the winner finishes, the other racers have a while to
//! finish before they're told their time ran out.

use sbrw_xml::dto::xmpp::{XMPP_EventTimedOutType, XMPP_ResponseTypeEventTimedOut};
use serde_json::json;

use crate::dao::event_session::EventSessionEntity;
use crate::timers::TimerKind;
use crate::{Fault, World, datetime};

// port: bo/DNFTimerBO#scheduleDNF(EventSessionEntity,Long)
/// A timer for the event's `dnfTimerTime`.
pub fn schedule_dnf(w: &mut World, event_session_entity: &EventSessionEntity, persona_id: i64) {
    let delay = i64::from(event_session_entity.event.dnf_timer_time);
    w.effects.timer("DNFTimerBO", delay, json!({ "eventSessionId": event_session_entity.id, "personaId": persona_id }));
    let due = datetime::current_time_millis(&w.env) + delay;
    w.timers.schedule(due, TimerKind::Dnf { event_session_id: event_session_entity.id, persona_id });
}

// port: bo/DNFTimerBO#onTimeout(Timer)
/// The countdown ran out: tell the racer their event has timed out.
pub fn on_timeout(w: &mut World, event_session_id: i64, persona_id: i64) -> Result<(), Fault> {
    let event_timed_out_response = XMPP_ResponseTypeEventTimedOut {
        event_timed_out: Some(XMPP_EventTimedOutType { event_session_id: Some(event_session_id) }),
        ..Default::default()
    };
    w.openfire.send(&event_timed_out_response, persona_id, &w.effects);
    Ok(())
}

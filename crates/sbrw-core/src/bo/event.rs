//! `EventBO`: the events a driver can enter, and the sessions and per-driver
//! data of the ones they launch.

use anyhow::anyhow;

use crate::bo::persona;
use crate::bo::token_session::TokenSession;
use crate::dao::event::{self, EventEntity};
use crate::dao::event_data::{self, EventDataEntity};
use crate::dao::event_session::{self, EventSessionEntity};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World, datetime};

/// The class of the events any car may enter.
pub const OPEN_CLASS: i32 = 607_077_938;

// port: bo/EventBO#availableAtLevel(Long)
pub fn available_at_level(w: &World, persona_id: i64) -> Result<Vec<EventEntity>, Fault> {
    let persona_entity =
        crate::dao::persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
    Ok(event::find_by_level(w.db(), persona_entity.level)?)
}

// port: bo/EventBO#createEventDataSession(Long,Long)
/// The driver's data for a session they start, with their car's class and
/// rating as it starts.
pub fn create_event_data_session(w: &mut World, persona_id: i64, event_session_id: Option<i64>) -> Result<(), Fault> {
    let car_entity = persona::get_default_car_entity(w, persona_id)?;
    // `find(null)`: Hibernate refuses a null id.
    let event_session_id =
        event_session_id.ok_or_else(|| Fault::Internal(anyhow!("id to load is required for loading")))?;
    let event_session_entity =
        event_session::find(w.db(), event_session_id)?.ok_or_else(|| Fault::null_pointer("EventSessionEntity"))?;
    let car_entity = car_entity.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
    let mut event_data_entity = EventDataEntity {
        persona_id: Some(persona_id),
        event_session_id: Some(event_session_id),
        event: event_session_entity.event,
        server_time_started: datetime::current_time_millis(&w.env),
        car_class_hash: Some(car_entity.car_class_hash),
        car_rating: Some(car_entity.rating),
        ..Default::default()
    };
    event_data::insert(w.db(), &mut event_data_entity)?;
    Ok(())
}

// port: bo/EventBO#createEventSession(TokenSessionEntity,int)
/// A new session of the event, or `None` when there's no such event or no
/// driver is selected. Events of a class other than the open one need the
/// driver's car to be of that class; whether the event is enabled isn't
/// checked.
pub fn create_event_session(
    w: &mut World,
    token_session: &TokenSession,
    event_id: i32,
) -> Result<Option<EventSessionEntity>, Fault> {
    let Some(event_entity) = event::find(w.db(), event_id)? else { return Ok(None) };
    let active_persona_id =
        token_session.active_persona_id.ok_or_else(|| Fault::null_pointer("TokenSessionEntity.activePersonaId"))?;
    if active_persona_id == 0 {
        return Ok(None);
    }
    let car_entity = persona::get_default_car_entity(w, active_persona_id)?;

    // only check restriction on non-open events
    if event_entity.car_class_hash != OPEN_CLASS {
        let car_entity = car_entity.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
        if car_entity.car_class_hash != event_entity.car_class_hash {
            // The client UI does not allow you to join events outside your current car's class
            return Err(Fault::engine(EngineExceptionCode::CarDataInvalid, true));
        }
    }

    let mut event_session_entity = EventSessionEntity {
        event: event_entity,
        started: Some(datetime::current_time_millis(&w.env)),
        ..Default::default()
    };
    event_session::insert(w.db(), &mut event_session_entity)?;
    Ok(Some(event_session_entity))
}

// port: bo/EventBO#findEventSessionById(Long)
/// The session, with its event. Its reward configurations are loaded by
/// whoever reads them.
pub fn find_event_session_by_id(w: &World, id: Option<i64>) -> Result<EventSessionEntity, Fault> {
    let id = id.ok_or_else(|| Fault::Internal(anyhow!("id to load is required for loading")))?;
    event_session::find(w.db(), id)?.ok_or_else(|| Fault::null_pointer("EventSessionEntity"))
}

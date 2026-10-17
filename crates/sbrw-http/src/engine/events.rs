//! `api/Events`: the event list, and the treasure hunt's coins and rewards.
//! As upstream, each call into a BO is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::event::{self, OPEN_CLASS};
use sbrw_core::bo::{events, persona};
use sbrw_core::dao::event::EventEntity;
use sbrw_xml::ToXml;
use sbrw_xml::dto::http::{
    ArrayOfEventDefinition, ArrayOfInt, EventDefinition, EventsPacket, TreasureHuntEventSession, Vector3,
};

use super::{Call, entity};
use crate::AppState;

/// `RequestSessionInfo.getActivePersonaId()`, which BOs dereference.
fn active_persona_id(call: &Call) -> Result<i64, Fault> {
    call.session().active_persona_id.ok_or_else(|| Fault::null_pointer("RequestSessionInfo.activePersonaId"))
}

// port: api/Events#availableAtLevel()
/// The enabled events for the driver's level. Those of another class than
/// the default car's are locked, except open ones.
pub(super) async fn available_at_level(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = active_persona_id(call)?;
    let world = state.world();
    let default_car = world.call(move |w| persona::get_default_car(w, active_persona_id)).await?;
    let car_class_hash =
        default_car.custom_car.ok_or_else(|| Fault::null_pointer("OwnedCarTrans.customCar"))?.car_class_hash;

    let available_at_level = world.call(move |w| event::available_at_level(w, active_persona_id)).await?;
    let mut array_of_event_definition = ArrayOfEventDefinition::default();
    for mut event_entity in available_at_level {
        if event_entity.car_class_hash != OPEN_CLASS && car_class_hash != event_entity.car_class_hash {
            event_entity.is_locked = true;
        }
        array_of_event_definition.event_definition.push(get_event_definition_with_id(&event_entity));
    }
    let events_packet = EventsPacket { events: Some(array_of_event_definition) };
    Ok(entity(call, StatusCode::OK, events_packet.to_xml()))
}

// port: api/Events#getEventDefinitionWithId(EventEntity)
/// The event as the list shows it: most of it fixed placeholders.
fn get_event_definition_with_id(event_entity: &EventEntity) -> EventDefinition {
    EventDefinition {
        car_class_hash: event_entity.car_class_hash,
        coins: 0,
        engage_point: Some(Vector3 { x: 0.0, y: 0.0, z: 0.0 }),
        event_id: event_entity.id,
        event_localization: 953953223,
        event_mode_description_localization: 1204604434,
        event_mode_icon: Some("GameModeIcon_Sprint".into()),
        event_mode_id: event_entity.event_mode_id,
        event_mode_localization: -1152300457,
        is_enabled: event_entity.is_enabled,
        is_locked: event_entity.is_locked,
        laps: 0,
        length: 0.0,
        max_class_rating: event_entity.max_car_class_rating,
        max_entrants: 2,
        max_level: event_entity.max_level,
        min_class_rating: event_entity.min_car_class_rating,
        min_entrants: 2,
        min_level: event_entity.min_level,
        region_localization: 0,
        reward_modes: Some(ArrayOfInt::default()),
        time_limit: 0.0,
        track_layout_map: Some("nothing".into()),
        track_localization: -491522157,
    }
}

// port: api/Events#instancedAccolades(Long)
pub(super) fn instanced_accolades(call: &Call) -> Result<Response, Fault> {
    call.query_long("eventSessionId")?;
    Ok(entity(call, StatusCode::OK, String::new()))
}

// port: api/Events#getTreasureHuntEventSession()
/// Today's treasure hunt, or an empty one with `ENABLE_TREASURE_HUNT` off.
pub(super) async fn get_treasure_hunt_event_session(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let world = state.world();
    let treasure_hunt_event_session = if world.call(|w| Ok(w.params.get_bool_param("ENABLE_TREASURE_HUNT"))).await? {
        let active_persona_id = active_persona_id(call)?;
        world.call(move |w| events::get_treasure_hunt_event_session(w, active_persona_id)).await?
    } else {
        TreasureHuntEventSession::default()
    };
    Ok(entity(call, StatusCode::OK, treasure_hunt_event_session.to_xml()))
}

// port: api/Events#notifyCoinCollected(Integer)
/// A coin collected: the day's accolades with the last one, otherwise an
/// empty body (`JAXBUtility.marshal(null)`).
pub(super) async fn notify_coin_collected(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let coins = call.query_int("coins")?;
    let active_persona_id = active_persona_id(call)?;
    let accolades = state.world().call(move |w| events::notify_coin_collected(w, active_persona_id, coins)).await?;
    Ok(entity(call, StatusCode::OK, accolades.map(|a| a.to_xml()).unwrap_or_default()))
}

// port: api/Events#accolades()
pub(super) async fn accolades(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let active_persona_id = active_persona_id(call)?;
    let accolades = state.world().call(move |w| events::accolades(w, active_persona_id, false)).await?;
    Ok(entity(call, StatusCode::OK, accolades.to_xml()))
}

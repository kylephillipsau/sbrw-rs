//! `EventResultBO`: what the event result BOs share, from an arbitration
//! packet to the driver's event data, the result's exit and the EVENT
//! achievements.

use anyhow::anyhow;
use sbrw_xml::dto::http::ExitPath;

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::reward;
use crate::bo::util::achievement_event_context::AchievementEventContext;
use crate::bo::util::arbitration::Arbitration;
use crate::dao::event::EventMode;
use crate::dao::event_data::{self, EventDataEntity};
use crate::dao::event_session::EventSessionEntity;
use crate::dao::persona::PersonaEntity;
use crate::{Fault, World, datetime};

// port: bo/EventResultBO#handle(EventSessionEntity,Long,TA)
// port: bo/EventResultBO#handleInternal(EventSessionEntity,Long,TA)
/// Clear the ModifiedFiles flag (32) from the hacks the game detected, then
/// let the event's mode work out the result.
pub fn handle<P: Arbitration, R>(
    w: &mut World,
    event_session_entity: EventSessionEntity,
    active_persona_id: i64,
    mut packet: P,
    handle_internal: impl FnOnce(&mut World, EventSessionEntity, i64, P) -> Result<R, Fault>,
) -> Result<R, Fault> {
    packet.set_hacks_detected(packet.hacks_detected() & !32); // remove ModifiedFiles flag

    handle_internal(w, event_session_entity, active_persona_id, packet)
}

// port: bo/EventResultBO#prepareBasicEventData(EventDataEntity,Long,TA)
/// The packet's base fields into the driver's event data, with the server's
/// own timing.
pub fn prepare_basic_event_data(
    w: &World,
    event_data_entity: &mut EventDataEntity,
    active_persona_id: i64,
    packet: &dyn Arbitration,
) {
    event_data_entity.alternate_event_duration_in_milliseconds = packet.alternate_event_duration_in_milliseconds();
    event_data_entity.car_id = packet.car_id();
    event_data_entity.event_duration_in_milliseconds = packet.event_duration_in_milliseconds();
    event_data_entity.finish_reason = packet.finish_reason();
    event_data_entity.hacks_detected = packet.hacks_detected();
    event_data_entity.rank = packet.rank();
    event_data_entity.persona_id = Some(active_persona_id);
    event_data_entity.event_mode_id = event_data_entity.event.event_mode_id;
    event_data_entity.server_time_ended = datetime::current_time_millis(&w.env);
    event_data_entity.server_time_in_milliseconds =
        event_data_entity.server_time_ended - event_data_entity.server_time_started;
}

/// Race Again's exit: a single-player session's result always exits to
/// freeroam. The rest of `prepareRaceAgain`, a new lobby for a multiplayer
/// session's racers, arrives with lobbies.
pub fn prepare_race_again(event_session_entity: &EventSessionEntity) -> Result<ExitPath, Fault> {
    if event_session_entity.lobby.is_some() && event_session_entity.event.is_race_again_enabled {
        return Err(anyhow!("Race Again for a lobby's session isn't ported yet").into());
    }
    Ok(ExitPath::EXIT_TO_FREEROAM)
}

/// `eventDataDao.getRacers`, as Hibernate answers it in the middle of a
/// result: the driver's own row is the instance already being changed, not
/// what the table holds.
pub fn get_racers(w: &World, event_data_entity: &EventDataEntity) -> Result<Vec<EventDataEntity>, Fault> {
    let event_session_id =
        event_data_entity.event_session_id.ok_or_else(|| Fault::null_pointer("EventDataEntity.eventSessionId"))?;
    Ok(event_data::get_racers(w.db(), event_session_id)?
        .into_iter()
        .map(|racer| if racer.id == event_data_entity.id { event_data_entity.clone() } else { racer })
        .collect())
}

// port: bo/EventResultBO#updateEventAchievements(EventDataEntity,EventSessionEntity,Long,TA,AchievementTransaction)
/// An EVENT update for the achievements, with the event, the driver's data,
/// the session, the packet and the default car.
pub fn update_event_achievements(
    w: &mut World,
    event_data_entity: &EventDataEntity,
    event_session_entity: &EventSessionEntity,
    persona_entity: &mut PersonaEntity,
    packet: &dyn Arbitration,
    transaction: &mut AchievementTransaction,
) -> Result<(), Fault> {
    let event_entity = event_data_entity.event.clone();
    let event_context = AchievementEventContext::new(
        EventMode::from_id(event_entity.event_mode_id)?,
        packet.packet().to_owned(),
        event_session_entity.clone(),
    );
    // `Map.of` refuses a null value.
    let car = reward::default_car_of(w, persona_entity)?.ok_or_else(|| Fault::null_pointer("car"))?;
    transaction.add(
        "EVENT",
        Bindings::from([
            ("persona".into(), Binding::Persona(persona_entity.clone())),
            ("event".into(), Binding::Event(Box::new(event_entity))),
            ("eventData".into(), Binding::EventData(Box::new(event_data_entity.clone()))),
            ("eventSession".into(), Binding::EventSession(Box::new(event_session_entity.clone()))),
            ("eventContext".into(), Binding::EventCtx(Box::new(event_context))),
            ("car".into(), Binding::OwnedCar(Box::new(car))),
        ]),
    )
}

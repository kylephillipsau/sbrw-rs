//! `EventResultBO`: what the event result BOs share, from an arbitration
//! packet to the driver's event data, the result's exit and the EVENT
//! achievements.

use anyhow::anyhow;
use sbrw_xml::dto::http::ExitPath;

use crate::bo::achievement_transaction::{AchievementTransaction, Binding, Bindings};
use crate::bo::util::achievement_event_context::AchievementEventContext;
use crate::bo::util::arbitration::Arbitration;
use crate::bo::{lobby, reward};
use crate::dao::event::EventMode;
use crate::dao::event_data::{self, EventDataEntity};
use crate::dao::event_session::EventSessionEntity;
use crate::dao::lobby as lobby_dao;
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

/// What `prepareRaceAgain` sets on a result.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RaceAgain {
    pub exit_path: ExitPath,
    pub lobby_invite_id: i64,
    pub invite_lifetime_in_milliseconds: i32,
}

// port: bo/EventResultBO#prepareRaceAgain(EventSessionEntity,TR)
/// Race Again, for a lobby's session of an event that has it: the session's
/// next lobby, made by the first racer to finish, and an invitation into it
/// while it has more than six seconds of countdown left. Otherwise back to
/// freeroam.
pub fn prepare_race_again(w: &mut World, event_session_entity: &mut EventSessionEntity) -> Result<RaceAgain, Fault> {
    let mut race_again =
        RaceAgain { exit_path: ExitPath::EXIT_TO_FREEROAM, lobby_invite_id: 0, invite_lifetime_in_milliseconds: 0 };
    let event_entity = &event_session_entity.event;

    // Only set up Race Again if it's enabled for the event AND the session was multiplayer
    if let Some(old_lobby) = &event_session_entity.lobby
        && event_entity.is_race_again_enabled
    {
        // If nextLobby hasn't been set, create a new lobby
        let next_lobby_entity = match event_session_entity.next_lobby_id {
            Some(next_lobby_id) => {
                lobby_dao::find(w.db(), next_lobby_id)?.ok_or_else(|| Fault::null_pointer("LobbyEntity"))?
            }
            None => {
                let persona_id = old_lobby.persona_id.ok_or_else(|| anyhow!("id to load is required for loading"))?;
                let is_private = old_lobby.is_private.ok_or_else(|| Fault::null_pointer("LobbyEntity.isPrivate"))?;
                let next_lobby_entity =
                    lobby::create_lobby(w, persona_id, old_lobby.event.id, old_lobby.event.car_class_hash, is_private)?;
                event_session_entity.next_lobby_id = Some(next_lobby_entity.id);
                next_lobby_entity
            }
        };

        let invite_lifetime = next_lobby_entity.get_lobby_countdown_in_milliseconds(
            event_session_entity.event.lobby_countdown_time,
            datetime::current_time_millis(&w.env),
        );

        // lobby must have more than 6 seconds left
        if invite_lifetime > 6000 {
            race_again.lobby_invite_id = next_lobby_entity.id;
            race_again.invite_lifetime_in_milliseconds = invite_lifetime;
            race_again.exit_path = ExitPath::EXIT_TO_LOBBY;
        }
    }
    Ok(race_again)
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

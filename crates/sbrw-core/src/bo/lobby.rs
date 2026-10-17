//! `LobbyBO`: multiplayer lobbies. Drivers find one by racing now or by
//! choosing an event, or make one; they're invited, and they're in it once
//! they accept. A lobby launches when its countdown runs out
//! (`lobby_countdown`).

use sbrw_xml::dto::http::{ArrayOfLobbyEntrantInfo, LobbyCountdown, LobbyEntrantInfo, LobbyInfo};

use crate::bo::event::OPEN_CLASS;
use crate::bo::{lobby_countdown, lobby_messaging, matchmaking, persona};
use crate::dao::event;
use crate::dao::lobby::{self, LobbyEntity, LobbyEntrantEntity};
use crate::dao::persona::{self as persona_dao, PersonaEntity};
use crate::engine::EngineExceptionCode;
use crate::{Fault, World, datetime};

fn find_persona(w: &World, persona_id: i64) -> Result<PersonaEntity, Fault> {
    persona_dao::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))
}

// port: bo/LobbyBO#joinFastLobby(Long,int)
/// Race now: into a lobby started in the last 35 seconds that suits the car
/// and level, chosen at random; into the queue when there's none.
pub fn join_fast_lobby(w: &mut World, persona_id: i64, car_class_hash: i32) -> Result<(), Fault> {
    let persona_entity = find_persona(w, persona_id)?;
    let mut lobbys = lobby::find_all_open(w.db(), car_class_hash, persona_entity.level, datetime::now(&w.env))?;

    if lobbys.is_empty() {
        matchmaking::add_player_to_queue(w, persona_id, car_class_hash);
    } else {
        w.env.random(|r| r.shuffle(&mut lobbys));
        join_lobby(w, &persona_entity, &mut lobbys, true)?;
    }
    Ok(())
}

// port: bo/LobbyBO#joinQueueEvent(Long,int)
/// Into a public lobby of the event started within its countdown, or a new
/// one. Events of a class other than the open one need the default car to
/// be of that class.
pub fn join_queue_event(w: &mut World, persona_id: i64, event_id: i32) -> Result<(), Fault> {
    let persona_entity = find_persona(w, persona_id)?;
    let event_entity = event::find(w.db(), event_id)?;

    let car_entity = persona::get_default_car_entity(w, persona_id)?;
    let event_entity = event_entity.ok_or_else(|| Fault::null_pointer("EventEntity"))?;

    // only check restriction on non-open events
    if event_entity.car_class_hash != OPEN_CLASS {
        let car_entity = car_entity.ok_or_else(|| Fault::null_pointer("CarEntity"))?;
        if car_entity.car_class_hash != event_entity.car_class_hash {
            // The client UI does not allow you to join events outside your current car's class
            return Err(Fault::engine(EngineExceptionCode::CarDataInvalid, true));
        }
    }

    let mut lobbys = lobby::find_by_event_started(w.db(), &event_entity, datetime::now(&w.env))?;
    if lobbys.is_empty() {
        create_lobby(w, persona_entity.persona_id, event_id, event_entity.car_class_hash, false)?;
    } else {
        join_lobby(w, &persona_entity, &mut lobbys, false)?;
    }
    Ok(())
}

// port: bo/LobbyBO#createPrivateLobby(Long,int)
/// A private lobby, and invitations to the creator's chat group. A creator
/// in no group makes nothing.
pub fn create_private_lobby(w: &mut World, creator_persona_id: i64, event_id: i32) -> Result<(), Fault> {
    let persona_id_list = w.openfire.get_all_persona_by_group(creator_persona_id, &w.effects);
    let event_entity = event::find(w.db(), event_id)?;
    if !persona_id_list.is_empty() {
        let event_entity = event_entity.ok_or_else(|| Fault::null_pointer("EventEntity"))?;
        create_lobby(w, creator_persona_id, event_id, event_entity.car_class_hash, true)?;

        let lobby_entity =
            lobby::find_by_event_and_persona(w.db(), &event_entity, creator_persona_id, datetime::now(&w.env))?;
        if let Some(lobby_entity) = lobby_entity {
            for recipient_persona_id in persona_id_list {
                if recipient_persona_id != creator_persona_id {
                    let recipient_persona_entity = find_persona(w, recipient_persona_id)?;

                    lobby_messaging::send_lobby_invitation(
                        w,
                        &lobby_entity,
                        &recipient_persona_entity,
                        i64::from(event_entity.lobby_countdown_time),
                    )?;
                }
            }
        }
    }
    Ok(())
}

// port: bo/LobbyBO#createLobby(Long,int,int,Boolean)
/// A lobby for the event, its creator's invitation, for a public one an
/// invitation from the queue for every other seat, and its countdown. The
/// queue gives the same driver each time, as nothing takes them off it.
pub fn create_lobby(
    w: &mut World,
    persona_id: i64,
    event_id: i32,
    car_class_hash: i32,
    is_private: bool,
) -> Result<LobbyEntity, Fault> {
    let event_entity = event::find(w.db(), event_id)?.ok_or_else(|| Fault::null_pointer("EventEntity"))?;

    let mut lobby_entity = LobbyEntity {
        event: event_entity.clone(),
        is_private: Some(is_private),
        persona_id: Some(persona_id),
        started_time: Some(datetime::now(&w.env)),
        ..Default::default()
    };

    lobby::insert(w.db(), &mut lobby_entity)?;

    let persona_entity = find_persona(w, persona_id)?;
    lobby_messaging::send_lobby_invitation(w, &lobby_entity, &persona_entity, 10000)?;

    if !is_private {
        let max_players = lobby_entity.event.max_players;
        for _ in 1..=max_players - 1 {
            if lobby_entity.entrants.len() as i32 >= max_players {
                break;
            }

            let queue_persona_id = matchmaking::get_player_from_queue(w, car_class_hash)?;

            if queue_persona_id != -1
                && !matchmaking::is_event_ignored(w, queue_persona_id, i64::from(event_id))
                && (lobby_entity.entrants.len() as i32) < max_players
            {
                let queue_persona = find_persona(w, queue_persona_id)?;
                lobby_messaging::send_lobby_invitation(
                    w,
                    &lobby_entity,
                    &queue_persona,
                    i64::from(event_entity.lobby_countdown_time),
                )?;
            }
        }
    }

    lobby_countdown::schedule_lobby_start(w, &lobby_entity);

    Ok(lobby_entity)
}

// port: bo/LobbyBO#joinLobby(PersonaEntity,List)
// port: bo/LobbyBO#joinLobby(PersonaEntity,List,boolean)
/// An invitation to the first public lobby with a seat (skipping events the
/// driver declined, when asked to). Upstream adds the driver to that lobby's
/// entrants, but only in memory: they're in it once they accept.
fn join_lobby(
    w: &World,
    persona_entity: &PersonaEntity,
    lobbys: &mut [LobbyEntity],
    check_ignored_events: bool,
) -> Result<(), Fault> {
    let mut chosen = None;
    for (i, lobby_entity) in lobbys.iter_mut().enumerate() {
        if lobby_entity.is_private.ok_or_else(|| Fault::null_pointer("LobbyEntity.isPrivate"))? {
            continue;
        }
        let event = &lobby_entity.event;
        if check_ignored_events && matchmaking::is_event_ignored(w, persona_entity.persona_id, i64::from(event.id)) {
            continue;
        }
        let max_entrants = event.max_players;
        let entrants_size = lobby_entity.entrants.len() as i32;
        if entrants_size < max_entrants {
            chosen = Some(i);
            if !is_persona_inside(persona_entity.persona_id, &lobby_entity.entrants) {
                lobby_entity.entrants.push(LobbyEntrantEntity {
                    persona_id: Some(persona_entity.persona_id),
                    lobby_id: Some(lobby_entity.id),
                    ..Default::default()
                });
            }
            break;
        }
    }
    if let Some(i) = chosen {
        lobby_messaging::send_lobby_invitation(w, &lobbys[i], persona_entity, 10000)?;
    }
    Ok(())
}

// port: bo/LobbyBO#isPersonaInside(Long,List)
fn is_persona_inside(persona_id: i64, lobby_entrants: &[LobbyEntrantEntity]) -> bool {
    lobby_entrants.iter().any(|e| e.persona_id == Some(persona_id))
}

// port: bo/LobbyBO#declineinvite(Long,Long)
/// The driver won't be offered the lobby's event from the queue again.
pub fn declineinvite(w: &mut World, active_persona_id: i64, lobby_invite_id: Option<i64>) -> Result<(), Fault> {
    let Some(lobby_entity) = find_lobby(w, lobby_invite_id)? else { return Ok(()) };

    matchmaking::ignore_event(w, active_persona_id, i64::from(lobby_entity.event.id));
    Ok(())
}

/// `lobbyDao.find(id)`: Hibernate refuses a null id.
fn find_lobby(w: &World, lobby_id: Option<i64>) -> Result<Option<LobbyEntity>, Fault> {
    let lobby_id = lobby_id.ok_or_else(|| anyhow::anyhow!("id to load is required for loading"))?;
    lobby::find(w.db(), lobby_id)
}

// port: bo/LobbyBO#acceptinvite(Long,Long)
/// Into the lobby, on the next grid place, while it has a seat and more
/// than six seconds of countdown left: the drivers already in it hear of it,
/// and the driver gets the lobby as it stands.
pub fn acceptinvite(w: &mut World, persona_id: i64, lobby_invite_id: Option<i64>) -> Result<LobbyInfo, Fault> {
    let lobby_entity = find_lobby(w, lobby_invite_id)?;
    let persona_entity = persona_dao::find(w.db(), persona_id)?;

    let Some(mut lobby_entity) = lobby_entity else {
        return Err(Fault::engine(EngineExceptionCode::GameDoesNotExist, false));
    };

    let Some(persona_entity) = persona_entity else {
        return Err(Fault::engine(EngineExceptionCode::PersonaNotFound, false));
    };

    let lobby_invite_id = lobby_entity.id;
    let event_id = lobby_entity.event.id;

    let lobby_countdown = LobbyCountdown {
        lobby_id: lobby_invite_id,
        event_id,
        lobby_countdown_in_milliseconds: lobby_entity.get_lobby_countdown_in_milliseconds(
            lobby_entity.event.lobby_countdown_time,
            datetime::current_time_millis(&w.env),
        ),
        lobby_stuck_duration_in_milliseconds: 10000,
        ..Default::default()
    };

    let mut lobby_entrant_info = Vec::new();

    if lobby_entity.entrants.len() as i32 >= lobby_entity.event.max_players {
        return Err(Fault::engine(EngineExceptionCode::GameLocked, false));
    }

    if lobby_countdown.lobby_countdown_in_milliseconds <= 6000 {
        return Err(Fault::engine(EngineExceptionCode::GameLocked, false));
    }

    matchmaking::remove_player_from_queue(w, persona_id);
    for lobby_entrant_entity in &lobby_entity.entrants {
        let entrant_persona_id =
            lobby_entrant_entity.persona_id.ok_or_else(|| Fault::null_pointer("LobbyEntrantEntity.persona"))?;
        if persona_entity.persona_id != entrant_persona_id {
            let entrant_persona = find_persona(w, entrant_persona_id)?;
            lobby_messaging::send_join_message(w, &lobby_entity, &persona_entity, &entrant_persona)?;
        }
    }
    let mut persona_inside = false;
    for lobby_entrant_entity in &lobby_entity.entrants {
        let entrant_persona_id =
            lobby_entrant_entity.persona_id.ok_or_else(|| Fault::null_pointer("LobbyEntrantEntity.persona"))?;
        let entrant_persona = find_persona(w, entrant_persona_id)?;
        lobby_entrant_info.push(LobbyEntrantInfo {
            persona_id: entrant_persona_id,
            level: entrant_persona.level,
            grid_index: lobby_entrant_entity.grid_index,
            ..Default::default()
        });
        if entrant_persona_id == persona_id {
            persona_inside = true;
        }
    }
    if !persona_inside {
        let lobby_entrant_entity = LobbyEntrantEntity {
            persona_id: Some(persona_entity.persona_id),
            lobby_id: Some(lobby_entity.id),
            grid_index: lobby_entity.entrants.len() as i32,
            ..Default::default()
        };
        let grid_index = lobby_entrant_entity.grid_index;
        lobby_entity.entrants.push(lobby_entrant_entity);
        lobby::update(w.db(), &mut lobby_entity)?;
        lobby_entrant_info.push(LobbyEntrantInfo {
            persona_id: persona_entity.persona_id,
            level: persona_entity.level,
            grid_index,
            ..Default::default()
        });
    }

    Ok(LobbyInfo {
        countdown: Some(lobby_countdown),
        entrants: Some(ArrayOfLobbyEntrantInfo { lobby_entrant_info }),
        event_id,
        lobby_invite_id,
        lobby_id: lobby_invite_id,
        ..Default::default()
    })
}

// port: bo/LobbyBO#removeEntrantFromLobby(Long,Long)
/// Out of the lobby; the others in it (as it was) hear of it, whether or not
/// the driver was in it.
pub fn remove_entrant_from_lobby(w: &mut World, persona_id: i64, lobby_id: i64) -> Result<(), Fault> {
    let lobby_entity = lobby::find(w.db(), lobby_id)?;
    let persona_entity = persona_dao::find(w.db(), persona_id)?;

    let Some(lobby_entity) = lobby_entity else {
        return Err(Fault::engine(EngineExceptionCode::GameDoesNotExist, false));
    };

    let Some(persona_entity) = persona_entity else {
        return Err(Fault::engine(EngineExceptionCode::PersonaNotFound, false));
    };

    lobby::delete_entrant_by_persona_and_lobby(w.db(), persona_entity.persona_id, lobby_entity.id)?;
    for entity in &lobby_entity.entrants {
        let entrant_persona_id = entity.persona_id.ok_or_else(|| Fault::null_pointer("LobbyEntrantEntity.persona"))?;
        if entrant_persona_id != persona_id {
            lobby_messaging::send_leave_message(w, &lobby_entity, &persona_entity, entrant_persona_id);
        }
    }
    Ok(())
}

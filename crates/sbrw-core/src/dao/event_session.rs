//! `EventSessionEntity` (one running of an event) and `EventSessionDAO`.

// port: dao/EventSessionDAO#<init>()

use rusqlite::{Connection, OptionalExtension, params};

use crate::Fault;
use crate::dao::event::{self, EventEntity};
use crate::dao::lobby::{self, LobbyEntity};

/// `EventSessionEntity`, with its EAGER event and lobby. The next lobby is
/// kept as the id it joins on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventSessionEntity {
    pub id: i64,
    pub event: EventEntity,
    pub lobby: Option<LobbyEntity>,
    pub next_lobby_id: Option<i64>,
    pub started: Option<i64>,
    pub ended: Option<i64>,
}

/// `EventSessionDAO.find` (`LongKeyedDAO.find`).
pub fn find(db: &Connection, id: i64) -> Result<Option<EventSessionEntity>, Fault> {
    let row = db
        .prepare_cached("SELECT EVENTID, LOBBYID, NEXTLOBBYID, STARTED, ENDED FROM event_session WHERE ID = ?1")?
        .query_row([id], |r| {
            Ok((
                r.get::<_, Option<i32>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })
        .optional()?;
    let Some((event_id, lobby_id, next_lobby_id, started, ended)) = row else { return Ok(None) };
    let event = match event_id {
        Some(event_id) => event::find(db, event_id)?,
        None => None,
    };
    // A session always has its event: the column is set when it's created.
    let event = event.ok_or_else(|| Fault::null_pointer("EventSessionEntity.event"))?;
    let lobby = match lobby_id {
        Some(lobby_id) => lobby::find(db, lobby_id)?,
        None => None,
    };
    Ok(Some(EventSessionEntity { id, event, lobby, next_lobby_id, started, ended }))
}

/// `EventSessionDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, session: &mut EventSessionEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "INSERT INTO event_session (EVENTID, LOBBYID, NEXTLOBBYID, STARTED, ENDED) VALUES (?1, ?2, ?3, ?4, ?5)",
    )?
    .execute(params![
        session.event.id,
        session.lobby.as_ref().map(|l| l.id),
        session.next_lobby_id,
        session.started,
        session.ended
    ])?;
    session.id = db.last_insert_rowid();
    Ok(())
}

/// `EventSessionDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, session: &EventSessionEntity) -> rusqlite::Result<()> {
    db.prepare_cached(
        "UPDATE event_session SET EVENTID = ?2, LOBBYID = ?3, NEXTLOBBYID = ?4, STARTED = ?5, ENDED = ?6 WHERE ID = ?1",
    )?
    .execute(params![
        session.id,
        session.event.id,
        session.lobby.as_ref().map(|l| l.id),
        session.next_lobby_id,
        session.started,
        session.ended
    ])?;
    Ok(())
}

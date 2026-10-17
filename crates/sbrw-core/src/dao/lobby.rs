//! `LobbyEntity` (a multiplayer lobby, with its entrants), `LobbyEntrantEntity`
//! and their DAOs.

// port: dao/LobbyDAO#<init>()
// port: dao/LobbyEntrantDAO#<init>()

use rusqlite::{Connection, OptionalExtension, Row, params};
use time::{Duration, PrimitiveDateTime};

use crate::dao::event::{self, EventEntity};
use crate::{Fault, datetime};

/// The class of the events any car may enter.
const OPEN_CLASS: i32 = 607_077_938;

/// `LobbyEntity`, with its EAGER event and entrants. `startedTime` keeps the
/// precision it was set with until it's read back from the table, which
/// holds whole seconds.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LobbyEntity {
    pub id: i64,
    pub event: EventEntity,
    pub entrants: Vec<LobbyEntrantEntity>,
    pub started_time: Option<PrimitiveDateTime>,
    pub is_private: Option<bool>,
    pub persona_id: Option<i64>,
}

/// `LobbyEntrantEntity`: a driver in a lobby, and their place on the grid.
/// Its persona is held by the id it joins on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LobbyEntrantEntity {
    pub id: i64,
    pub persona_id: Option<i64>,
    pub lobby_id: Option<i64>,
    pub grid_index: i32,
}

impl LobbyEntrantEntity {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            persona_id: row.get("PERSONAID")?,
            lobby_id: row.get("LOBBYID")?,
            grid_index: row.get("gridIndex")?,
        })
    }
}

impl LobbyEntity {
    // port: jpa/LobbyEntity#getLobbyCountdownInMilliseconds(int)
    /// `getLobbyCountdownInMilliseconds(baseTime)`: how long is left of a
    /// countdown of `base_time` from the lobby's start, which counts from its
    /// whole second.
    pub fn get_lobby_countdown_in_milliseconds(&self, base_time: i32, now_millis: i64) -> i32 {
        match self.started_time {
            Some(started) => {
                let started_second = started.assume_utc().unix_timestamp();
                (i64::from(base_time) - (now_millis - started_second * 1000)) as i32
            }
            None => base_time,
        }
    }
}

/// A lobby row, with its event and entrants.
fn load(
    db: &Connection,
    row: (i64, Option<i32>, Option<String>, Option<bool>, Option<i64>),
) -> Result<LobbyEntity, Fault> {
    let (id, event_id, started_time, is_private, persona_id) = row;
    let event = match event_id {
        Some(event_id) => event::find(db, event_id)?,
        None => None,
    };
    let started_time = started_time
        .map(|t| datetime::from_sql(&t))
        .transpose()
        .map_err(|e| anyhow::anyhow!("lobby {id} startedTime: {e}"))?;
    Ok(LobbyEntity {
        id,
        event: event.ok_or_else(|| Fault::null_pointer("LobbyEntity.event"))?,
        entrants: find_entrants(db, id)?,
        started_time,
        is_private,
        persona_id,
    })
}

fn query(db: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<LobbyEntity>, Fault> {
    let rows: Vec<_> = db
        .prepare_cached(sql)?
        .query_map(params, |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
        .collect::<rusqlite::Result<_>>()?;
    rows.into_iter().map(|row| load(db, row)).collect()
}

const COLUMNS: &str = "l.ID, l.EVENTID, l.startedTime, l.isPrivate, l.personaId";

/// `size(obj.entrants) < obj.event.maxPlayers`.
const NOT_FULL: &str = "(SELECT count(*) FROM lobby_entrant le WHERE le.LOBBYID = l.ID) < e.maxPlayers";

/// `LobbyEntity.entrants`: a bag with no order, which MySQL reads through the
/// foreign key's index, so in id order.
fn find_entrants(db: &Connection, lobby_id: i64) -> rusqlite::Result<Vec<LobbyEntrantEntity>> {
    db.prepare_cached("SELECT * FROM lobby_entrant WHERE LOBBYID = ?1 ORDER BY id")?
        .query_map([lobby_id], LobbyEntrantEntity::from_row)?
        .collect()
}

/// `LobbyDAO.find` (`LongKeyedDAO.find`).
pub fn find(db: &Connection, id: i64) -> Result<Option<LobbyEntity>, Fault> {
    let row = db
        .prepare_cached(&format!("SELECT {COLUMNS} FROM lobby l WHERE l.ID = ?1"))?
        .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        .optional()?;
    row.map(|row| load(db, row)).transpose()
}

/// `LobbyDAO.insert` (`persist`); sets the generated id. Entrants aren't
/// cascaded.
pub fn insert(db: &Connection, lobby: &mut LobbyEntity) -> rusqlite::Result<()> {
    db.prepare_cached("INSERT INTO lobby (isPrivate, personaId, EVENTID, startedTime) VALUES (?1, ?2, ?3, ?4)")?
        .execute(params![
            lobby.is_private,
            lobby.persona_id,
            lobby.event.id,
            datetime::opt_to_sql(lobby.started_time)
        ])?;
    lobby.id = db.last_insert_rowid();
    Ok(())
}

/// `LobbyDAO.update` (`merge`): every mapped column, and the merge cascaded
/// to the entrants: new ones are inserted (and given their ids), the others
/// written back.
pub fn update(db: &Connection, lobby: &mut LobbyEntity) -> rusqlite::Result<()> {
    db.prepare_cached("UPDATE lobby SET isPrivate = ?2, personaId = ?3, EVENTID = ?4, startedTime = ?5 WHERE ID = ?1")?
        .execute(params![
            lobby.id,
            lobby.is_private,
            lobby.persona_id,
            lobby.event.id,
            datetime::opt_to_sql(lobby.started_time)
        ])?;
    for entrant in &mut lobby.entrants {
        if entrant.id == 0 {
            db.prepare_cached("INSERT INTO lobby_entrant (gridIndex, LOBBYID, PERSONAID) VALUES (?1, ?2, ?3)")?
                .execute(params![entrant.grid_index, entrant.lobby_id, entrant.persona_id])?;
            entrant.id = db.last_insert_rowid();
        } else {
            db.prepare_cached("UPDATE lobby_entrant SET gridIndex = ?2, LOBBYID = ?3, PERSONAID = ?4 WHERE id = ?1")?
                .execute(params![entrant.id, entrant.grid_index, entrant.lobby_id, entrant.persona_id])?;
        }
    }
    Ok(())
}

// port: dao/LobbyDAO#findAllOpen(int,int)
// port: jpa/LobbyEntity@LobbyEntity.findAllOpenByCarClass
/// Public and private lobbies started in the last 35 seconds, with a seat
/// left, for the level and of the car's class or open, in key order.
pub fn find_all_open(
    db: &Connection,
    car_class_hash: i32,
    level: i32,
    now: PrimitiveDateTime,
) -> Result<Vec<LobbyEntity>, Fault> {
    let date_past = now - Duration::seconds(35);
    query(
        db,
        &format!(
            "SELECT {COLUMNS} FROM lobby l JOIN event e ON e.ID = l.EVENTID WHERE ?1 >= e.minLevel AND ?1 <= e.maxLevel
             AND l.startedTime || '.000' BETWEEN ?2 AND ?3 AND (e.carClassHash = {OPEN_CLASS} OR e.carClassHash = ?4)
             AND {NOT_FULL} ORDER BY l.ID"
        ),
        params![level, datetime::to_sql_millis(date_past), datetime::to_sql_millis(now), car_class_hash],
    )
}

// port: dao/LobbyDAO#findByEventStarted(EventEntity)
// port: jpa/LobbyEntity@LobbyEntity.findByEventStarted
/// The event's public lobbies with a seat left, started within its
/// countdown, in key order.
pub fn find_by_event_started(
    db: &Connection,
    event_entity: &EventEntity,
    now: PrimitiveDateTime,
) -> Result<Vec<LobbyEntity>, Fault> {
    let date_past = now - Duration::milliseconds(i64::from(event_entity.lobby_countdown_time));
    query(
        db,
        &format!(
            "SELECT {COLUMNS} FROM lobby l JOIN event e ON e.ID = l.EVENTID WHERE l.EVENTID = ?1
             AND l.startedTime || '.000' BETWEEN ?2 AND ?3 AND l.isPrivate = 0 AND {NOT_FULL} ORDER BY l.ID"
        ),
        params![event_entity.id, datetime::to_sql_millis(date_past), datetime::to_sql_millis(now)],
    )
}

// port: dao/LobbyDAO#findByEventAndPersona(EventEntity,Long)
// port: jpa/LobbyEntity@LobbyEntity.findByEventAndPersona
/// The persona's first private lobby for the event started within its
/// countdown.
pub fn find_by_event_and_persona(
    db: &Connection,
    event_entity: &EventEntity,
    persona_id: i64,
    now: PrimitiveDateTime,
) -> Result<Option<LobbyEntity>, Fault> {
    let date_past = now - Duration::milliseconds(i64::from(event_entity.lobby_countdown_time));
    Ok(query(
        db,
        &format!(
            "SELECT {COLUMNS} FROM lobby l WHERE l.EVENTID = ?1 AND l.startedTime || '.000' BETWEEN ?2 AND ?3
             AND l.isPrivate = 1 AND l.personaId = ?4 ORDER BY l.ID"
        ),
        params![event_entity.id, datetime::to_sql_millis(date_past), datetime::to_sql_millis(now), persona_id],
    )?
    .into_iter()
    .next())
}

// port: dao/LobbyEntrantDAO#deleteByPersonaAndLobby(PersonaEntity,LobbyEntity)
// port: jpa/LobbyEntrantEntity@LobbyEntrantEntity.deleteByPersonaAndLobby
pub fn delete_entrant_by_persona_and_lobby(db: &Connection, persona_id: i64, lobby_id: i64) -> rusqlite::Result<usize> {
    db.prepare_cached("DELETE FROM lobby_entrant WHERE PERSONAID = ?1 AND LOBBYID = ?2")?
        .execute([persona_id, lobby_id])
}

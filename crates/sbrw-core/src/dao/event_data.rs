//! `EventDataEntity` (one driver's run in an event session, with what their
//! arbitration packet said) and `EventDataDAO`.

// port: dao/EventDataDAO#<init>()

use rusqlite::{Connection, Row, params};

use crate::Fault;
use crate::dao::event::{self, EventEntity};

/// `EventDataEntity`, with its EAGER event.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EventDataEntity {
    pub id: i64,
    pub event: EventEntity,
    pub persona_id: Option<i64>,
    pub event_mode_id: i32,
    pub event_session_id: Option<i64>,
    pub alternate_event_duration_in_milliseconds: i64,
    pub car_id: i64,
    pub event_duration_in_milliseconds: i64,
    pub finish_reason: i32,
    pub hacks_detected: i64,
    pub rank: i32,
    pub is_legit: bool,
    pub server_time_in_milliseconds: i64,
    pub server_time_started: i64,
    pub server_time_ended: i64,
    pub car_class_hash: Option<i32>,
    pub car_rating: Option<i32>,
    pub longest_jump_duration_in_milliseconds: i64,
    pub sum_of_jumps_duration_in_milliseconds: i64,
    pub top_speed: f32,
    pub best_lap_duration_in_milliseconds: i64,
    pub fraction_completed: f32,
    pub number_of_collisions: i32,
    pub perfect_start: i32,
    pub cops_deployed: i32,
    pub cops_disabled: i32,
    pub cops_rammed: i32,
    pub cost_to_state: i32,
    pub infractions: i32,
    pub road_blocks_dodged: i32,
    pub spike_strips_dodged: i32,
    pub heat: f32,
    pub busted_count: i32,
    pub distance_to_finish: f32,
}

/// Every column but the id and the event, in table order.
const COLUMNS: &str = "alternateEventDurationInMilliseconds, bestLapDurationInMilliseconds, bustedCount, carId,
    copsDeployed, copsDisabled, copsRammed, costToState, distanceToFinish, eventDurationInMilliseconds, eventModeId,
    eventSessionId, finishReason, fractionCompleted, hacksDetected, heat, infractions,
    longestJumpDurationInMilliseconds, numberOfCollisions, perfectStart, personaId, rank, roadBlocksDodged,
    spikeStripsDodged, sumOfJumpsDurationInMilliseconds, topSpeed, EVENTID, isLegit, serverTimeInMilliseconds,
    serverTimeStarted, serverTimeEnded, carClassHash, carRating";

impl EventDataEntity {
    /// The row, and the id of the event it joins on.
    fn from_row(row: &Row<'_>) -> rusqlite::Result<(Self, Option<i32>)> {
        // MySQL FLOAT columns.
        let float = |name: &str| row.get::<_, f64>(name).map(|v| v as f32);
        let data = Self {
            id: row.get("ID")?,
            event: EventEntity::default(),
            persona_id: row.get("personaId")?,
            event_mode_id: row.get("eventModeId")?,
            event_session_id: row.get("eventSessionId")?,
            alternate_event_duration_in_milliseconds: row.get("alternateEventDurationInMilliseconds")?,
            car_id: row.get("carId")?,
            event_duration_in_milliseconds: row.get("eventDurationInMilliseconds")?,
            finish_reason: row.get("finishReason")?,
            hacks_detected: row.get("hacksDetected")?,
            rank: row.get("rank")?,
            is_legit: row.get::<_, Option<bool>>("isLegit")?.unwrap_or_default(),
            server_time_in_milliseconds: row.get::<_, Option<i64>>("serverTimeInMilliseconds")?.unwrap_or_default(),
            server_time_started: row.get::<_, Option<i64>>("serverTimeStarted")?.unwrap_or_default(),
            server_time_ended: row.get::<_, Option<i64>>("serverTimeEnded")?.unwrap_or_default(),
            car_class_hash: row.get("carClassHash")?,
            car_rating: row.get("carRating")?,
            longest_jump_duration_in_milliseconds: row.get("longestJumpDurationInMilliseconds")?,
            sum_of_jumps_duration_in_milliseconds: row.get("sumOfJumpsDurationInMilliseconds")?,
            top_speed: float("topSpeed")?,
            best_lap_duration_in_milliseconds: row.get("bestLapDurationInMilliseconds")?,
            fraction_completed: float("fractionCompleted")?,
            number_of_collisions: row.get("numberOfCollisions")?,
            perfect_start: row.get("perfectStart")?,
            cops_deployed: row.get("copsDeployed")?,
            cops_disabled: row.get("copsDisabled")?,
            cops_rammed: row.get("copsRammed")?,
            cost_to_state: row.get("costToState")?,
            infractions: row.get("infractions")?,
            road_blocks_dodged: row.get("roadBlocksDodged")?,
            spike_strips_dodged: row.get("spikeStripsDodged")?,
            heat: float("heat")?,
            busted_count: row.get("bustedCount")?,
            distance_to_finish: float("distanceToFinish")?,
        };
        Ok((data, row.get("EVENTID")?))
    }

    fn values(&self) -> [rusqlite::types::Value; 33] {
        use rusqlite::types::Value;
        let int = |v: i32| Value::Integer(i64::from(v));
        let opt = |v: Option<i64>| v.map_or(Value::Null, Value::Integer);
        let float = |v: f32| Value::Real(f64::from(v));
        [
            Value::Integer(self.alternate_event_duration_in_milliseconds),
            Value::Integer(self.best_lap_duration_in_milliseconds),
            int(self.busted_count),
            Value::Integer(self.car_id),
            int(self.cops_deployed),
            int(self.cops_disabled),
            int(self.cops_rammed),
            int(self.cost_to_state),
            float(self.distance_to_finish),
            Value::Integer(self.event_duration_in_milliseconds),
            int(self.event_mode_id),
            opt(self.event_session_id),
            int(self.finish_reason),
            float(self.fraction_completed),
            Value::Integer(self.hacks_detected),
            float(self.heat),
            int(self.infractions),
            Value::Integer(self.longest_jump_duration_in_milliseconds),
            int(self.number_of_collisions),
            int(self.perfect_start),
            opt(self.persona_id),
            int(self.rank),
            int(self.road_blocks_dodged),
            int(self.spike_strips_dodged),
            Value::Integer(self.sum_of_jumps_duration_in_milliseconds),
            float(self.top_speed),
            int(self.event.id),
            Value::Integer(i64::from(self.is_legit)),
            Value::Integer(self.server_time_in_milliseconds),
            Value::Integer(self.server_time_started),
            Value::Integer(self.server_time_ended),
            opt(self.car_class_hash.map(i64::from)),
            opt(self.car_rating.map(i64::from)),
        ]
    }
}

fn load(db: &Connection, sql: &str, params: impl rusqlite::Params) -> Result<Vec<EventDataEntity>, Fault> {
    let rows: Vec<(EventDataEntity, Option<i32>)> =
        db.prepare_cached(sql)?.query_map(params, EventDataEntity::from_row)?.collect::<rusqlite::Result<_>>()?;
    rows.into_iter()
        .map(|(mut data, event_id)| {
            let event = match event_id {
                Some(event_id) => event::find(db, event_id)?,
                None => None,
            };
            data.event = event.ok_or_else(|| Fault::null_pointer("EventDataEntity.event"))?;
            Ok(data)
        })
        .collect()
}

// port: dao/EventDataDAO#getRacers(Long)
// port: jpa/EventDataEntity@EventDataEntity.getRacers
/// Everyone's data for a session, by persona id: MySQL reads them through
/// the unique index on (session, persona).
pub fn get_racers(db: &Connection, event_session_id: i64) -> Result<Vec<EventDataEntity>, Fault> {
    load(db, "SELECT * FROM event_data WHERE eventSessionId = ?1 ORDER BY personaId", [event_session_id])
}

// port: dao/EventDataDAO#findByPersonaAndEventSessionId(Long,Long)
// port: jpa/EventDataEntity@EventDataEntity.findByPersonaAndEventSessionId
pub fn find_by_persona_and_event_session_id(
    db: &Connection,
    persona_id: i64,
    event_session_id: i64,
) -> Result<Option<EventDataEntity>, Fault> {
    Ok(load(
        db,
        "SELECT * FROM event_data WHERE personaId = ?1 AND eventSessionId = ?2",
        params![persona_id, event_session_id],
    )?
    .into_iter()
    .next())
}

/// `EventDataDAO.insert` (`persist`); sets the generated id.
pub fn insert(db: &Connection, data: &mut EventDataEntity) -> rusqlite::Result<()> {
    let placeholders = (1..=33).map(|i| format!("?{i}")).collect::<Vec<_>>().join(", ");
    db.prepare_cached(&format!("INSERT INTO event_data ({COLUMNS}) VALUES ({placeholders})"))?
        .execute(rusqlite::params_from_iter(data.values()))?;
    data.id = db.last_insert_rowid();
    Ok(())
}

/// `EventDataDAO.update` (`merge`): every mapped column, from this copy.
pub fn update(db: &Connection, data: &EventDataEntity) -> rusqlite::Result<()> {
    let assignments = COLUMNS
        .split(',')
        .enumerate()
        .map(|(i, column)| format!("{} = ?{}", column.trim(), i + 2))
        .collect::<Vec<_>>()
        .join(", ");
    let mut values = vec![rusqlite::types::Value::Integer(data.id)];
    values.extend(data.values());
    db.prepare_cached(&format!("UPDATE event_data SET {assignments} WHERE ID = ?1"))?
        .execute(rusqlite::params_from_iter(values))?;
    Ok(())
}

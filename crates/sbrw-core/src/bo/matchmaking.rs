//! `MatchmakingBO`: the "race now" queue and each persona's ignored events.
//! Disabled (every call a no-op) unless `ENABLE_REDIS` is true, as upstream.

use anyhow::anyhow;

use crate::{Fault, World};

const QUEUE: &str = "matchmaking_queue";

/// Car class hash that matches every car class.
const OPEN_CLASS: i32 = 607_077_938;

// port: bo/MatchmakingBO#addPlayerToQueue(Long,Integer)
pub fn add_player_to_queue(w: &mut World, persona_id: i64, car_class: i32) {
    if let Some(redis) = &mut w.redis {
        redis.hset(QUEUE, &persona_id.to_string(), &car_class.to_string());
    }
}

// port: bo/MatchmakingBO#removePlayerFromQueue(Long)
pub fn remove_player_from_queue(w: &mut World, persona_id: i64) {
    if let Some(redis) = &mut w.redis {
        redis.hdel(QUEUE, &[&persona_id.to_string()]);
    }
}

// port: bo/MatchmakingBO#getPlayerFromQueue(Integer)
/// The first queued persona whose car is in `car_class` (any, for the open
/// class), or -1.
pub fn get_player_from_queue(w: &World, car_class: i32) -> Result<i64, Fault> {
    let Some(redis) = &w.redis else { return Ok(-1) };
    for (persona, class) in redis.hscan(QUEUE) {
        let class: i32 = if car_class == OPEN_CLASS {
            0
        } else {
            class.parse().map_err(|_| anyhow!("For input string: \"{class}\""))?
        };
        if car_class == OPEN_CLASS || class == car_class {
            return Ok(persona.parse().map_err(|_| anyhow!("For input string: \"{persona}\""))?);
        }
    }
    Ok(-1)
}

fn ignored_events_key(persona_id: i64) -> String {
    format!("ignored_events.{persona_id}")
}

// port: bo/MatchmakingBO#ignoreEvent(long,long)
pub fn ignore_event(w: &mut World, persona_id: i64, event_id: i64) {
    if let Some(redis) = &mut w.redis {
        redis.sadd(&ignored_events_key(persona_id), &[&event_id.to_string()]);
    }
}

// port: bo/MatchmakingBO#resetIgnoredEvents(long)
pub fn reset_ignored_events(w: &mut World, persona_id: i64) {
    if let Some(redis) = &mut w.redis {
        redis.del(&[&ignored_events_key(persona_id)]);
    }
}

// port: bo/MatchmakingBO#isEventIgnored(long,long)
pub fn is_event_ignored(w: &World, persona_id: i64, event_id: i64) -> bool {
    w.redis.as_ref().is_some_and(|r| r.sismember(&ignored_events_key(persona_id), &event_id.to_string()))
}

// port: bo/MatchmakingBO#handlePersonaPresenceUpdated(PersonaPresenceUpdated)
/// Observer of presence changes (asynchronous upstream).
pub fn handle_persona_presence_updated(w: &mut World, persona_id: i64) -> Result<(), Fault> {
    remove_player_from_queue(w, persona_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_matches_by_class_or_any_for_open_events() {
        let mut w = World::test(&[]);
        add_player_to_queue(&mut w, 100, 7);
        add_player_to_queue(&mut w, 101, 8);
        assert_eq!(get_player_from_queue(&w, 8).unwrap(), 101);
        assert_eq!(get_player_from_queue(&w, 9).unwrap(), -1);
        assert_eq!(get_player_from_queue(&w, OPEN_CLASS).unwrap(), 100, "first queued");
        remove_player_from_queue(&mut w, 100);
        assert_eq!(get_player_from_queue(&w, OPEN_CLASS).unwrap(), 101);

        ignore_event(&mut w, 101, 42);
        assert!(is_event_ignored(&w, 101, 42));
        assert!(!is_event_ignored(&w, 100, 42));
        reset_ignored_events(&mut w, 101);
        assert!(!is_event_ignored(&w, 101, 42));

        let mut off = World::test(&[("ENABLE_REDIS", "false")]);
        add_player_to_queue(&mut off, 100, 7);
        assert_eq!(get_player_from_queue(&off, OPEN_CLASS).unwrap(), -1);
    }
}

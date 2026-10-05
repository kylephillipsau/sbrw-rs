//! `PresenceBO`: whether each persona is online, and telling interested
//! parties when that changes. Disabled unless `ENABLE_REDIS` is true.

use anyhow::anyhow;

use crate::bo::matchmaking;
use crate::{Fault, World};

// port: bo/PresenceBO#getPresenceKey(Long)
fn presence_key(persona_id: i64) -> String {
    format!("game_presence.{persona_id}")
}

// port: bo/PresenceBO#updatePresence(Long,Long)
/// Asynchronous when called from outside the bean upstream: callers outside
/// this module queue it with [`World::queue_async`]; `remove_presence` calls
/// it directly, as upstream's self-call bypasses the container.
pub fn update_presence(w: &mut World, persona_id: i64, presence: i64) -> Result<(), Fault> {
    if w.redis.is_none() || persona_id == 0 {
        return Ok(());
    }
    if get_presence(w, persona_id)? != presence {
        let redis = w.redis.as_mut().expect("checked above");
        redis.set(&presence_key(persona_id), &presence.to_string());
        redis.publish("game_presence_updates", &format!("{persona_id}|{presence}"));
        // PersonaPresenceUpdated: its observers are asynchronous beans.
        w.queue_async(move |w| matchmaking::handle_persona_presence_updated(w, persona_id));
        w.queue_async(move |_| {
            Err(Fault::unported("bo/SocialRelationshipBO#handlePersonaPresenceUpdated(PersonaPresenceUpdated)"))
        });
    }
    Ok(())
}

// port: bo/PresenceBO#removePresence(Long)
pub fn remove_presence(w: &mut World, persona_id: i64) -> Result<(), Fault> {
    if w.redis.is_some() && persona_id != 0 {
        update_presence(w, persona_id, 0)?;
        w.redis.as_mut().expect("checked above").del(&[&presence_key(persona_id)]);
    }
    Ok(())
}

// port: bo/PresenceBO#getPresence(Long)
pub fn get_presence(w: &World, persona_id: i64) -> Result<i64, Fault> {
    let Some(redis) = &w.redis else { return Ok(0) };
    match redis.get(&presence_key(persona_id)) {
        None => Ok(0),
        Some(v) if v.trim().is_empty() => Ok(0),
        Some(v) => Ok(v.parse().map_err(|_| anyhow!("For input string: \"{v}\""))?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redis_commands(w: &World) -> Vec<String> {
        w.effects.drain().redis.into_iter().map(|c| format!("{} {}", c.command, serde_json::json!(c.args))).collect()
    }

    #[test]
    fn changes_are_published_and_observed_after_the_request() {
        let mut w = World::test(&[]);
        w.transaction(|w| update_presence(w, 100, 1)).unwrap();
        w.transaction(|w| update_presence(w, 100, 1)).unwrap();
        assert_eq!(get_presence(&w, 100).unwrap(), 1);
        w.transaction(|w| remove_presence(w, 100)).unwrap();
        assert_eq!(get_presence(&w, 100).unwrap(), 0);
        assert_eq!(
            redis_commands(&w),
            [
                r#"SET ["game_presence.100","1"]"#,
                r#"PUBLISH ["game_presence_updates","100|1"]"#,
                r#"SET ["game_presence.100","0"]"#,
                r#"PUBLISH ["game_presence_updates","100|0"]"#,
                r#"DEL ["game_presence.100"]"#,
            ],
            "an unchanged presence is not set again"
        );
        // Each change has two observers; SocialRelationshipBO's fails (unported) without
        // stopping the rest.
        w.run_async_jobs();
        assert_eq!(redis_commands(&w), [r#"HDEL ["matchmaking_queue",["100"]]"#; 2]);
        w.run_async_jobs();
        assert!(redis_commands(&w).is_empty());
    }

    #[test]
    fn persona_zero_and_disabled_redis_do_nothing() {
        let mut w = World::test(&[]);
        w.transaction(|w| remove_presence(w, 0)).unwrap();
        assert!(redis_commands(&w).is_empty());

        let mut w = World::test(&[("ENABLE_REDIS", "false")]);
        assert!(w.redis.is_none());
        w.transaction(|w| update_presence(w, 100, 1)).unwrap();
        w.transaction(|w| remove_presence(w, 100)).unwrap();
        w.run_async_jobs();
        assert_eq!(get_presence(&w, 100).unwrap(), 0);
        assert!(redis_commands(&w).is_empty());
    }
}

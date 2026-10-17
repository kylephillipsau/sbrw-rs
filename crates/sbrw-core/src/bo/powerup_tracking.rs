//! `PowerupTrackingBO`: a record of each powerup used.

use anyhow::anyhow;

use crate::dao::persona;
use crate::dao::used_powerup::{self, UsedPowerupEntity};
use crate::{Fault, World, datetime};

// port: bo/PowerupTrackingBO#createPowerupRecord(Long,Long,Integer)
/// Record that a persona used a powerup, in an event session if it is in
/// one that exists.
pub fn create_powerup_record(
    w: &mut World,
    event_session_id: Option<i64>,
    active_persona_id: i64,
    powerup_hash: i32,
) -> Result<(), Fault> {
    // The persona is required (`optional = false`): Hibernate refuses the insert without one.
    let persona_entity = persona::find(w.db(), active_persona_id)?.ok_or_else(|| {
        anyhow!("not-null property references a null or transient value: UsedPowerupEntity.personaEntity")
    })?;
    let event_session_id = match event_session_id {
        Some(id) => w
            .db()
            .prepare_cached("SELECT ID FROM event_session WHERE ID = ?1")?
            .query_map([id], |r| r.get::<_, i64>(0))?
            .next()
            .transpose()?,
        None => None,
    };
    let mut used_powerup_entity = UsedPowerupEntity {
        id: 0,
        persona_id: persona_entity.persona_id,
        event_session_id,
        powerup_hash: Some(powerup_hash),
    };
    used_powerup::insert(w.db(), &mut used_powerup_entity, datetime::now(&w.env))?;
    Ok(())
}

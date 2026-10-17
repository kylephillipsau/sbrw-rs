//! `api/Powerups`: a driver used a powerup. As upstream, each call into a BO
//! is its own world call.

use axum::http::StatusCode;
use axum::response::Response;
use sbrw_core::Fault;
use sbrw_core::bo::achievement_transaction::Binding;
use sbrw_core::bo::util::achievement_inventory_context::{AchievementInventoryContext, Event};
use sbrw_core::bo::{achievement, inventory, persona, powerup_tracking};
use sbrw_java::lang::split;
use sbrw_xml::dto::xmpp::{XMPP_PowerupActivatedType, XMPP_ResponseTypePowerupActivated};

use super::{Call, entity};
use crate::AppState;

// port: api/Powerups#activated(Integer,Long,String,Long)
/// Tell each receiver (a `-`-separated list of persona ids; ids of 10 and
/// below are skipped) that the selected driver used the powerup, take a use
/// off its stack when `ENABLE_POWERUP_DECREASE` is on, and record the use.
/// The `eventSessionId` parameter isn't read: the session's event is used.
pub(super) async fn activated(state: &AppState, call: &Call) -> Result<Response, Fault> {
    let powerup_hash = call
        .path_param("powerupHash")
        .map(|v| {
            v.parse::<i32>().map_err(|_| {
                Fault::Internal(anyhow::anyhow!(
                    "Unable to extract parameter from http request: javax.ws.rs.PathParam(\"powerupHash\") value is '{v}'"
                ))
            })
        })
        .transpose()?
        .ok_or_else(|| Fault::null_pointer("powerupHash"))?;
    let target_id = call.query_long("targetId")?;
    let receivers = call.query("receivers");
    call.query_long("eventSessionId")?;
    let session = call.session();
    let active_persona_id = session.active_persona_id;
    let event_session_id = session.event_session_id;

    let powerup_activated_response = XMPP_ResponseTypePowerupActivated {
        powerup_activated: Some(XMPP_PowerupActivatedType {
            id: Some(i64::from(powerup_hash)),
            target_persona_id: target_id,
            persona_id: active_persona_id,
            ..Default::default()
        }),
        ..Default::default()
    };
    let world = state.world();
    let receivers = receivers.ok_or_else(|| Fault::null_pointer("receivers"))?;
    for receiver in split(&receivers, '-') {
        let receiver_persona_id: i64 =
            receiver.parse().map_err(|_| anyhow::anyhow!("NumberFormatException: For input string: \"{receiver}\""))?;
        if receiver_persona_id > 10 {
            let response = powerup_activated_response.clone();
            world
                .call(move |w| {
                    w.openfire.send(&response, receiver_persona_id, &w.effects);
                    Ok(())
                })
                .await?;
        }
    }

    // `personaDAO.find(null)` and the like: Hibernate refuses a null id.
    let active =
        || active_persona_id.ok_or_else(|| Fault::Internal(anyhow::anyhow!("id to load is required for loading")));
    if world.call(|w| Ok(w.params.get_bool_param("ENABLE_POWERUP_DECREASE"))).await? {
        let persona_id = active()?;
        let persona_entity = world.call(move |w| persona::get_persona_by_id(w, persona_id)).await?;
        let lookup = persona_entity.clone();
        let mut inventory_entity = world
            .call(move |w| {
                let persona_entity = lookup.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
                inventory::get_inventory(w, &persona_entity)
            })
            .await?;
        let inventory_item_entity =
            world.call(move |w| inventory::decrease_item_count_by_hash(w, &mut inventory_entity, powerup_hash)).await?;
        world
            .call(move |w| {
                let mut transaction = achievement::create_transaction(persona_id);
                let persona_binding = persona_entity.clone().ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
                transaction.add(
                    "INVENTORY",
                    [
                        ("persona".to_owned(), Binding::Persona(persona_binding)),
                        (
                            "ctx".to_owned(),
                            Binding::InventoryCtx(Box::new(AchievementInventoryContext::new(
                                inventory_item_entity,
                                Event::QuantityDecreased,
                            ))),
                        ),
                    ]
                    .into(),
                )?;
                let mut persona_entity = persona_entity.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
                achievement::commit_transaction(w, &mut persona_entity, &mut transaction)
            })
            .await?;
    }

    let persona_id = active()?;
    world.call(move |w| powerup_tracking::create_powerup_record(w, event_session_id, persona_id, powerup_hash)).await?;

    Ok(entity(call, StatusCode::OK, String::new()))
}

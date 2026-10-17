//! `LobbyMessagingBO`: what lobbies tell their drivers over XMPP.

use anyhow::anyhow;
use sbrw_xml::dto::http::{LobbyEntrantAdded, LobbyEntrantRemoved};
use sbrw_xml::dto::xmpp::{
    XMPP_CryptoTicketsType, XMPP_LobbyInviteType, XMPP_LobbyLaunchedType, XMPP_ResponseTypeEntrantAdded,
    XMPP_ResponseTypeEntrantRemoved, XMPP_ResponseTypeLobbyInvite, XMPP_ResponseTypeLobbyLaunched,
};

use crate::dao::lobby::LobbyEntity;
use crate::dao::persona::PersonaEntity;
use crate::{Fault, World};

// port: bo/LobbyMessagingBO#sendJoinMessage(LobbyEntity,PersonaEntity,PersonaEntity)
/// Tell a driver in the lobby that another has joined.
pub fn send_join_message(
    w: &World,
    lobby_entity: &LobbyEntity,
    source_persona: &PersonaEntity,
    recipient_persona: &PersonaEntity,
) -> Result<(), Fault> {
    if source_persona.persona_id == recipient_persona.persona_id {
        return Err(anyhow!("IllegalArgumentException: Source and recipient personas cannot be the same!").into());
    }

    let response = XMPP_ResponseTypeEntrantAdded {
        lobby_invite: Some(LobbyEntrantAdded {
            heat: 1.0,
            level: source_persona.level,
            persona_id: source_persona.persona_id,
            lobby_id: lobby_entity.id,
            ..Default::default()
        }),
        ..Default::default()
    };

    w.openfire.send(&response, recipient_persona.persona_id, &w.effects);
    Ok(())
}

// port: bo/LobbyMessagingBO#sendLeaveMessage(LobbyEntity,PersonaEntity,PersonaEntity)
/// Tell a driver in the lobby that another has left.
pub fn send_leave_message(
    w: &World,
    lobby_entity: &LobbyEntity,
    source_persona: &PersonaEntity,
    recipient_persona_id: i64,
) {
    let response = XMPP_ResponseTypeEntrantRemoved {
        lobby_entrant_removed: Some(LobbyEntrantRemoved {
            lobby_id: lobby_entity.id,
            persona_id: source_persona.persona_id,
        }),
        ..Default::default()
    };

    w.openfire.send(&response, recipient_persona_id, &w.effects);
}

// port: bo/LobbyMessagingBO#sendLobbyInvitation(LobbyEntity,PersonaEntity,long)
/// Invite a driver to the lobby. Its creator's invitation leaves out who
/// invited them, for how long and whether it's private.
pub fn send_lobby_invitation(
    w: &World,
    lobby_entity: &LobbyEntity,
    recipient_persona: &PersonaEntity,
    invite_lifetime: i64,
) -> Result<(), Fault> {
    let mut lobby_invite = XMPP_LobbyInviteType {
        event_id: i64::from(lobby_entity.event.id),
        lobby_invite_id: lobby_entity.id,
        ..Default::default()
    };

    let creator = lobby_entity.persona_id.ok_or_else(|| Fault::null_pointer("LobbyEntity.personaId"))?;
    if creator != recipient_persona.persona_id {
        lobby_invite.invited_by_persona_id = creator;
        lobby_invite.invite_lifetime_in_milliseconds = invite_lifetime;
        lobby_invite.is_private =
            lobby_entity.is_private.ok_or_else(|| Fault::null_pointer("LobbyEntity.isPrivate"))?;
    }

    let response = XMPP_ResponseTypeLobbyInvite { lobby_invite: Some(lobby_invite), ..Default::default() };

    w.openfire.send(&response, recipient_persona.persona_id, &w.effects);
    Ok(())
}

// port: bo/LobbyMessagingBO#sendRelay(XMPP_LobbyLaunchedType,XMPP_CryptoTicketsType)
/// The launch, to each entrant, with the other entrants' peer tickets.
pub fn send_relay(w: &World, mut lobby_launched: XMPP_LobbyLaunchedType, crypto_tickets: &XMPP_CryptoTicketsType) {
    let entrants = lobby_launched.entrants.as_ref().map(|e| e.lobby_entrant_info.clone()).unwrap_or_default();
    for lobby_entrant_info in entrants {
        let persona_id = lobby_entrant_info.persona_id;
        let crypto_tickets_tmp = XMPP_CryptoTicketsType {
            p2_p_crypto_ticket: crypto_tickets
                .p2_p_crypto_ticket
                .iter()
                .filter(|t| t.persona_id != persona_id)
                .cloned()
                .collect(),
        };
        lobby_launched.crypto_tickets = Some(crypto_tickets_tmp);
        let response =
            XMPP_ResponseTypeLobbyLaunched { lobby_invite: Some(lobby_launched.clone()), ..Default::default() };
        w.openfire.send(&response, persona_id, &w.effects);
    }
}

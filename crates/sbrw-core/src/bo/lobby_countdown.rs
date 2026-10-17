//! `LobbyCountdownBO`: a lobby's countdown, and its launch when it runs out.

use sbrw_common::RelaySession;
use sbrw_java::util::base64_encode;
use sbrw_xml::dto::http::{Entrants, LobbyEntrantInfo, LobbyEntrantState};
use sbrw_xml::dto::xmpp::{
    ChallengeType, XMPP_CryptoTicketsType, XMPP_EventSessionType, XMPP_LobbyLaunchedType, XMPP_P2PCryptoTicketType,
};
use serde_json::json;

use crate::bo::{lobby_messaging, token_session};
use crate::dao::event_session::{self, EventSessionEntity};
use crate::dao::lobby::{self, LobbyEntity};
use crate::dao::persona;
use crate::timers::TimerKind;
use crate::{Fault, World, datetime};

// port: bo/LobbyCountdownBO#scheduleLobbyStart(LobbyEntity)
/// A timer for the event's `lobbyCountdownTime`.
pub fn schedule_lobby_start(w: &mut World, lobby_entity: &LobbyEntity) {
    let delay = i64::from(lobby_entity.event.lobby_countdown_time);
    w.effects.timer("LobbyCountdownBO", delay, json!(lobby_entity.id));
    let due = datetime::current_time_millis(&w.env) + delay;
    w.timers.schedule(due, TimerKind::LobbyStart { lobby_id: lobby_entity.id });
}

/// The ticket a racer gives the race relay: their grid place, a fixed
/// marker, the event session, how many race, and who they are, in 48 bytes.
pub fn relay_ticket(grid_index: u8, event_session_id: i64, num_of_racers: u8, persona_id: i64) -> [u8; 48] {
    let mut ticket = [0u8; 48];
    ticket[0] = grid_index;
    ticket[1..5].copy_from_slice(&[10, 11, 12, 13]);
    ticket[5..9].copy_from_slice(&(event_session_id as i32).to_be_bytes());
    ticket[9] = num_of_racers;
    ticket[10..14].copy_from_slice(&(persona_id as i32).to_be_bytes());
    ticket
}

// port: bo/LobbyCountdownBO#onTimeout(Timer)
/// The countdown ran out: with two or more entrants, an event session for
/// the lobby, a relay ticket in each entrant's session, and the launch to
/// each with the others' peer tickets. The grid is by persona id, highest
/// first. sbrw-rs also tells its race relay whom to expect.
pub fn on_timeout(w: &mut World, lobby_id: i64) -> Result<(), Fault> {
    let lobby_entity = lobby::find(w.db(), lobby_id)?.ok_or_else(|| Fault::null_pointer("LobbyEntity"))?;
    let mut entrants = lobby_entity.entrants.clone();
    if entrants.len() < 2 {
        return Ok(());
    }
    entrants.sort_by_key(|e| std::cmp::Reverse(e.persona_id));

    let num_of_racers = entrants.len() as u8;
    let mut event_session_entity = EventSessionEntity {
        started: Some(datetime::current_time_millis(&w.env)),
        event: lobby_entity.event.clone(),
        lobby: Some(lobby_entity.clone()),
        ..Default::default()
    };
    event_session::insert(w.db(), &mut event_session_entity)?;
    let udp_race_ip = w.params.get_str_param("UDP_RACE_IP");

    let mut lobby_entrant_info = Vec::new();
    let mut p2p_crypto_ticket = Vec::new();
    let mut relay_session = RelaySession { event_session_id: event_session_entity.id, racers: Vec::new() };
    for (i, lobby_entrant_entity) in entrants.iter().enumerate() {
        let persona_id =
            lobby_entrant_entity.persona_id.ok_or_else(|| Fault::null_pointer("LobbyEntrantEntity.persona"))?;
        let persona_entity = persona::find(w.db(), persona_id)?.ok_or_else(|| Fault::null_pointer("PersonaEntity"))?;
        let grid_index = i as u8;
        let relay_crypto_ticket =
            base64_encode(&relay_ticket(grid_index, event_session_entity.id, num_of_racers, persona_id));
        let user_id = persona_entity.user_id.ok_or_else(|| Fault::null_pointer("PersonaEntity.user"))?;
        let token =
            token_session::find_by_user_id(w, user_id)?.ok_or_else(|| Fault::null_pointer("TokenSessionEntity"))?;
        token_session::set_relay_crypto_ticket(w, &token.security_token, Some(relay_crypto_ticket));

        p2p_crypto_ticket
            .push(XMPP_P2PCryptoTicketType { persona_id, session_key: Some("AAAAAAAAAAAAAAAAAAAAAA==".into()) });

        lobby_entrant_info.push(LobbyEntrantInfo {
            persona_id,
            level: persona_entity.level,
            heat: 1.0,
            grid_index: i as i32,
            state: Some(LobbyEntrantState::UNKNOWN),
        });
        relay_session.racers.push(persona_id);
    }
    let crypto_tickets = XMPP_CryptoTicketsType { p2_p_crypto_ticket: p2p_crypto_ticket };
    let event_session = XMPP_EventSessionType {
        event_id: i64::from(lobby_entity.event.id),
        challenge: Some(ChallengeType {
            challenge_id: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into()),
            pattern: Some("FFFFFFFFFFFFFFFF".into()),
            left_size: 14,
            right_size: 50,
        }),
        session_id: event_session_entity.id,
    };
    let lobby_launched = XMPP_LobbyLaunchedType {
        is_new_relay_server: true,
        lobby_id: lobby_entity.id,
        udp_relay_host: udp_race_ip,
        udp_relay_port: w.params.get_int_param("UDP_RACE_PORT")?,
        entrants: Some(Entrants { lobby_entrant_info }),
        event_session: Some(event_session),
        crypto_tickets: None,
    };

    w.relay.expect(relay_session);
    lobby_messaging::send_relay(w, lobby_launched, &crypto_tickets);
    Ok(())
}

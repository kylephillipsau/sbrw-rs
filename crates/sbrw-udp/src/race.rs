//! The race relay on UDP 9998, written from `docs/protocol/race.md` alone:
//! the server SBRW ran has no license, so nothing here is translated from it.
//!
//! Racers say hello with the ticket the world gave them when their lobby
//! launched, checked against the [`RelayRegistry`]. The relay then answers
//! their sync packets and passes their player packets to the others in the
//! race. [`RaceRelay::handle`] and [`RaceRelay::sweep`] return the datagrams
//! to send; `run_race` does the I/O.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use sbrw_common::RelayRegistry;
use sbrw_common::config::RaceRelayConfig;

/// Datagrams to send, and where.
pub type Datagrams = Vec<(SocketAddr, Vec<u8>)>;

/// The acknowledgement every datagram gets, when it's on.
pub const ACK: u8 = b'1';
/// Where the game would have a CRC, at the end of everything else the relay sends.
const TRAILER: [u8; 4] = [1, 1, 1, 1];
/// The ticket's fixed bytes after the grid index.
const MARKER: [u8; 4] = [0x0a, 0x0b, 0x0c, 0x0d];
/// A hello is read up to its client time, at 69..71.
const HELLO_LEN: usize = 71;
/// A sync reply's body.
const SYNC: [u8; 6] = [0x01, 0x03, 0x00, 0x40, 0xb7, 0xff];
/// A keep-alive reply's body.
const KEEP_ALIVE: [u8; 1] = [0xff];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// How long a race waits for all its racers' hellos; `None` waits forever.
    pub wait_for_all: Option<Duration>,
    /// A racer silent this long no longer holds the others up.
    pub racer_idle: Duration,
    /// A race with no traffic this long is over.
    pub session_idle: Duration,
    /// Answer every datagram with [`ACK`].
    pub legacy_ack_byte: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self::from_config(&RaceRelayConfig::default())
    }
}

impl Settings {
    pub fn from_config(c: &RaceRelayConfig) -> Self {
        Self {
            wait_for_all: (c.wait_for_all_secs > 0).then(|| Duration::from_secs(c.wait_for_all_secs)),
            racer_idle: Duration::from_secs(c.racer_idle_secs),
            session_idle: Duration::from_secs(c.session_idle_secs),
            legacy_ack_byte: c.legacy_ack_byte,
        }
    }
}

/// A relay ticket, as a hello carries it at 4..52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    pub grid_index: u8,
    pub event_session_id: i32,
    pub racers: u8,
    pub persona_id: i32,
}

impl Ticket {
    /// The ticket's fields, if it has the fixed marker.
    pub fn read(ticket: &[u8]) -> Option<Ticket> {
        if ticket.len() < 14 || ticket[1..5] != MARKER {
            return None;
        }
        Some(Ticket {
            grid_index: ticket[0],
            event_session_id: i32::from_be_bytes(ticket[5..9].try_into().ok()?),
            racers: ticket[9],
            persona_id: i32::from_be_bytes(ticket[10..14].try_into().ok()?),
        })
    }
}

/// What a datagram is, from its first bytes and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hello,
    SyncHello,
    Sync,
    KeepAlive,
    /// `meeting` for one shaped like player info: bytes 6..10 all `ff`.
    Player {
        meeting: bool,
    },
    Other,
}

fn kind(packet: &[u8]) -> Kind {
    match packet {
        [0x00, _, _, 0x06, ..] => Kind::Hello,
        [0x00, _, _, 0x07, ..] => match packet.len() {
            26 => Kind::SyncHello,
            22 => Kind::Sync,
            18 => Kind::KeepAlive,
            _ => Kind::Other,
        },
        [0x01, ..] => Kind::Player { meeting: packet.get(6..10) == Some(&[0xff; 4]) },
        _ => Kind::Other,
    }
}

/// A sync reply's mask: all ones but bit 16 − counter, for counters 1 to 16.
pub fn mask(client_counter: u16) -> u16 {
    if (1..=16).contains(&client_counter) { !(1 << (16 - client_counter)) } else { 0xffff }
}

/// Sync start's slot bits: one per player for 2 to 7 players, else `ff`.
pub fn slot_bits(players: u8) -> u8 {
    if (2..=7).contains(&players) { (1u8 << players) - 1 } else { 0xff }
}

/// What a player-info-shaped packet carried.
#[derive(Debug, Default, PartialEq, Eq)]
struct Details {
    player_info: Option<Vec<u8>>,
    car_state: Option<Vec<u8>>,
}

/// The player info (`02`) and car state (`12`) subpackets at the start of a
/// player packet's subpackets: at most four rounds of a `02` then a `12`,
/// each taken if it's next, up to `ff`. `None` if a subpacket runs past the
/// end.
fn read_subpackets(packet: &[u8]) -> Option<Details> {
    let mut details = Details::default();
    let mut at = 10;
    let take = |at: &mut usize| {
        let len = usize::from(*packet.get(*at + 1)?) + 2;
        let sub = packet.get(*at..*at + len)?.to_vec();
        *at += len;
        Some(sub)
    };
    for _ in 0..4 {
        match packet.get(at) {
            None => return None,
            Some(0xff) => break,
            Some(_) => {}
        }
        if packet[at] == 0x02 {
            details.player_info = Some(take(&mut at)?);
        }
        if packet.get(at) == Some(&0x12) {
            details.car_state = Some(take(&mut at)?);
        }
    }
    Some(details)
}

struct Racer {
    addr: SocketAddr,
    /// The client time from the hello, echoed in control packets once the
    /// hello has been answered.
    hello_time: u16,
    echoed_time: u16,
    control_counter: u16,
    relay_counter: u16,
    /// `None` (sent as `ffff`) until the racer's first racing packet.
    second_counter: Option<u16>,
    sync_hello: Option<Vec<u8>>,
    player_index: u8,
    player_info: Option<Vec<u8>>,
    car_state: Option<Vec<u8>>,
    last_heard: Instant,
}

impl Racer {
    fn new(addr: SocketAddr, hello_time: u16, now: Instant) -> Self {
        Racer {
            addr,
            hello_time,
            echoed_time: 0,
            control_counter: 0,
            relay_counter: 0,
            second_counter: None,
            sync_hello: None,
            player_index: 0,
            player_info: None,
            car_state: None,
            last_heard: now,
        }
    }

    fn ready(&self) -> bool {
        self.player_info.is_some() && self.car_state.is_some()
    }

    fn silent(&self, now: Instant, idle: Duration) -> bool {
        now.saturating_duration_since(self.last_heard) >= idle
    }

    fn control(&mut self, kind: u8, tick: u16, payload: &[u8]) -> Vec<u8> {
        self.control_counter = self.control_counter.wrapping_add(1);
        let mut p = vec![0x00];
        p.extend(self.control_counter.to_be_bytes());
        p.push(kind);
        p.extend(tick.to_be_bytes());
        p.extend(self.echoed_time.to_be_bytes());
        p.extend(payload);
        p.extend(TRAILER);
        p
    }

    fn hello_reply(&mut self, tick: u16) -> Vec<u8> {
        self.echoed_time = self.hello_time;
        self.control(0x01, tick, &[])
    }

    fn sync_reply(&mut self, client_counter: u16, tick: u16, body: &[u8]) -> Vec<u8> {
        let mut payload = client_counter.wrapping_add(1).to_be_bytes().to_vec();
        payload.extend(mask(client_counter).to_be_bytes());
        payload.extend(body);
        self.control(0x02, tick, &payload)
    }

    /// Sync start, from the racer's sync hello.
    fn sync_start(&mut self, tick: u16) -> Option<Vec<u8>> {
        let hello = self.sync_hello.as_deref()?;
        let client_counter = u16::from_be_bytes([hello[9], hello[10]]);
        let mut body = vec![0x00, 0x06, 0x00];
        body.extend(&hello[16..20]);
        body.push(slot_bits((hello[20] >> 1) & 0x07));
        body.push(0xff);
        Some(self.sync_reply(client_counter, tick, &body))
    }

    fn relayed(&mut self, payload: &[u8]) -> Vec<u8> {
        self.relay_counter = self.relay_counter.wrapping_add(1);
        let mut p = vec![0x01, self.player_index];
        p.extend(self.relay_counter.to_be_bytes());
        p.extend(self.second_counter.unwrap_or(0xffff).to_be_bytes());
        p.extend([0xff, 0xff]);
        p.extend(payload);
        p.push(0xff);
        p.extend(TRAILER);
        p
    }

    /// The racer's details for the others: its player info until it has
    /// raced, then its car state.
    fn meeting_packet(&mut self) -> Option<Vec<u8>> {
        let payload = match self.second_counter {
            None => self.player_info.clone()?,
            Some(_) => self.car_state.clone()?,
        };
        if let Some(c) = &mut self.second_counter {
            *c = c.wrapping_add(1);
        }
        Some(self.relayed(&payload))
    }

    fn racing_packet(&mut self, payload: &[u8]) -> Vec<u8> {
        self.second_counter = Some(2);
        self.relayed(payload)
    }
}

struct Race {
    /// Racers by grid index, as the registry has them.
    racers: Vec<Option<Racer>>,
    /// The hello replies have gone out: everyone said hello, or the race
    /// stopped waiting.
    started: bool,
    first_hello: Instant,
    /// The last datagram, or the start if that came later: a race doesn't
    /// idle while it waits for everyone.
    last_heard: Instant,
}

impl Race {
    fn present(&self) -> impl Iterator<Item = &Racer> {
        self.racers.iter().flatten()
    }

    /// The racers present and not silent: those the others wait for.
    fn counted(&self, now: Instant, idle: Duration) -> impl Iterator<Item = &Racer> {
        self.present().filter(move |r| !r.silent(now, idle))
    }

    fn full(&self) -> bool {
        self.racers.iter().all(Option::is_some)
    }

    /// Each present racer's packet from `make`, sent to `to(racer)`.
    fn each(&mut self, out: &mut Datagrams, mut make: impl FnMut(&mut Racer) -> Option<Vec<u8>>) {
        for racer in self.racers.iter_mut().flatten() {
            if let Some(datagram) = make(racer) {
                out.push((racer.addr, datagram));
            }
        }
    }

    /// `datagram` to every present racer but the one at `grid`.
    fn to_others(&self, grid: usize, datagram: &[u8], out: &mut Datagrams) {
        for (g, racer) in self.racers.iter().enumerate() {
            if let Some(racer) = racer.as_ref().filter(|_| g != grid) {
                out.push((racer.addr, datagram.to_vec()));
            }
        }
    }
}

pub struct RaceRelay {
    settings: Settings,
    registry: RelayRegistry,
    races: BTreeMap<i64, Race>,
    /// Each racer's race and grid index, by address and port.
    by_addr: HashMap<SocketAddr, (i64, usize)>,
}

impl RaceRelay {
    pub fn new(settings: Settings, registry: RelayRegistry) -> Self {
        RaceRelay { settings, registry, races: BTreeMap::new(), by_addr: HashMap::new() }
    }

    /// How many races are under way.
    pub fn races(&self) -> usize {
        self.races.len()
    }

    /// A datagram from `from`; `tick` is the wall clock in milliseconds, cut
    /// to 16 bits.
    pub fn handle(&mut self, from: SocketAddr, packet: &[u8], tick: u16, now: Instant) -> Datagrams {
        let mut out = Vec::new();
        if self.settings.legacy_ack_byte {
            out.push((from, vec![ACK]));
        }
        let kind = kind(packet);
        if kind == Kind::Hello {
            self.hello(from, packet, tick, now, &mut out);
            return out;
        }
        let Some(&(race_id, grid)) = self.by_addr.get(&from) else { return out };
        let idle = self.settings.racer_idle;
        let race = self.races.get_mut(&race_id).expect("a bound racer's race");
        race.last_heard = now;
        let racer = race.racers[grid].as_mut().expect("a bound racer");
        racer.last_heard = now;
        match kind {
            Kind::SyncHello => {
                racer.sync_hello = Some(packet.to_vec());
                racer.player_index = packet[20] >> 5;
                out.extend(racer.sync_start(tick).map(|d| (from, d)));
                if race.counted(now, idle).all(|r| r.sync_hello.is_some()) {
                    race.each(&mut out, |r| r.sync_start(tick));
                }
            }
            Kind::Sync | Kind::KeepAlive => {
                let client_counter = u16::from_be_bytes([packet[9], packet[10]]);
                let body: &[u8] = if kind == Kind::Sync { &SYNC } else { &KEEP_ALIVE };
                out.push((from, racer.sync_reply(client_counter, tick, body)));
            }
            Kind::Player { meeting } => Self::player(race, grid, packet, meeting, now, idle, &mut out),
            Kind::Hello | Kind::Other => {}
        }
        out
    }

    fn hello(&mut self, from: SocketAddr, packet: &[u8], tick: u16, now: Instant, out: &mut Datagrams) {
        if packet.len() < HELLO_LEN {
            tracing::debug!(%from, len = packet.len(), "race: short hello");
            return;
        }
        let Some(ticket) = Ticket::read(&packet[4..52]) else {
            tracing::warn!(%from, "race: hello without a ticket");
            return;
        };
        let race_id = i64::from(ticket.event_session_id);
        let grid = usize::from(ticket.grid_index);
        let expected = self.registry.session(race_id).filter(|s| {
            s.racers.len() == usize::from(ticket.racers) && s.racers.get(grid) == Some(&i64::from(ticket.persona_id))
        });
        let Some(expected) = expected else {
            tracing::warn!(%from, ?ticket, "race: ticket refused");
            return;
        };
        let hello_time = u16::from_be_bytes([packet[69], packet[70]]);

        // A hello makes a new racer: the address is no longer whoever it was.
        if let Some((old_race, old_grid)) = self.by_addr.remove(&from)
            && let Some(race) = self.races.get_mut(&old_race)
        {
            race.racers[old_grid] = None;
        }
        let race = self.races.entry(race_id).or_insert_with(|| Race {
            racers: expected.racers.iter().map(|_| None).collect(),
            started: false,
            first_hello: now,
            last_heard: now,
        });
        // Rebinding: the persona's old address is forgotten.
        if let Some(old) = race.racers[grid].take() {
            self.by_addr.remove(&old.addr);
        }
        tracing::info!(%from, race = race_id, grid, persona = ticket.persona_id, "race: hello");
        race.racers[grid] = Some(Racer::new(from, hello_time, now));
        race.last_heard = now;
        self.by_addr.insert(from, (race_id, grid));

        if race.full() {
            race.started = true;
            race.each(out, |r| Some(r.hello_reply(tick)));
        } else if race.started {
            let racer = race.racers[grid].as_mut().expect("just added");
            out.push((from, racer.hello_reply(tick)));
        }
    }

    /// Meeting first, for a packet shaped like player info, then racing.
    fn player(
        race: &mut Race,
        grid: usize,
        packet: &[u8],
        meeting: bool,
        now: Instant,
        idle: Duration,
        out: &mut Datagrams,
    ) {
        if meeting {
            let Some(details) = read_subpackets(packet) else { return };
            let racer = race.racers[grid].as_mut().expect("the sender");
            racer.player_info = details.player_info.or(racer.player_info.take());
            racer.car_state = details.car_state.or(racer.car_state.take());
        }
        if !race.counted(now, idle).all(Racer::ready) {
            return;
        }
        if meeting {
            for g in 0..race.racers.len() {
                let Some(datagram) = race.racers[g].as_mut().filter(|r| r.ready()).and_then(Racer::meeting_packet)
                else {
                    continue;
                };
                race.to_others(g, &datagram, out);
            }
        }
        // The subpackets, without the `ff` and five trailer bytes after them.
        let Some(payload) = packet.len().checked_sub(6).and_then(|end| packet.get(10..end)) else { return };
        let datagram = race.racers[grid].as_mut().expect("the sender").racing_packet(payload);
        race.to_others(grid, &datagram, out);
    }

    /// Run about once a second: races that waited long enough for everyone
    /// start with those there, and races gone quiet since they started end.
    /// A race waiting forever ends when the registry forgets it.
    pub fn sweep(&mut self, tick: u16, now: Instant) -> Datagrams {
        let mut out = Vec::new();
        let mut over = Vec::new();
        for (&race_id, race) in &mut self.races {
            if !race.started {
                let waited =
                    self.settings.wait_for_all.is_some_and(|w| now.saturating_duration_since(race.first_hello) >= w);
                if waited {
                    tracing::info!(race = race_id, racers = race.present().count(), "race: starting without everyone");
                    race.started = true;
                    race.last_heard = race.last_heard.max(now);
                    race.each(&mut out, |r| Some(r.hello_reply(tick)));
                } else if self.registry.session(race_id).is_none() {
                    over.push(race_id);
                }
            } else if now.saturating_duration_since(race.last_heard) >= self.settings.session_idle {
                over.push(race_id);
            }
        }
        for race_id in over {
            tracing::info!(race = race_id, "race: over");
            let race = self.races.remove(&race_id).expect("listed");
            for racer in race.present() {
                self.by_addr.remove(&racer.addr);
            }
            self.registry.finish(race_id);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sbrw_common::RelaySession;

    const SESSION: i32 = 7;
    const TICK: u16 = 0x1234;

    fn addr(n: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 40000 + n))
    }

    fn ticket(grid: u8, session: i32, racers: u8, persona: i32) -> [u8; 48] {
        let mut t = [0u8; 48];
        t[0] = grid;
        t[1..5].copy_from_slice(&MARKER);
        t[5..9].copy_from_slice(&session.to_be_bytes());
        t[9] = racers;
        t[10..14].copy_from_slice(&persona.to_be_bytes());
        t
    }

    fn hello_with(ticket: [u8; 48], client_time: u16) -> Vec<u8> {
        let mut h = vec![0u8; 75];
        h[3] = 0x06;
        h[4..52].copy_from_slice(&ticket);
        h[69..71].copy_from_slice(&client_time.to_be_bytes());
        h
    }

    /// Persona 102 is grid 0 and 101 grid 1, as the lobby's launch orders them.
    fn hello(grid: u8) -> Vec<u8> {
        hello_with(ticket(grid, SESSION, 2, 102 - i32::from(grid)), 0x0a00 + u16::from(grid))
    }

    fn sync_hello(counter: u16, player_index: u8, players: u8) -> Vec<u8> {
        let mut p = vec![0u8; 26];
        p[3] = 0x07;
        p[9..11].copy_from_slice(&counter.to_be_bytes());
        p[16..20].copy_from_slice(&0x0102_0304u32.to_be_bytes());
        p[20] = (player_index << 5) | (players << 1);
        p
    }

    fn sync(len: usize, counter: u16) -> Vec<u8> {
        let mut p = vec![0u8; len];
        p[3] = 0x07;
        p[9..11].copy_from_slice(&counter.to_be_bytes());
        p
    }

    /// A player packet: a header, the subpackets, `ff` and five trailer bytes.
    fn player(meeting: bool, subpackets: &[&[u8]]) -> Vec<u8> {
        let mut p = vec![0x01, 0, 0, 9, 0, 0, 0, 3, 0, 0];
        if meeting {
            p[6..10].copy_from_slice(&[0xff; 4]);
        }
        for s in subpackets {
            p.extend(*s);
        }
        p.push(0xff);
        p.extend([9, 9, 9, 9, 9]);
        p
    }

    const INFO_A: &[u8] = &[0x02, 3, b'A', b'A', b'A'];
    const CAR_A: &[u8] = &[0x12, 2, 0xa1, 0xa2];
    const INFO_B: &[u8] = &[0x02, 3, b'B', b'B', b'B'];
    const CAR_B: &[u8] = &[0x12, 2, 0xb1, 0xb2];

    fn relay_with(settings: Settings, racers: Vec<i64>) -> (RaceRelay, RelayRegistry) {
        let registry = RelayRegistry::default();
        registry.expect(RelaySession { event_session_id: i64::from(SESSION), racers });
        (RaceRelay::new(settings, registry.clone()), registry)
    }

    fn relay() -> RaceRelay {
        relay_with(Settings::default(), vec![102, 101]).0
    }

    /// What `handle` sent, less the acknowledgement it starts with.
    fn sent(relay: &mut RaceRelay, from: SocketAddr, packet: &[u8], now: Instant) -> Datagrams {
        let mut out = relay.handle(from, packet, TICK, now);
        assert_eq!(out.remove(0), (from, vec![ACK]), "every datagram is acknowledged first");
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }

    /// Both racers said hello and sent a sync hello.
    fn synced(relay: &mut RaceRelay, t: Instant) {
        sent(relay, addr(1), &hello(0), t);
        sent(relay, addr(2), &hello(1), t);
        sent(relay, addr(1), &sync_hello(1, 0, 2), t);
        sent(relay, addr(2), &sync_hello(1, 1, 2), t);
    }

    #[test]
    fn masks_and_slot_bits() {
        assert_eq!(
            [mask(0), mask(1), mask(2), mask(16), mask(17), mask(0xffff)],
            [0xffff, 0x7fff, 0xbfff, 0xfffe, 0xffff, 0xffff]
        );
        let bits: Vec<u8> = (0..=8).map(slot_bits).collect();
        assert_eq!(bits, [0xff, 0xff, 0x03, 0x07, 0x0f, 0x1f, 0x3f, 0x7f, 0xff]);
    }

    #[test]
    fn hello_replies_go_out_once_everyone_has_said_hello() {
        let mut relay = relay();
        let t = Instant::now();
        assert!(sent(&mut relay, addr(1), &hello(0), t).is_empty(), "racer 101 hasn't said hello");
        let out = sent(&mut relay, addr(2), &hello(1), t);
        let lines: Vec<(SocketAddr, String)> = out.iter().map(|(a, d)| (*a, hex(d))).collect();
        assert_eq!(
            lines,
            [
                (addr(1), "00 00 01 01 12 34 0a 00 01 01 01 01".to_owned()),
                (addr(2), "00 00 01 01 12 34 0a 01 01 01 01 01".to_owned()),
            ]
        );

        // Another hello into a full race: everyone gets another reply.
        let out = sent(&mut relay, addr(2), &hello(1), t);
        let lines: Vec<String> = out.iter().map(|(_, d)| hex(d)).collect();
        assert_eq!(lines, ["00 00 02 01 12 34 0a 00 01 01 01 01", "00 00 01 01 12 34 0a 01 01 01 01 01"]);
    }

    #[test]
    fn tickets_are_checked_against_the_registry() {
        let mut relay = relay();
        let t = Instant::now();
        let refused = [
            ("an unknown session", hello_with(ticket(0, 8, 2, 102), 0)),
            ("another racer's grid place", hello_with(ticket(1, SESSION, 2, 102), 0)),
            ("the wrong number of racers", hello_with(ticket(0, SESSION, 3, 102), 0)),
            ("a grid place past the end", hello_with(ticket(2, SESSION, 2, 102), 0)),
            ("no marker", {
                let mut h = hello(0);
                h[5] = 0;
                h
            }),
            ("a short hello", hello(0)[..70].to_vec()),
        ];
        for (what, packet) in refused {
            assert!(sent(&mut relay, addr(9), &packet, t).is_empty(), "{what}");
            assert!(sent(&mut relay, addr(9), &sync_hello(1, 0, 2), t).is_empty(), "{what}: not a racer");
        }
        assert_eq!(relay.races(), 0);
    }

    #[test]
    fn sync_start_sync_and_keep_alive() {
        let mut relay = relay();
        let t = Instant::now();
        // Before the hello reply the client time isn't known yet.
        sent(&mut relay, addr(1), &hello(0), t);
        let out = sent(&mut relay, addr(1), &sync_hello(1, 0, 2), t);
        let alone = "00 00 01 02 12 34 00 00 00 02 7f ff 00 06 00 01 02 03 04 03 ff 01 01 01 01";
        // Alone in the race, racer 102 is everyone who has said hello, so it gets the round too.
        let lines: Vec<String> = out.iter().map(|(_, d)| hex(d)).collect();
        assert_eq!(lines, [alone, &alone.replacen("00 01 02", "00 02 02", 1)]);

        sent(&mut relay, addr(2), &hello(1), t);
        // Racer 101 hasn't sent a sync hello, so 102's next one is only answered.
        assert_eq!(sent(&mut relay, addr(1), &sync_hello(1, 0, 2), t).len(), 1);
        // Racer 101's sync hello completes the set: it's answered, then both get sync start again.
        let out = sent(&mut relay, addr(2), &sync_hello(3, 1, 2), t);
        let lines: Vec<(SocketAddr, String)> = out.iter().map(|(a, d)| (*a, hex(d))).collect();
        assert_eq!(
            lines,
            [
                (addr(2), "00 00 02 02 12 34 0a 01 00 04 df ff 00 06 00 01 02 03 04 03 ff 01 01 01 01".to_owned()),
                (addr(1), "00 00 05 02 12 34 0a 00 00 02 7f ff 00 06 00 01 02 03 04 03 ff 01 01 01 01".to_owned()),
                (addr(2), "00 00 03 02 12 34 0a 01 00 04 df ff 00 06 00 01 02 03 04 03 ff 01 01 01 01".to_owned()),
            ]
        );

        let out = sent(&mut relay, addr(1), &sync(22, 16), t);
        assert_eq!(hex(&out[0].1), "00 00 06 02 12 34 0a 00 00 11 ff fe 01 03 00 40 b7 ff 01 01 01 01");
        let out = sent(&mut relay, addr(1), &sync(18, 40), t);
        assert_eq!(hex(&out[0].1), "00 00 07 02 12 34 0a 00 00 29 ff ff ff 01 01 01 01");
        assert!(sent(&mut relay, addr(1), &sync(20, 1), t).is_empty(), "a length the relay doesn't know");
    }

    #[test]
    fn racers_meet_then_race() {
        let mut relay = relay();
        let t = Instant::now();
        synced(&mut relay, t);

        // Racer 102 is ready, but 101 isn't yet: nothing is relayed.
        assert!(sent(&mut relay, addr(1), &player(true, &[INFO_A, CAR_A]), t).is_empty());

        // Racer 101's player info, then car state, in separate packets.
        assert!(sent(&mut relay, addr(2), &player(true, &[INFO_B]), t).is_empty());
        let out = sent(&mut relay, addr(2), &player(true, &[CAR_B]), t);
        let lines: Vec<(SocketAddr, String)> = out.iter().map(|(a, d)| (*a, hex(d))).collect();
        assert_eq!(
            lines,
            [
                // Meeting: each sends its player info to the other, in grid order.
                (addr(2), "01 00 00 01 ff ff ff ff 02 03 41 41 41 ff 01 01 01 01".to_owned()),
                (addr(1), "01 01 00 01 ff ff ff ff 02 03 42 42 42 ff 01 01 01 01".to_owned()),
                // Racing: the packet's subpackets as sent.
                (addr(1), "01 01 00 02 00 02 ff ff 12 02 b1 b2 ff 01 01 01 01".to_owned()),
            ]
        );

        // A racing packet from 102, then another meeting round.
        let out = sent(&mut relay, addr(1), &player(false, &[CAR_A, &[0x10, 1, 7]]), t);
        assert_eq!(out.len(), 1);
        assert_eq!(hex(&out[0].1), "01 00 00 02 00 02 ff ff 12 02 a1 a2 10 01 07 ff 01 01 01 01");
        let out = sent(&mut relay, addr(1), &player(true, &[INFO_A, CAR_A]), t);
        let lines: Vec<(SocketAddr, String)> = out.iter().map(|(a, d)| (*a, hex(d))).collect();
        assert_eq!(
            lines,
            [
                // Both have raced, so they send their car states, and the second counter goes up.
                (addr(2), "01 00 00 03 00 03 ff ff 12 02 a1 a2 ff 01 01 01 01".to_owned()),
                (addr(1), "01 01 00 03 00 03 ff ff 12 02 b1 b2 ff 01 01 01 01".to_owned()),
                (addr(2), "01 00 00 04 00 02 ff ff 02 03 41 41 41 12 02 a1 a2 ff 01 01 01 01".to_owned()),
            ]
        );
    }

    #[test]
    fn subpackets_are_read_as_the_notes_say() {
        let read = |subs: &[&[u8]]| read_subpackets(&player(true, subs));
        let details = |info: Option<&[u8]>, car: Option<&[u8]>| {
            Some(Details { player_info: info.map(<[u8]>::to_vec), car_state: car.map(<[u8]>::to_vec) })
        };
        assert_eq!(read(&[CAR_A, INFO_A]), details(Some(INFO_A), Some(CAR_A)));
        assert_eq!(read(&[INFO_A, CAR_A, INFO_B, CAR_B]), details(Some(INFO_B), Some(CAR_B)));
        assert_eq!(read(&[&[0x10, 1, 0], INFO_A, CAR_A]), details(None, None), "another type first");
        assert_eq!(read(&[]), details(None, None));
        // A subpacket of 200 bytes: the length is unsigned.
        let mut long = vec![0x12, 200];
        long.extend([5; 200]);
        assert_eq!(read(&[&long]), details(None, Some(&long)));
        // Running past the end.
        assert_eq!(read_subpackets(&[1, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff, 0x02, 9, 1]), None);
        assert_eq!(read_subpackets(&[1, 0, 0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]), None);
    }

    #[test]
    fn a_race_stops_waiting_for_everyone() {
        let (mut relay, _) = relay_with(Settings::default(), vec![103, 102, 101]);
        let t = Instant::now();
        let hello = |grid: u8| hello_with(ticket(grid, SESSION, 3, 103 - i32::from(grid)), 0x0a00 + u16::from(grid));
        assert!(sent(&mut relay, addr(1), &hello(0), t).is_empty());
        assert!(relay.sweep(TICK, t + Duration::from_secs(59)).is_empty());
        let out = relay.sweep(TICK, t + Duration::from_secs(60));
        assert_eq!(out, [(addr(1), hex_bytes("00 00 01 01 12 34 0a 00 01 01 01 01"))]);
        // Once; and the idle clock starts now, not at the hello.
        assert!(relay.sweep(TICK, t + Duration::from_secs(61)).is_empty());
        assert_eq!(relay.races(), 1);

        // A late racer is answered at once; the last one fills the race, so everyone is.
        let out = sent(&mut relay, addr(2), &hello(1), t + Duration::from_secs(62));
        assert_eq!(out, [(addr(2), hex_bytes("00 00 01 01 12 34 0a 01 01 01 01 01"))]);
        let out = sent(&mut relay, addr(3), &hello(2), t + Duration::from_secs(63));
        let to: Vec<SocketAddr> = out.iter().map(|(a, _)| *a).collect();
        assert_eq!(to, [addr(1), addr(2), addr(3)]);
    }

    #[test]
    fn waiting_forever_as_the_original() {
        let settings = Settings { wait_for_all: None, ..Settings::default() };
        let (mut relay, registry) = relay_with(settings, vec![102, 101]);
        let t = Instant::now();
        sent(&mut relay, addr(1), &hello(0), t);
        for s in [60, 600, 6000] {
            assert!(relay.sweep(TICK, t + Duration::from_secs(s)).is_empty());
            assert_eq!(relay.races(), 1, "a race that hasn't started doesn't idle");
        }
        // Until the registry forgets it.
        registry.finish(i64::from(SESSION));
        relay.sweep(TICK, t + Duration::from_secs(6001));
        assert_eq!(relay.races(), 0);
    }

    fn hex_bytes(text: &str) -> Vec<u8> {
        text.split(' ').map(|b| u8::from_str_radix(b, 16).unwrap()).collect()
    }

    #[test]
    fn a_new_hello_rebinds_the_persona() {
        let mut relay = relay();
        let t = Instant::now();
        sent(&mut relay, addr(1), &hello(0), t);
        // Racer 102 again, from another address and port: it replaces the first.
        let elsewhere = SocketAddr::from(([10, 0, 0, 2], 40001));
        sent(&mut relay, elsewhere, &hello(0), t);
        assert!(sent(&mut relay, addr(1), &sync(18, 1), t).is_empty(), "the old address is forgotten");
        let out = sent(&mut relay, addr(2), &hello(1), t);
        let to: Vec<SocketAddr> = out.iter().map(|(a, _)| *a).collect();
        assert_eq!(to, [elsewhere, addr(2)], "the race didn't grow, so it's full");
        assert_eq!(sent(&mut relay, elsewhere, &sync(18, 1), t).len(), 1);

        // The same port on another address is another racer.
        let same_port = SocketAddr::from(([10, 0, 0, 3], 40002));
        assert!(sent(&mut relay, same_port, &sync(18, 1), t).is_empty());
        assert_eq!(sent(&mut relay, addr(2), &sync(18, 1), t).len(), 1);

        // An address that says hello as someone else is no longer who it was.
        sent(&mut relay, addr(2), &hello(0), t);
        let out = sent(&mut relay, addr(2), &sync(18, 1), t);
        // A new racer: its hello reply, as the race has started, then this.
        assert_eq!(hex(&out[0].1)[..17], *"00 00 02 02 12 34", "a new racer's counters");
        assert!(sent(&mut relay, elsewhere, &sync(18, 1), t).is_empty(), "grid 0's old address is forgotten");
    }

    #[test]
    fn a_silent_racer_stops_holding_the_others_up() {
        let mut relay = relay();
        let t = Instant::now();
        sent(&mut relay, addr(1), &hello(0), t);
        sent(&mut relay, addr(2), &hello(1), t);
        sent(&mut relay, addr(1), &sync_hello(1, 0, 2), t);
        // Racer 101 never gets ready.
        let nine = t + Duration::from_secs(9);
        assert!(sent(&mut relay, addr(1), &player(true, &[INFO_A, CAR_A]), nine).is_empty());
        // Ten seconds on, the others stop waiting for it, but still send to it.
        let ten = t + Duration::from_secs(10);
        let out = sent(&mut relay, addr(1), &player(true, &[INFO_A, CAR_A]), ten);
        let to: Vec<SocketAddr> = out.iter().map(|(a, _)| *a).collect();
        assert_eq!(to, [addr(2), addr(2)], "102 meets 101, then races");

        // Once it speaks it counts again.
        sent(&mut relay, addr(2), &sync(18, 1), ten);
        assert!(sent(&mut relay, addr(1), &player(false, &[CAR_A]), ten).is_empty());
    }

    #[test]
    fn a_quiet_race_ends_and_the_registry_forgets_it() {
        let (mut relay, registry) = relay_with(Settings::default(), vec![102, 101]);
        let t = Instant::now();
        sent(&mut relay, addr(1), &hello(0), t);
        sent(&mut relay, addr(2), &hello(1), t);
        sent(&mut relay, addr(1), &sync(18, 1), t + Duration::from_secs(5));
        relay.sweep(TICK, t + Duration::from_secs(34));
        assert_eq!(relay.races(), 1);
        relay.sweep(TICK, t + Duration::from_secs(35));
        assert_eq!(relay.races(), 0);
        assert!(registry.session(i64::from(SESSION)).is_none());
        assert!(sent(&mut relay, addr(1), &sync(18, 1), t).is_empty());
        assert!(sent(&mut relay, addr(1), &hello(0), t).is_empty(), "the ticket is no longer good");
    }

    #[test]
    fn without_the_acknowledgement_byte() {
        let settings = Settings { legacy_ack_byte: false, ..Settings::default() };
        let (mut relay, _) = relay_with(settings, vec![102, 101]);
        let t = Instant::now();
        assert!(relay.handle(addr(1), &hello(0), TICK, t).is_empty());
        assert_eq!(relay.handle(addr(2), &hello(1), TICK, t).len(), 2);
        assert!(relay.handle(addr(1), b"garbage", TICK, t).is_empty());
    }

    #[test]
    fn any_datagram_is_survived() {
        let mut relay = relay();
        let t = Instant::now();
        synced(&mut relay, t);
        sent(&mut relay, addr(1), &player(true, &[INFO_A, CAR_A]), t);
        sent(&mut relay, addr(2), &player(true, &[INFO_B, CAR_B]), t);
        // Every short shape of every kind, from both racers and a stranger.
        let mut seed = 1u32;
        for len in 0..80 {
            for first in [0x00, 0x01, 0x99, 0x42] {
                for _ in 0..20 {
                    let mut p: Vec<u8> = (0..len)
                        .map(|_| {
                            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                            (seed >> 16) as u8
                        })
                        .collect();
                    if let Some(b) = p.first_mut() {
                        *b = first;
                    }
                    if p.len() > 3 && first == 0x00 {
                        p[3] = [0x06, 0x07][len % 2];
                    }
                    for from in [addr(1), addr(2), addr(9)] {
                        relay.handle(from, &p, TICK, t);
                    }
                }
            }
        }
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from WorldUnitedNFS/freeroam (server.go, client.go, packet.go,
// diff.go, misc.go).

//! The free-roam server: every player sees the nearest others in its slots.
//! Checked against the Go server's replies (`fixtures/vectors/freeroam_*.json`).
//!
//! - A client says hello with a 58-byte packet whose third byte is 0x06,
//!   carrying its tick at bytes 52..54. The server answers with 11 bytes: its
//!   sequence number, 0x01, its own tick, the difference between the two
//!   ticks, and four constant bytes.
//! - Its other packets carry an ack counter at bytes 8..10 and subpackets
//!   (type, length, data) between 16 header bytes and a 5-byte trailer:
//!   channel info (0x00), player info (0x01) and car state (0x12). Once the
//!   client has sent all three, the server answers every packet with its
//!   slots.
//! - The slots hold the nearest ready players within 300 units, nearest
//!   first, or players in the same channel first when the client's channel
//!   info asks for that (its second byte is 1). A player leaving view frees
//!   its slot at once; one coming into view waits in a queue, and one waiting
//!   player takes a free slot every 200 ms.
//! - A slot sends the player's channel, player and car state once ("full"),
//!   then its car state when there is a new one, and `00 ff` when there
//!   isn't. At most three full slots go in one packet. A full slot is sent
//!   again when the client has missed acking it five times.
//! - Ticks are milliseconds truncated to 16 bits, big-endian throughout. A
//!   client silent for 5 seconds is dropped.
//!
//! The Go server decodes each car state to place the car (`carstate`). Its
//! decoder reads missing bits as zeros and never fails, so every car state of
//! two or more bytes is taken.
//!
//! The Go server moves waiting players on a ticker per client, but reads the
//! slots only when that client sends, so here the ticks that have passed are
//! applied when it sends. Go breaks ties between players at the same whole
//! distance in its random map order; here they keep the order they said
//! hello in.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use crate::carstate;

/// `Config.UDP.MaxVisiblePlayers`'s default.
pub const SLOTS: usize = 14;

/// How long a client may be silent before it is dropped (`Client.Active`).
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// `Config.UDP`, with the Go server's defaults.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    /// `VisibilityRadius`, in the game's units.
    pub visibility_radius: f64,
    /// `MaxVisiblePlayers`: the number of slots.
    pub max_visible_players: usize,
    /// `PlayerSpawnDelayMs`: how often a waiting player takes a free slot.
    pub player_spawn_delay: Duration,
    /// `DisableRadiusSync`: see every player, however far.
    pub disable_radius_sync: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            visibility_radius: 300.0,
            max_visible_players: SLOTS,
            player_spawn_delay: Duration::from_millis(200),
            disable_radius_sync: false,
        }
    }
}

impl Settings {
    /// From the `[udp]` config section; a spawn delay of 0 means 200 ms, as
    /// in `newClient`.
    pub fn from_config(udp: &sbrw_common::config::UdpConfig) -> Self {
        let delay = if udp.player_spawn_delay_ms == 0 { 200 } else { udp.player_spawn_delay_ms };
        Settings {
            visibility_radius: udp.visibility_radius,
            max_visible_players: udp.max_visible_players,
            player_spawn_delay: Duration::from_millis(delay),
            disable_radius_sync: udp.disable_radius_sync,
        }
    }
}

/// The client's tick from a hello packet, or `None` if this isn't one
/// (`Server.RunPacketRead`).
pub fn hello_tick(packet: &[u8]) -> Option<u16> {
    (packet.len() == 58 && packet[2] == 0x06).then(|| u16::from_be_bytes([packet[52], packet[53]]))
}

/// `int16(InitialTick - getServerTick())`, as `newClient` keeps it.
pub fn tick_diff(client_tick: u16, server_tick: u16) -> i16 {
    client_tick.wrapping_sub(server_tick) as i16
}

/// `Client.replyHandshake`.
pub fn handshake_reply(seq: u16, server_tick: u16, tick_diff: i16) -> [u8; 11] {
    let mut out = [0; 11];
    out[0..2].copy_from_slice(&seq.to_be_bytes());
    out[2] = 0x01;
    out[3..5].copy_from_slice(&server_tick.to_be_bytes());
    out[5..7].copy_from_slice(&tick_diff.to_be_bytes());
    out[7..11].copy_from_slice(&[0x49, 0x26, 0x03, 0x01]);
    out
}

/// Each hello makes a new client, even from the same address: the Go server's
/// slots point at client objects, so a player who says hello again is gone
/// from everyone's slots and comes back as someone new.
type ClientId = u64;

/// `slotInfo`. `LastUpdateID` is never read in the Go server, so it is left out.
struct Slot {
    client: ClientId,
    update_acked: bool,
    has_sent_full: bool,
    ack_missed: u8,
    packet_sent_seq: u16,
    /// The `pos_recv_td` of the car state last sent.
    last_cp_time: u16,
}

impl Slot {
    fn new(client: ClientId) -> Self {
        Slot { client, update_acked: false, has_sent_full: false, ack_missed: 0, packet_sent_seq: 0, last_cp_time: 0 }
    }
}

struct Client {
    id: ClientId,
    start: Instant,
    seq: u16,
    tick_diff: i16,
    channel: Option<Vec<u8>>,
    channel_name: Vec<u8>,
    social_filtering: bool,
    player: Option<Vec<u8>>,
    name: String,
    car: Option<Vec<u8>>,
    /// The car's x and y (`CarPosPacket.pos`).
    pos: (f64, f64),
    /// When the last car state came, in milliseconds since hello, truncated
    /// to 16 bits (`posRecvTD`). Only compared for equality.
    pos_recv_td: u16,
    slots: Vec<Option<Slot>>,
    /// Players in view waiting for a slot (`pendingPlayerQueue`).
    waiting: Vec<ClientId>,
    /// When this client's spawn ticker fires next.
    next_spawn: Instant,
    last_packet: Instant,
}

impl Client {
    fn new(id: ClientId, tick_diff: i16, settings: &Settings, now: Instant) -> Self {
        Client {
            id,
            start: now,
            seq: 0,
            tick_diff,
            channel: None,
            channel_name: Vec::new(),
            social_filtering: false,
            player: None,
            name: String::new(),
            car: None,
            pos: (0.0, 0.0),
            pos_recv_td: 0,
            slots: (0..settings.max_visible_players).map(|_| None).collect(),
            waiting: Vec::new(),
            next_spawn: now + settings.player_spawn_delay,
            last_packet: now,
        }
    }

    fn next_seq(&mut self) -> u16 {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        seq
    }

    /// `Client.IsReady`.
    fn ready(&self) -> bool {
        self.channel.is_some() && self.player.is_some() && self.car.is_some()
    }

    /// `Client.getTimeDiff`.
    fn time_diff(&self, now: Instant) -> u16 {
        (now.duration_since(self.start).as_secs_f64() * 1000.0) as i64 as u16
    }

    /// The spawn ticker's ticks up to `now` (`startSpawnProcessor`).
    fn catch_up(&mut self, now: Instant, delay: Duration) {
        while self.next_spawn <= now {
            self.spawn_next();
            self.next_spawn += delay;
        }
    }

    /// `processNextPendingPlayer`: the first waiting player takes the first
    /// free slot. It may have left view since it queued; the next packet
    /// frees its slot again.
    fn spawn_next(&mut self) {
        if self.waiting.is_empty() {
            return;
        }
        let Some(free) = self.slots.iter().position(Option::is_none) else { return };
        let client = self.waiting.remove(0);
        self.slots[free] = Some(Slot::new(client));
    }

    /// `Client.processPacket`, from the ack counter on. `None` where the Go
    /// code panics part-way, which its `recover` turns into no answer, with
    /// whatever was taken in before the panic kept.
    fn process(
        &mut self,
        packet: &[u8],
        others: &BTreeMap<ClientId, Client>,
        settings: &Settings,
        tick: u16,
        now: Instant,
    ) -> Option<Vec<u8>> {
        let ack = u16::from_be_bytes([*packet.get(8)?, *packet.get(9)?]);
        for slot in self.slots.iter_mut().flatten() {
            if !slot.update_acked {
                if ack == slot.packet_sent_seq {
                    slot.update_acked = true;
                } else {
                    slot.ack_missed = slot.ack_missed.wrapping_add(1);
                }
            }
        }
        self.take_in(packet, now)?;
        if !self.ready() {
            return None;
        }
        Some(self.send_player_slots(others, settings, tick))
    }

    /// The subpacket loop of `processPacket`.
    fn take_in(&mut self, packet: &[u8], now: Instant) -> Option<()> {
        let mut data = packet.get(16..packet.len().checked_sub(5)?)?;
        while let Some((&kind, rest)) = data.split_first() {
            let (len, rest) = rest.split_first().map_or((0, rest), |(&l, r)| (usize::from(l), r));
            // bytes.Reader.Read fills what it can; the rest stays zero.
            let mut inner = vec![0; len];
            let n = len.min(rest.len());
            inner[..n].copy_from_slice(&rest[..n]);
            data = &rest[n..];
            match kind {
                0x00 => {
                    if let Some(name) = inner.get(2..) {
                        self.channel_name = c_str(name).to_vec();
                        self.social_filtering = inner[1] == 1;
                    }
                    let short = inner.len() < 2;
                    self.channel = Some(inner);
                    if short {
                        return None;
                    }
                }
                0x01 => {
                    if let Some(name) = inner.get(1..33) {
                        self.name = String::from_utf8_lossy(c_str(name)).into_owned();
                    }
                    let short = inner.len() < 33;
                    self.player = Some(inner);
                    if short {
                        return None;
                    }
                }
                0x12 => {
                    // CarPosPacket.Update reads the time before keeping the packet.
                    if inner.len() < 2 {
                        return None;
                    }
                    let state = carstate::decode(&inner);
                    self.pos = (state.pos.x, state.pos.y);
                    self.car = Some(inner);
                    self.pos_recv_td = self.time_diff(now);
                }
                kind => tracing::debug!(kind, len, "free-roam: a subpacket the server doesn't read"),
            }
        }
        Some(())
    }

    /// `getClosestPlayers`: the ready players in view, nearest first by whole
    /// distance, or in this client's channel first when it filters, as many
    /// as there are slots.
    fn closest_players(&self, others: &BTreeMap<ClientId, Client>, settings: &Settings) -> Vec<ClientId> {
        let mut close = Vec::new();
        for other in others.values() {
            if !other.ready() || other.id == self.id {
                continue;
            }
            let (xd, yd) = (self.pos.0 - other.pos.0, self.pos.1 - other.pos.1);
            let distance = (xd * xd + yd * yd).sqrt();
            if !settings.disable_radius_sync && distance > settings.visibility_radius {
                continue;
            }
            close.push((other.id, distance as i64, other.channel_name == self.channel_name));
        }
        if self.social_filtering {
            close.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
        } else {
            close.sort_by_key(|c| c.1);
        }
        close.truncate(self.slots.len());
        close.into_iter().map(|c| c.0).collect()
    }

    /// `recalculateSlots`: players gone from view lose their slot (or their
    /// place in the queue) at once; new ones join the queue.
    fn recalculate_slots(&mut self, others: &BTreeMap<ClientId, Client>, settings: &Settings) {
        let players = self.closest_players(others, settings);
        let old: Vec<ClientId> = self.slots.iter().flatten().map(|s| s.client).collect();
        for gone in old.iter().filter(|id| !players.contains(id)) {
            for slot in &mut self.slots {
                if slot.as_ref().is_some_and(|s| s.client == *gone) {
                    *slot = None;
                }
            }
            // As Go's removeFromPendingQueue; a player in a slot never waits, so
            // this does nothing.
            self.waiting.retain(|id| id != gone);
        }
        for new in players.iter().filter(|id| !old.contains(id)) {
            if !self.waiting.contains(new) {
                self.waiting.push(*new);
            }
        }
    }

    /// `sendPlayerSlots`.
    fn send_player_slots(&mut self, others: &BTreeMap<ClientId, Client>, settings: &Settings, tick: u16) -> Vec<u8> {
        self.recalculate_slots(others, settings);
        let seq = self.next_seq();
        let mut out = Vec::with_capacity(512);
        out.extend_from_slice(&seq.to_be_bytes());
        out.push(0x02);
        out.extend_from_slice(&tick.to_be_bytes());
        out.extend_from_slice(&self.tick_diff.to_be_bytes());
        out.extend_from_slice(&seq.to_be_bytes());
        out.extend_from_slice(&[0xff, 0xff, 0x00]);
        let mut fulls_sent = 0;
        for slot in &mut self.slots {
            // recalculate_slots keeps only players that are in `others` and ready.
            let Some((slot, other)) = slot.as_mut().and_then(|s| others.get(&s.client).map(|o| (s, o))) else {
                out.extend_from_slice(&[0xff, 0xff]);
                continue;
            };
            if slot.has_sent_full && other.pos_recv_td == slot.last_cp_time {
                out.extend_from_slice(&[0x00, 0xff]);
            } else if fulls_sent >= 3 {
                other.write_pos(&mut out);
                slot.last_cp_time = other.pos_recv_td;
            } else if !slot.has_sent_full {
                other.write_full(&mut out);
                slot.has_sent_full = true;
                slot.packet_sent_seq = seq;
                slot.last_cp_time = other.pos_recv_td;
                fulls_sent += 1;
            } else if slot.update_acked || slot.ack_missed < 5 {
                other.write_pos(&mut out);
                slot.last_cp_time = other.pos_recv_td;
            } else {
                other.write_full(&mut out);
                slot.ack_missed = 0;
                slot.packet_sent_seq = seq;
                slot.last_cp_time = other.pos_recv_td;
                fulls_sent += 1;
            }
        }
        out.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
        out
    }

    /// `writeFullPosPacket`.
    fn write_pos(&self, out: &mut Vec<u8>) {
        out.push(0x00);
        subpacket(out, 0x12, self.car.as_deref().unwrap_or_default());
        out.push(0xff);
    }

    /// `writeFullSlotPacket`.
    fn write_full(&self, out: &mut Vec<u8>) {
        out.push(0x00);
        subpacket(out, 0x00, self.channel.as_deref().unwrap_or_default());
        subpacket(out, 0x01, self.player.as_deref().unwrap_or_default());
        subpacket(out, 0x12, self.car.as_deref().unwrap_or_default());
        out.push(0xff);
    }
}

/// `WriteSubpacket`. Subpackets come in with a one-byte length, so they fit.
fn subpacket(out: &mut Vec<u8>, kind: u8, data: &[u8]) {
    out.push(kind);
    out.push(data.len() as u8);
    out.extend_from_slice(data);
}

/// `cStrLen`: up to the first zero byte.
fn c_str(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

/// Every client (`Server.Clients`), in the order they said hello.
#[derive(Default)]
pub struct Freeroam {
    settings: Settings,
    clients: BTreeMap<ClientId, Client>,
    by_addr: HashMap<SocketAddr, ClientId>,
    next_id: ClientId,
}

impl Freeroam {
    pub fn new(settings: Settings) -> Self {
        Freeroam { settings, ..Default::default() }
    }

    /// One packet from `from`, at server tick `tick`; the answer, if any.
    pub fn handle(&mut self, from: SocketAddr, packet: &[u8], tick: u16, now: Instant) -> Option<Vec<u8>> {
        if let Some(client_tick) = hello_tick(packet) {
            let diff = tick_diff(client_tick, tick);
            let id = self.next_id;
            self.next_id += 1;
            if let Some(old) = self.by_addr.insert(from, id) {
                self.clients.remove(&old);
            }
            let mut client = Client::new(id, diff, &self.settings, now);
            let reply = handshake_reply(client.next_seq(), tick, diff).to_vec();
            self.clients.insert(id, client);
            return Some(reply);
        }
        let id = *self.by_addr.get(&from)?;
        let mut client = self.clients.remove(&id)?;
        client.catch_up(now, self.settings.player_spawn_delay);
        client.last_packet = now;
        let reply = client.process(packet, &self.clients, &self.settings, tick, now);
        self.clients.insert(id, client);
        reply
    }

    /// Drop clients silent for `TIMEOUT` (`Server.RunTimer`, every second).
    pub fn sweep(&mut self, now: Instant) {
        self.clients.retain(|_, c| now.duration_since(c.last_packet) < TIMEOUT);
        let clients = &self.clients;
        self.by_addr.retain(|_, id| clients.contains_key(id));
    }

    pub fn len(&self) -> usize {
        self.clients.len()
    }

    /// Whether `from` has said hello and not gone quiet since.
    pub fn contains(&self, from: SocketAddr) -> bool {
        self.by_addr.contains_key(&from)
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    /// The player name a client sent, once it has.
    pub fn name(&self, from: SocketAddr) -> Option<&str> {
        let client = self.clients.get(self.by_addr.get(&from)?)?;
        (!client.name.is_empty()).then_some(client.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Case {
        packet: String,
        reply: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn matches_the_go_server() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/freeroam_handshake.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/handshake_vectors.py");
        let vectors: Vectors = serde_json::from_str(&text).unwrap();
        assert!(vectors.cases.iter().any(|c| c.reply.is_some()) && vectors.cases.iter().any(|c| c.reply.is_none()));
        for case in vectors.cases {
            let packet = bytes(&case.packet);
            let Some(reply) = case.reply.as_deref().map(bytes) else {
                assert_eq!(hello_tick(&packet), None, "freeroamd ignored {}", case.packet);
                continue;
            };
            let client_tick = hello_tick(&packet).expect("freeroamd answered it as a hello");
            let server_tick = u16::from_be_bytes([reply[3], reply[4]]);
            // freeroamd reads its clock once for the difference and again for
            // the reply, so the first read can be a millisecond earlier.
            let same_ms = handshake_reply(0, server_tick, tick_diff(client_tick, server_tick));
            let ms_before = handshake_reply(0, server_tick, tick_diff(client_tick, server_tick.wrapping_sub(1)));
            assert!(reply == same_ms || reply == ms_before, "{} -> {}", case.packet, case.reply.unwrap());
        }
    }

    #[derive(serde::Deserialize)]
    struct Exchange {
        send: String,
        reply: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Scenario {
        name: String,
        exchanges: Vec<Exchange>,
    }

    #[derive(serde::Deserialize)]
    struct Scenarios {
        cases: Vec<Scenario>,
    }

    /// Feed one packet to `server` and compare the answer with Go's. The
    /// tick passed in is the one Go's reply carries; for a hello, the tick
    /// its difference came from, which may be a millisecond earlier.
    fn check(
        server: &mut Freeroam,
        from: SocketAddr,
        now: Instant,
        send: &str,
        reply: Option<&str>,
    ) -> Result<(), String> {
        let packet = bytes(send);
        let go = reply.map(bytes);
        let tick = match (&go, hello_tick(&packet)) {
            (Some(go), Some(client_tick)) => client_tick.wrapping_sub(i16::from_be_bytes([go[5], go[6]]) as u16),
            (Some(go), None) => u16::from_be_bytes([go[3], go[4]]),
            (None, _) => 0,
        };
        let ours = server.handle(from, &packet, tick, now);
        let matches = match (&ours, &go) {
            (Some(ours), Some(go)) if hello_tick(&packet).is_some() => {
                let go_tick = u16::from_be_bytes([go[3], go[4]]);
                ours[..3] == go[..3] && ours[5..] == go[5..] && (go_tick == tick || go_tick == tick.wrapping_add(1))
            }
            (ours, go) => ours == go,
        };
        if matches {
            return Ok(());
        }
        let hex = |b: &Vec<u8>| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
        Err(format!("ours {:?}\n  Go's {:?}", ours.as_ref().map(hex), reply))
    }

    #[test]
    fn answers_a_lone_client_as_the_go_server_does() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/freeroam_slots.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/slots_vectors.py");
        let scenarios: Scenarios = serde_json::from_str(&text).unwrap();
        assert!(scenarios.cases.len() >= 4);
        let from: SocketAddr = "192.0.2.1:30000".parse().unwrap();
        let start = Instant::now();
        for case in scenarios.cases {
            let mut server = Freeroam::default();
            for (i, exchange) in case.exchanges.iter().enumerate() {
                let now = start + Duration::from_millis(i as u64);
                if let Err(err) = check(&mut server, from, now, &exchange.send, exchange.reply.as_deref()) {
                    panic!("{} #{i}: {err}", case.name);
                }
            }
        }
    }

    #[derive(serde::Deserialize)]
    struct TimedExchange {
        at: f64,
        player: u16,
        send: String,
        reply: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct Multi {
        name: String,
        players: Vec<String>,
        exchanges: Vec<TimedExchange>,
    }

    #[derive(serde::Deserialize)]
    struct MultiVectors {
        cases: Vec<Multi>,
    }

    #[test]
    fn answers_several_players_as_the_go_server_does() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/vectors/freeroam_multi.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/multi_vectors.py");
        let vectors: MultiVectors = serde_json::from_str(&text).unwrap();
        assert!(vectors.cases.len() >= 9);
        for case in vectors.cases {
            let mut server = Freeroam::default();
            let start = Instant::now();
            for (i, exchange) in case.exchanges.iter().enumerate() {
                let from = SocketAddr::from(([192, 0, 2, 1], 30000 + exchange.player));
                let now = start + Duration::from_secs_f64(exchange.at / 1000.0);
                // Go sweeps once a second; the scenarios observe nothing in
                // the second when that matters, so sweeping now is the same.
                server.sweep(now);
                if let Err(err) = check(&mut server, from, now, &exchange.send, exchange.reply.as_deref()) {
                    let who = &case.players[usize::from(exchange.player)];
                    panic!("{} #{i} ({who} at {} ms): {err}", case.name, exchange.at);
                }
            }
        }
    }

    /// A real ground state from NFS-World-Offline's capture, at about (5291, 1008).
    const NEAR_CAR: &str = "c7129809e22961a4b349c42589c41ffffc02829696969a28003f";

    /// Header, channel, player and car subpackets, trailer.
    fn ready_packet(car: &[u8]) -> Vec<u8> {
        let mut packet = vec![0u8; 16];
        packet.extend_from_slice(&[0x00, 2, 0, 0, 0x01, 33]);
        packet.extend_from_slice(&[0; 33]);
        packet.extend_from_slice(&[0x12, car.len() as u8]);
        packet.extend_from_slice(car);
        packet.extend_from_slice(&[0; 5]);
        packet
    }

    /// Slots in a reply: `None` for empty, `Some(subpacket types)` otherwise.
    fn slots(reply: &[u8]) -> Vec<Option<Vec<u8>>> {
        let (mut out, mut i) = (Vec::new(), 12);
        while i < reply.len() - 4 {
            if reply[i..i + 2] == [0xff, 0xff] {
                out.push(None);
                i += 2;
                continue;
            }
            i += 1;
            let mut kinds = Vec::new();
            while reply[i] != 0xff {
                kinds.push(reply[i]);
                i += 2 + usize::from(reply[i + 1]);
            }
            i += 1;
            out.push(Some(kinds));
        }
        out
    }

    /// A near B, B far away (a two-byte car state decodes to all zero bits,
    /// about (-15000, 15000)); A's view after `after`.
    fn view_of_a_far_player(settings: Settings, after: Duration) -> Vec<Option<Vec<u8>>> {
        let mut server = Freeroam::new(settings);
        let (a, b) = ("192.0.2.1:1".parse().unwrap(), "192.0.2.1:2".parse().unwrap());
        let start = Instant::now();
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        server.handle(a, &hello, 0, start);
        server.handle(b, &hello, 0, start);
        server.handle(b, &ready_packet(&[0, 0]), 0, start + Duration::from_millis(10)).unwrap();
        server.handle(a, &ready_packet(&bytes(NEAR_CAR)), 0, start + Duration::from_millis(20)).unwrap();
        slots(&server.handle(a, &ready_packet(&bytes(NEAR_CAR)), 0, start + after).unwrap())
    }

    #[test]
    fn a_far_player_is_seen_only_without_radius_sync() {
        let after = Duration::from_millis(300);
        assert!(view_of_a_far_player(Settings::default(), after).iter().all(Option::is_none));
        let everyone = Settings { disable_radius_sync: true, ..Settings::default() };
        let view = view_of_a_far_player(everyone, after);
        assert_eq!(view[0], Some(vec![0x00, 0x01, 0x12]));
        assert!(view[1..].iter().all(Option::is_none));
    }

    #[test]
    fn settings_set_the_slots_and_the_spawn_delay() {
        let settings = Settings {
            max_visible_players: 3,
            player_spawn_delay: Duration::from_millis(500),
            disable_radius_sync: true,
            ..Settings::default()
        };
        let before = view_of_a_far_player(settings.clone(), Duration::from_millis(499));
        assert_eq!(before, [None, None, None]);
        let after = view_of_a_far_player(settings, Duration::from_millis(500));
        assert_eq!(after, [Some(vec![0x00, 0x01, 0x12]), None, None]);
    }

    #[test]
    fn drops_silent_clients() {
        let mut server = Freeroam::default();
        let start = Instant::now();
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        server.handle("192.0.2.1:1".parse().unwrap(), &hello, 0, start);
        server.sweep(start + Duration::from_millis(4999));
        assert_eq!(server.len(), 1);
        server.sweep(start + TIMEOUT);
        assert!(server.is_empty());
    }

    #[test]
    fn recognises_a_hello_by_length_and_type() {
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        hello[52..54].copy_from_slice(&0xBEEFu16.to_be_bytes());
        assert_eq!(hello_tick(&hello), Some(0xBEEF));
        assert_eq!(hello_tick(&hello[..57]), None);
        hello[2] = 0x07;
        assert_eq!(hello_tick(&hello), None);
    }

    #[test]
    fn replies_with_sequence_ticks_and_the_constant_tail() {
        // The client's tick is behind the server's across the 16-bit wrap.
        let diff = tick_diff(0xFFF0, 0x0010);
        assert_eq!(diff, -32);
        assert_eq!(
            handshake_reply(0, 0x0010, diff),
            [0x00, 0x00, 0x01, 0x00, 0x10, 0xFF, 0xE0, 0x49, 0x26, 0x03, 0x01]
        );
    }
}

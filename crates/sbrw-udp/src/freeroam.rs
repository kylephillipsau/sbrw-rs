// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Ported from WorldUnitedNFS/freeroam (server.go, client.go).

//! The free-roam server, so far for a player alone: every slot is empty, and
//! showing other players is M3. Checked against the Go server's replies
//! (`fixtures/vectors/freeroam_*.json`).
//!
//! - A client says hello with a 58-byte packet whose third byte is 0x06,
//!   carrying its tick at bytes 52..54. The server answers with 11 bytes: its
//!   sequence number, 0x01, its own tick, the difference between the two
//!   ticks, and four constant bytes.
//! - Its other packets carry subpackets (type, length, data) between 16 header
//!   bytes and a 5-byte trailer: channel info (0x00), player info (0x01) and
//!   car state (0x12). Once the client has sent all three, the server answers
//!   every packet with the slots it can see.
//! - Ticks are milliseconds truncated to 16 bits, big-endian throughout. A
//!   client silent for 5 seconds is dropped.
//!
//! The Go server decodes each car state to place the car (`carstate`). Its
//! decoder reads missing bits as zeros and never fails, so every car state of
//! two or more bytes is taken.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

/// `Config.UDP.MaxVisiblePlayers`'s default.
pub const SLOTS: usize = 14;

/// How long a client may be silent before it is dropped (`Client.Active`).
pub const TIMEOUT: Duration = Duration::from_secs(5);

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

/// `Client.sendPlayerSlots` with every slot empty.
pub fn empty_slots(seq: u16, server_tick: u16, tick_diff: i16) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + 2 * SLOTS + 4);
    out.extend_from_slice(&seq.to_be_bytes());
    out.push(0x02);
    out.extend_from_slice(&server_tick.to_be_bytes());
    out.extend_from_slice(&tick_diff.to_be_bytes());
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&[0xff, 0xff, 0x00]);
    for _ in 0..SLOTS {
        out.extend_from_slice(&[0xff, 0xff]);
    }
    out.extend_from_slice(&[0x01, 0x01, 0x01, 0x01]);
    out
}

struct Client {
    seq: u16,
    tick_diff: i16,
    channel: Option<Vec<u8>>,
    player: Option<Vec<u8>>,
    car: Option<Vec<u8>>,
    last_packet: Instant,
}

impl Client {
    fn next_seq(&mut self) -> u16 {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        seq
    }

    /// `Client.IsReady`.
    fn ready(&self) -> bool {
        self.channel.is_some() && self.player.is_some() && self.car.is_some()
    }

    /// `Client.processPacket`: take in the subpackets. `None` where the Go
    /// code panics part-way, which its `recover` turns into no answer, with
    /// whatever was taken in before the panic kept.
    fn take_in(&mut self, packet: &[u8]) -> Option<()> {
        // The ack counter at 8..10 matters only for occupied slots.
        packet.get(8..10)?;
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
                    let short = inner.len() < 2;
                    self.channel = Some(inner);
                    if short {
                        return None;
                    }
                }
                0x01 => {
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
                    self.car = Some(inner);
                }
                _ => {}
            }
        }
        Some(())
    }
}

/// Every client, by address (`Server.Clients`).
#[derive(Default)]
pub struct Freeroam {
    clients: HashMap<SocketAddr, Client>,
}

impl Freeroam {
    /// One packet from `from`, at server tick `tick`; the answer, if any.
    pub fn handle(&mut self, from: SocketAddr, packet: &[u8], tick: u16, now: Instant) -> Option<Vec<u8>> {
        if let Some(client_tick) = hello_tick(packet) {
            let diff = tick_diff(client_tick, tick);
            let mut client =
                Client { seq: 0, tick_diff: diff, channel: None, player: None, car: None, last_packet: now };
            let reply = handshake_reply(client.next_seq(), tick, diff).to_vec();
            self.clients.insert(from, client);
            return Some(reply);
        }
        let client = self.clients.get_mut(&from)?;
        client.last_packet = now;
        client.take_in(packet)?;
        if !client.ready() {
            return None;
        }
        let seq = client.next_seq();
        Some(empty_slots(seq, tick, client.tick_diff))
    }

    /// Drop clients silent for `TIMEOUT` (`Server.RunTimer`, every second).
    pub fn sweep(&mut self, now: Instant) {
        self.clients.retain(|_, c| now.duration_since(c.last_packet) < TIMEOUT);
    }

    pub fn len(&self) -> usize {
        self.clients.len()
    }

    pub fn is_empty(&self) -> bool {
        self.clients.is_empty()
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
                let packet = bytes(&exchange.send);
                let go = exchange.reply.as_deref().map(bytes);
                // Tick the Go server's reply carries; for a hello, the tick its
                // difference came from, which may be a millisecond earlier.
                let tick = match (&go, hello_tick(&packet)) {
                    (Some(go), Some(client_tick)) => {
                        client_tick.wrapping_sub(i16::from_be_bytes([go[5], go[6]]) as u16)
                    }
                    (Some(go), None) => u16::from_be_bytes([go[3], go[4]]),
                    (None, _) => 0,
                };
                let ours = server.handle(from, &packet, tick, start + Duration::from_millis(i as u64));
                let matches = match (&ours, &go) {
                    (Some(ours), Some(go)) if hello_tick(&packet).is_some() => {
                        let go_tick = u16::from_be_bytes([go[3], go[4]]);
                        ours[..3] == go[..3]
                            && ours[5..] == go[5..]
                            && (go_tick == tick || go_tick == tick.wrapping_add(1))
                    }
                    (ours, go) => ours == go,
                };
                assert!(
                    matches,
                    "{} #{i}: ours {:?}, Go's {:?}",
                    case.name,
                    ours.as_ref().map(|o| o.len()),
                    exchange.reply
                );
            }
        }
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

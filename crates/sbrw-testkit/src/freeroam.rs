//! A fake free-roam client, and a ghost that drives a real player's captured
//! car states (`fixtures/freeroam/ghost.json`, from
//! `reference/freeroam/ghost_track.py`).
//!
//! The ghost stands in for the roadmap's M3 check with the real game, "a
//! ghost replaying a captured car stream is visible and moving": the
//! `ghost` test has a fake player watch it, and the `ghost` example drives
//! it against a running server for a real client to watch.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

/// A captured player: what it sends to be seen, and its car states with the
/// milliseconds since the previous one.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Track {
    #[serde(with = "hex")]
    pub channel: Vec<u8>,
    #[serde(with = "hex")]
    pub player: Vec<u8>,
    pub cars: Vec<TrackCar>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TrackCar(pub u64, #[serde(with = "hex")] pub Vec<u8>);

impl TrackCar {
    pub fn gap(&self) -> Duration {
        Duration::from_millis(self.0)
    }

    pub fn state(&self) -> &[u8] {
        &self.1
    }
}

impl Track {
    pub fn ghost() -> Track {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/freeroam/ghost.json");
        let text = std::fs::read_to_string(path).expect("run reference/freeroam/ghost_track.py");
        serde_json::from_str(&text).expect("fixtures/freeroam/ghost.json")
    }
}

mod hex {
    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(d)?;
        (0..text.len())
            .step_by(2)
            .map(|i| text.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()))
            .collect::<Option<_>>()
            .ok_or_else(|| serde::de::Error::custom("not hex"))
    }
}

/// A free-roam client: says hello, then sends subpackets and reads the slots
/// the server answers with, acking each answer as the next packet goes out.
pub struct FakeFreeroam {
    socket: UdpSocket,
    last_seq: u16,
}

/// What a slot in the server's answer held.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    Empty,
    /// `00 ff`: the same player as before, with nothing new.
    Unchanged,
    /// The player's subpackets: channel and player info for a full slot,
    /// and the car state.
    Player {
        channel: Option<Vec<u8>>,
        player: Option<Vec<u8>>,
        car: Option<Vec<u8>>,
    },
}

pub const REPLY_TIMEOUT: Duration = Duration::from_millis(500);

impl FakeFreeroam {
    /// Bind a local socket and say hello to `server`.
    pub async fn connect(server: SocketAddr) -> io::Result<FakeFreeroam> {
        let socket = UdpSocket::bind(("127.0.0.1", 0)).await?;
        socket.connect(server).await?;
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        socket.send(&hello).await?;
        let mut client = FakeFreeroam { socket, last_seq: 0 };
        let reply = client.receive().await?.ok_or_else(|| io::Error::other("no answer to hello"))?;
        if reply.len() != 11 || reply[2] != 0x01 {
            return Err(io::Error::other(format!("not a handshake reply: {reply:02x?}")));
        }
        Ok(client)
    }

    /// Send `subpackets` and return the slots the server answers with, or
    /// `None` when it doesn't answer.
    pub async fn send(&mut self, subpackets: &[(u8, &[u8])]) -> io::Result<Option<Vec<Slot>>> {
        let mut packet = vec![0u8; 16];
        packet[8..10].copy_from_slice(&self.last_seq.to_be_bytes());
        for (kind, data) in subpackets {
            packet.push(*kind);
            packet.push(u8::try_from(data.len()).map_err(io::Error::other)?);
            packet.extend_from_slice(data);
        }
        packet.extend_from_slice(&[0; 5]);
        self.socket.send(&packet).await?;
        Ok(self.receive().await?.map(|reply| slots(&reply)))
    }

    /// Channel info, player info and a car state: what makes a player seen.
    pub async fn ready(&mut self, channel: &[u8], player: &[u8], car: &[u8]) -> io::Result<Option<Vec<Slot>>> {
        self.send(&[(0x00, channel), (0x01, player), (0x12, car)]).await
    }

    async fn receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; 4096];
        match tokio::time::timeout(REPLY_TIMEOUT, self.socket.recv(&mut buf)).await {
            Ok(n) => {
                buf.truncate(n?);
                self.last_seq = u16::from_be_bytes([buf[0], buf[1]]);
                Ok(Some(buf))
            }
            Err(_) => Ok(None),
        }
    }
}

/// The slots of a slot packet (type 0x02).
pub fn slots(reply: &[u8]) -> Vec<Slot> {
    let mut out = Vec::new();
    let mut i = 12;
    while i + 4 < reply.len() {
        if reply[i..i + 2] == [0xff, 0xff] {
            out.push(Slot::Empty);
            i += 2;
            continue;
        }
        i += 1;
        let (mut channel, mut player, mut car) = (None, None, None);
        while i < reply.len() && reply[i] != 0xff {
            let (kind, len) = (reply[i], usize::from(reply[i + 1]));
            let data = reply[i + 2..i + 2 + len].to_vec();
            match kind {
                0x00 => channel = Some(data),
                0x01 => player = Some(data),
                0x12 => car = Some(data),
                _ => {}
            }
            i += 2 + len;
        }
        i += 1;
        out.push(if channel.is_none() && player.is_none() && car.is_none() {
            Slot::Unchanged
        } else {
            Slot::Player { channel, player, car }
        });
    }
    out
}

/// Drive `track` once from `client`, after `ready`, with the gaps between
/// car states divided by `speedup`.
pub async fn drive(client: &mut FakeFreeroam, track: &Track, speedup: u32) -> io::Result<()> {
    let (first, rest) = track.cars.split_first().ok_or_else(|| io::Error::other("empty track"))?;
    client.ready(&track.channel, &track.player, first.state()).await?;
    for car in rest {
        tokio::time::sleep(car.gap() / speedup).await;
        client.send(&[(0x12, car.state())]).await?;
    }
    Ok(())
}

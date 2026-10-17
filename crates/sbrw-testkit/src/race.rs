//! A fake racer for the race relay: just what `docs/protocol/race.md` says
//! the game sends, and a reader for what the relay sends back.

use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::UdpSocket;

/// How long to wait for the relay before giving up.
const PATIENCE: Duration = Duration::from_secs(2);

/// What the relay sent, less its acknowledgements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    HelloReply { client_time: u16 },
    SyncStart { session: [u8; 4], slot_bits: u8 },
    Sync,
    KeepAlive,
    Player { player_index: u8, counter: u16, second_counter: u16, payload: Vec<u8> },
    Unknown(Vec<u8>),
}

impl Received {
    pub fn read(d: &[u8]) -> Received {
        let body = d.get(..d.len().saturating_sub(4)).unwrap_or_default();
        match body {
            [0x00, _, _, 0x01, _, _, t0, t1] => Received::HelloReply { client_time: u16::from_be_bytes([*t0, *t1]) },
            [0x00, _, _, 0x02, _, _, _, _, _, _, _, _, 0x00, 0x06, 0x00, s @ .., bits, 0xff] if s.len() == 4 => {
                Received::SyncStart { session: s.try_into().expect("four bytes"), slot_bits: *bits }
            }
            [0x00, _, _, 0x02, _, _, _, _, _, _, _, _, 0x01, 0x03, 0x00, 0x40, 0xb7, 0xff] => Received::Sync,
            [0x00, _, _, 0x02, _, _, _, _, _, _, _, _, 0xff] => Received::KeepAlive,
            [0x01, index, c0, c1, s0, s1, 0xff, 0xff, payload @ .., 0xff] => Received::Player {
                player_index: *index,
                counter: u16::from_be_bytes([*c0, *c1]),
                second_counter: u16::from_be_bytes([*s0, *s1]),
                payload: payload.to_vec(),
            },
            _ => Received::Unknown(d.to_vec()),
        }
    }
}

pub struct FakeRacer {
    socket: UdpSocket,
    /// The grid index from the ticket, sent back as the player index.
    grid_index: u8,
    counter: u16,
}

impl FakeRacer {
    pub async fn connect(relay: SocketAddr) -> io::Result<FakeRacer> {
        let socket = UdpSocket::bind("127.0.0.1:0").await?;
        socket.connect(relay).await?;
        Ok(FakeRacer { socket, grid_index: 0, counter: 0 })
    }

    /// Hello, with the 48-byte ticket at 4 and the client time at 69.
    pub async fn hello(&mut self, ticket: &[u8], client_time: u16) -> io::Result<()> {
        self.grid_index = ticket[0];
        let mut hello = vec![0u8; 75];
        hello[3] = 0x06;
        hello[4..52].copy_from_slice(&ticket[..48]);
        hello[69..71].copy_from_slice(&client_time.to_be_bytes());
        self.socket.send(&hello).await.map(drop)
    }

    /// A sync hello for `players` racers, with the event session from the ticket.
    pub async fn sync_hello(&mut self, ticket: &[u8], players: u8) -> io::Result<()> {
        let mut p = vec![0u8; 26];
        p[3] = 0x07;
        p[9..11].copy_from_slice(&self.next_counter().to_be_bytes());
        p[16..20].copy_from_slice(&ticket[5..9]);
        p[20] = (self.grid_index << 5) | (players << 1);
        self.socket.send(&p).await.map(drop)
    }

    pub async fn keep_alive(&mut self) -> io::Result<()> {
        let mut p = vec![0u8; 18];
        p[3] = 0x07;
        p[9..11].copy_from_slice(&self.next_counter().to_be_bytes());
        self.socket.send(&p).await.map(drop)
    }

    /// A player packet: shaped like player info (`meeting`) or not, with these
    /// subpackets, then `ff` and five trailer bytes.
    pub async fn player(&mut self, meeting: bool, subpackets: &[&[u8]]) -> io::Result<()> {
        let mut p = vec![0x01, self.grid_index, 0, 0, 0, 0, 0, 0, 0, 0];
        p[2..4].copy_from_slice(&self.next_counter().to_be_bytes());
        if meeting {
            p[6..10].copy_from_slice(&[0xff; 4]);
        }
        for s in subpackets {
            p.extend(*s);
        }
        p.push(0xff);
        p.extend([0; 5]);
        self.socket.send(&p).await.map(drop)
    }

    /// The next datagram that isn't an acknowledgement.
    pub async fn receive(&mut self) -> io::Result<Received> {
        let mut buf = [0u8; 2048];
        loop {
            let n = tokio::time::timeout(PATIENCE, self.socket.recv(&mut buf))
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "the relay said nothing"))??;
            if buf[..n] != [sbrw_udp::race::ACK] {
                return Ok(Received::read(&buf[..n]));
            }
        }
    }

    /// Datagrams until one `wanted` picks, skipping the rest.
    pub async fn receive_until<T>(&mut self, mut wanted: impl FnMut(Received) -> Option<T>) -> io::Result<T> {
        loop {
            if let Some(t) = wanted(self.receive().await?) {
                return Ok(t);
            }
        }
    }

    fn next_counter(&mut self) -> u16 {
        self.counter = self.counter.wrapping_add(1);
        self.counter
    }
}

/// Standard base64, as the core encodes the relay ticket.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        acc = (acc << 6) | ALPHABET.iter().position(|&a| a == c)? as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_what_the_core_writes() {
        assert_eq!(base64_decode("AAECAwQ="), Some(vec![0, 1, 2, 3, 4]));
        assert_eq!(base64_decode("AAAAAAAAAAAAAAAAAAAAAA=="), Some(vec![0; 16]));
        assert_eq!(base64_decode("+/8="), Some(vec![0xfb, 0xff]));
        assert_eq!(base64_decode("*"), None);
    }
}

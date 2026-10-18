//! UDP free-roam and race relay servers for NFS World.
//!
//! The free-roam server (`freeroam`, a port of the Go server, with its
//! car-state decoder in `carstate`), the race relay (`race`, written from
//! `docs/protocol/race.md`), and a probe that binds a port and records every
//! datagram without answering, to collect real client packets for the
//! protocol notes.

pub mod carstate;
pub mod freeroam;
pub mod guard;
pub mod race;

use std::collections::HashSet;
use std::io;

use sbrw_common::{CaptureSink, CaptureStream, Direction, RelayRegistry};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

/// The milliseconds clock truncated to 16 bits, as the Go server's
/// `getServerTick`.
fn server_tick() -> u16 {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
    ms as u16
}

/// Serve free-roam on `socket` until shutdown, recording traffic under
/// `udp-freeroam` when capture is on.
pub async fn run_freeroam(
    socket: UdpSocket,
    settings: freeroam::Settings,
    limits: guard::Limits,
    capture: CaptureSink,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let local = socket.local_addr()?;
    let mut server = freeroam::Freeroam::new(settings);
    let mut guard = guard::Guard::new(limits);
    let mut sweep = tokio::time::interval(std::time::Duration::from_secs(1));
    // The Go server's receive buffer: a longer datagram is cut to this.
    let mut buf = vec![0u8; 1024];
    loop {
        let received = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = sweep.tick() => {
                let now = std::time::Instant::now();
                server.sweep(now);
                guard.sweep(now, freeroam::TIMEOUT);
                continue;
            }
            received = socket.recv_from(&mut buf) => received,
        };
        let (n, peer) = match received {
            Ok(received) => received,
            Err(err) => {
                tracing::warn!(%err, "free-roam: receive failed");
                continue;
            }
        };
        capture.udp(CaptureStream::UdpFreeroam, Direction::In, peer, local, &buf[..n]);
        let hello = freeroam::hello_tick(&buf[..n]).is_some();
        let now = std::time::Instant::now();
        if guard.check(peer, hello && !server.contains(peer), server.len(), now).is_err() {
            continue;
        }
        if hello {
            tracing::info!(%peer, "free-roam: new client");
        }
        let named = server.name(peer).is_some();
        let reply = server.handle(peer, &buf[..n], server_tick(), now);
        if let Some(name) = server.name(peer).filter(|_| !named) {
            tracing::info!(%peer, name, "free-roam: player info");
        }
        let Some(reply) = reply else { continue };
        capture.udp(CaptureStream::UdpFreeroam, Direction::Out, local, peer, &reply);
        if let Err(err) = socket.send_to(&reply, peer).await {
            tracing::warn!(%peer, %err, "free-roam: send failed");
        }
    }
}

/// Relay races on `socket` until shutdown, taking the racers `registry`
/// expects and recording traffic under `udp-race` when capture is on.
pub async fn run_race(
    socket: UdpSocket,
    settings: race::Settings,
    registry: RelayRegistry,
    capture: CaptureSink,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let local = socket.local_addr()?;
    let mut relay = race::RaceRelay::new(settings, registry);
    let mut sweep = tokio::time::interval(std::time::Duration::from_secs(1));
    // What Netty reads of a datagram, as the original relay ran.
    let mut buf = vec![0u8; 2048];
    loop {
        let out = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = sweep.tick() => relay.sweep(server_tick(), std::time::Instant::now()),
            received = socket.recv_from(&mut buf) => match received {
                Ok((n, peer)) => {
                    capture.udp(CaptureStream::UdpRace, Direction::In, peer, local, &buf[..n]);
                    relay.handle(peer, &buf[..n], server_tick(), std::time::Instant::now())
                }
                Err(err) => {
                    tracing::warn!(%err, "race: receive failed");
                    continue;
                }
            },
        };
        for (peer, datagram) in out {
            capture.udp(CaptureStream::UdpRace, Direction::Out, local, peer, &datagram);
            if let Err(err) = socket.send_to(&datagram, peer).await {
                tracing::warn!(%peer, %err, "race: send failed");
            }
        }
    }
}

/// Record every datagram arriving on `socket` under `stream`. Logs the first
/// datagram from each peer at info and the rest at debug, since free-roam
/// clients send many packets per second.
pub async fn run_probe(
    stream: CaptureStream,
    socket: UdpSocket,
    capture: CaptureSink,
    shutdown: CancellationToken,
) -> io::Result<()> {
    let local = socket.local_addr()?;
    let mut seen = HashSet::new();
    let mut buf = vec![0u8; 2048];
    loop {
        let received = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            received = socket.recv_from(&mut buf) => received,
        };
        let (n, peer) = match received {
            Ok(received) => received,
            Err(err) => {
                tracing::warn!(server = stream.name(), %err, "udp probe: receive failed");
                continue;
            }
        };
        if seen.insert(peer) {
            tracing::info!(server = stream.name(), %peer, len = n, "udp probe: first datagram from peer (not answering)");
        } else {
            tracing::debug!(server = stream.name(), %peer, len = n, "udp probe: datagram");
        }
        capture.udp(stream, Direction::In, peer, local, &buf[..n]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn freeroam_answers_hello_and_a_ready_client() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let server = tokio::spawn(run_freeroam(
            socket,
            freeroam::Settings::default(),
            guard::Limits::from_config(&sbrw_common::config::UdpConfig::default()),
            CaptureSink::disabled(),
            shutdown.clone(),
        ));

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        client.send_to(&hello, addr).await.unwrap();
        let mut buf = [0u8; 256];
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(n, 11);
        assert_eq!(&buf[..3], [0, 0, 1]);

        let mut packet = vec![0u8; 16];
        packet.extend_from_slice(&[0x00, 2, 0, 0, 0x01, 33]);
        packet.extend_from_slice(&[0; 33]);
        packet.extend_from_slice(&[0x12, 2, 0, 0]);
        packet.extend_from_slice(&[0; 5]);
        client.send_to(&packet, addr).await.unwrap();
        let (n, _) = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf)).await.unwrap().unwrap();
        assert_eq!(n, 12 + 2 * freeroam::SLOTS + 4);
        assert_eq!(&buf[..3], [0, 1, 2]);

        shutdown.cancel();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn freeroam_turns_away_clients_past_the_cap() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let limits = guard::Limits { max_clients: 1, max_packets_per_sec: 0 };
        let server = tokio::spawn(run_freeroam(
            socket,
            freeroam::Settings::default(),
            limits,
            CaptureSink::disabled(),
            shutdown.clone(),
        ));
        let mut hello = [0u8; 58];
        hello[2] = 0x06;
        let mut buf = [0u8; 256];
        let first = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        first.send_to(&hello, addr).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), first.recv_from(&mut buf)).await.unwrap().unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        second.send_to(&hello, addr).await.unwrap();
        let answered = tokio::time::timeout(Duration::from_millis(300), second.recv_from(&mut buf)).await;
        assert!(answered.is_err(), "the server is full");
        first.send_to(&hello, addr).await.unwrap();
        let again = tokio::time::timeout(Duration::from_secs(2), first.recv_from(&mut buf)).await;
        assert!(again.is_ok(), "a client in may say hello again");

        shutdown.cancel();
        server.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn probe_receives_and_never_answers() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = socket.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let server =
            tokio::spawn(run_probe(CaptureStream::UdpFreeroam, socket, CaptureSink::disabled(), shutdown.clone()));

        let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        client.send_to(&[0, 0, 6], addr).await.unwrap();
        let mut buf = [0u8; 16];
        let answered = tokio::time::timeout(Duration::from_millis(200), client.recv_from(&mut buf)).await;
        assert!(answered.is_err(), "the probe must not reply");

        shutdown.cancel();
        server.await.unwrap().unwrap();
    }
}

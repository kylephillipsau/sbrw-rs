//! UDP free-roam and race relay servers for NFS World.
//!
//! So far: the free-roam server for a player alone (`freeroam`, every slot
//! empty), and a probe that binds a port and records every datagram without
//! answering, to collect real client packets for the protocol notes.

pub mod freeroam;

use std::collections::HashSet;
use std::io;

use sbrw_common::{CaptureSink, CaptureStream, Direction};
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
pub async fn run_freeroam(socket: UdpSocket, capture: CaptureSink, shutdown: CancellationToken) -> io::Result<()> {
    let local = socket.local_addr()?;
    let mut server = freeroam::Freeroam::default();
    let mut sweep = tokio::time::interval(std::time::Duration::from_secs(1));
    let mut buf = vec![0u8; 2048];
    loop {
        let received = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            _ = sweep.tick() => {
                server.sweep(std::time::Instant::now());
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
        if freeroam::hello_tick(&buf[..n]).is_some() {
            tracing::info!(%peer, "free-roam: new client");
        }
        let Some(reply) = server.handle(peer, &buf[..n], server_tick(), std::time::Instant::now()) else { continue };
        capture.udp(CaptureStream::UdpFreeroam, Direction::Out, local, peer, &reply);
        if let Err(err) = socket.send_to(&reply, peer).await {
            tracing::warn!(%peer, %err, "free-roam: send failed");
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
        let server = tokio::spawn(run_freeroam(socket, CaptureSink::disabled(), shutdown.clone()));

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

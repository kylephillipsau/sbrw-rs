//! UDP free-roam and race relay servers for NFS World.
//!
//! So far this only has a probe: it binds a port and records every datagram
//! without answering, to collect real client packets for the protocol notes.

pub mod freeroam;

use std::collections::HashSet;
use std::io;

use sbrw_common::{CaptureSink, CaptureStream, Direction};
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

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

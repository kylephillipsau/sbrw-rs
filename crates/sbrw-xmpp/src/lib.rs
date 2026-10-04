//! XMPP server for NFS World chat and server-to-client pushes.
//!
//! So far this only has a probe: it accepts connections and records what the
//! client sends without ever answering, to learn when the game first connects
//! and what its opening stream looks like.

use std::io;
use std::net::SocketAddr;

use sbrw_common::{CaptureSink, Direction};
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

pub async fn run_probe(listener: TcpListener, capture: CaptureSink, shutdown: CancellationToken) -> io::Result<()> {
    let mut next_conn = 0u64;
    loop {
        let accepted = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((stream, peer)) => {
                next_conn += 1;
                tokio::spawn(probe_connection(next_conn, stream, peer, capture.clone(), shutdown.clone()));
            }
            Err(err) => tracing::warn!(%err, "xmpp probe: accept failed"),
        }
    }
}

async fn probe_connection(
    conn: u64,
    mut stream: TcpStream,
    peer: SocketAddr,
    capture: CaptureSink,
    shutdown: CancellationToken,
) {
    tracing::info!(conn, %peer, "xmpp probe: connection opened");
    let mut buf = vec![0; 16 * 1024];
    loop {
        let read = tokio::select! {
            () = shutdown.cancelled() => break,
            read = stream.read(&mut buf) => read,
        };
        let n = match read {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) => {
                tracing::warn!(conn, %peer, %err, "xmpp probe: read failed");
                break;
            }
        };
        let data = &buf[..n];
        tracing::info!(
            conn,
            %peer,
            len = n,
            data = %String::from_utf8_lossy(&data[..n.min(512)]),
            "xmpp probe: received (not answering)"
        );
        capture.xmpp(conn, Direction::In, peer, data);
    }
    tracing::info!(conn, %peer, "xmpp probe: connection closed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn probe_accepts_reads_and_never_answers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let server = tokio::spawn(run_probe(listener, CaptureSink::disabled(), shutdown.clone()));

        let mut client = TcpStream::connect(addr).await.unwrap();
        client.write_all(b"<stream:stream to='127.0.0.1'>").await.unwrap();
        let mut buf = [0u8; 1];
        let answered = tokio::time::timeout(std::time::Duration::from_millis(200), client.read(&mut buf)).await;
        assert!(answered.is_err(), "the probe must not reply");

        shutdown.cancel();
        server.await.unwrap().unwrap();
    }
}

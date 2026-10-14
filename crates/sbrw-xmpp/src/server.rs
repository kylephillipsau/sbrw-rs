//! The listener on port 5222: each connection runs its stream in plaintext
//! until STARTTLS, then over TLS; stanzas its stream doesn't answer go
//! through the hub, and the hub's deliveries go to the connections they are
//! for.

use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use openssl::ssl::{Ssl, SslAcceptor};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::framer::Framer;
use crate::hub::{Delivery, Hub};
use crate::stream::{Accounts, FullJid, Out, Stream};

const CONFLICT: &str = r#"<stream:error xmlns:stream="http://etherx.jabber.org/streams"><conflict xmlns="urn:ietf:params:xml:ns:xmpp-streams"/></stream:error></stream:stream>"#;

enum ToClient {
    Stanza(String),
    /// A newer login took this address.
    Conflict,
}

pub struct Server {
    domain: String,
    acceptor: SslAcceptor,
    accounts: Arc<dyn Accounts + Send + Sync>,
    hub: Mutex<Hub>,
    /// Each logged-in connection, by full JID, with a number telling a
    /// replaced connection from its replacement.
    connections: Mutex<HashMap<String, (u64, mpsc::UnboundedSender<ToClient>)>>,
}

impl Server {
    pub fn new(domain: &str, acceptor: SslAcceptor, accounts: Arc<dyn Accounts + Send + Sync>, hub: Hub) -> Self {
        Server {
            domain: domain.to_owned(),
            acceptor,
            accounts,
            hub: Mutex::new(hub),
            connections: Mutex::new(HashMap::new()),
        }
    }

    /// Run `f` on the hub and send what it delivers.
    pub fn with_hub(&self, f: impl FnOnce(&mut Hub) -> Vec<Delivery>) {
        let deliveries = f(&mut self.hub.lock().expect("hub lock"));
        self.dispatch(deliveries);
    }

    fn dispatch(&self, deliveries: Vec<Delivery>) {
        let connections = self.connections.lock().expect("connections lock");
        for d in deliveries {
            if let Some((_, tx)) = connections.get(&d.to) {
                let _ = tx.send(ToClient::Stanza(d.stanza));
            }
        }
    }
}

pub async fn run(listener: TcpListener, server: Arc<Server>, shutdown: CancellationToken) -> io::Result<()> {
    let mut next = 0u64;
    loop {
        let accepted = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            accepted = listener.accept() => accepted,
        };
        match accepted {
            Ok((tcp, peer)) => {
                next += 1;
                let (server, shutdown) = (server.clone(), shutdown.clone());
                tokio::spawn(async move {
                    if let Err(err) = connection(next, tcp, server, shutdown).await {
                        tracing::info!(conn = next, %peer, %err, "xmpp: connection ended");
                    }
                });
            }
            Err(err) => tracing::warn!(%err, "xmpp: accept failed"),
        }
    }
}

/// Openfire's stream ids are ten lowercase letters and digits.
fn stream_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_owned()
}

async fn connection(conn: u64, mut tcp: TcpStream, server: Arc<Server>, shutdown: CancellationToken) -> io::Result<()> {
    let mut stream = Stream::new(&server.domain, &stream_id());
    let mut framer = Framer::default();
    let mut buf = vec![0; 16 * 1024];
    // Plaintext until the client asks for TLS; nothing is routed before login.
    'plain: loop {
        let n = tcp.read(&mut buf).await?;
        if n == 0 {
            return Ok(());
        }
        framer.push(&buf[..n]);
        while let Some(frame) = framer.next_frame().map_err(io::Error::other)? {
            let outs = stream.handle(frame, server.accounts.as_ref()).unwrap_or_default();
            for out in outs {
                match out {
                    Out::Send(s) => tcp.write_all(s.as_bytes()).await?,
                    Out::StartTls => break 'plain,
                    Out::Available => {}
                    Out::Close => return Ok(()),
                }
            }
        }
    }
    stream.tls_started();
    let ssl = Ssl::new(server.acceptor.context()).map_err(io::Error::other)?;
    let mut tls = tokio_openssl::SslStream::new(ssl, tcp).map_err(io::Error::other)?;
    Pin::new(&mut tls).accept().await.map_err(io::Error::other)?;
    let mut framer = Framer::default();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = secure(conn, &mut tls, &mut stream, &mut framer, &server, tx, &mut rx, &shutdown).await;
    if let Some(jid) = stream.jid() {
        let key = jid.to_string();
        let ours = server.connections.lock().expect("connections lock").get(&key).is_some_and(|(c, _)| *c == conn);
        if ours {
            server.connections.lock().expect("connections lock").remove(&key);
            server.with_hub(|hub| hub.disconnect(jid));
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn secure<S: AsyncRead + AsyncWrite + Unpin>(
    conn: u64,
    tls: &mut S,
    stream: &mut Stream,
    framer: &mut Framer,
    server: &Server,
    tx: mpsc::UnboundedSender<ToClient>,
    rx: &mut mpsc::UnboundedReceiver<ToClient>,
    shutdown: &CancellationToken,
) -> io::Result<()> {
    let mut buf = vec![0; 16 * 1024];
    loop {
        let n = tokio::select! {
            () = shutdown.cancelled() => return Ok(()),
            message = rx.recv() => {
                match message {
                    Some(ToClient::Stanza(s)) => tls.write_all(s.as_bytes()).await?,
                    Some(ToClient::Conflict) | None => {
                        tls.write_all(CONFLICT.as_bytes()).await?;
                        return Ok(());
                    }
                }
                continue;
            }
            n = tls.read(&mut buf) => n?,
        };
        if n == 0 {
            return Ok(());
        }
        framer.push(&buf[..n]);
        while let Some(frame) = framer.next_frame().map_err(io::Error::other)? {
            let was_logged_in = stream.jid().is_some();
            match stream.handle(frame, server.accounts.as_ref()) {
                Ok(outs) => {
                    for out in outs {
                        match out {
                            Out::Send(s) => tls.write_all(s.as_bytes()).await?,
                            Out::StartTls => {}
                            Out::Available => {
                                let jid = stream.jid().expect("presence is answered only after login").clone();
                                server.with_hub(|hub| hub.available(&jid));
                            }
                            Out::Close => return Ok(()),
                        }
                    }
                }
                Err(stanza) => {
                    let from: FullJid = stream.jid().expect("only answered stanzas before login").clone();
                    server.with_hub(|hub| hub.route(&from, stanza));
                }
            }
            if !was_logged_in && let Some(jid) = stream.jid() {
                login(conn, jid, server, tx.clone());
            }
        }
    }
}

/// Register a logged-in connection, ending any older one at the same address.
fn login(conn: u64, jid: &FullJid, server: &Server, tx: mpsc::UnboundedSender<ToClient>) {
    let old = server.connections.lock().expect("connections lock").insert(jid.to_string(), (conn, tx));
    let (_, left) = server.hub.lock().expect("hub lock").connect(jid);
    if let Some((_, old)) = old {
        let _ = old.send(ToClient::Conflict);
    }
    server.dispatch(left);
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use openssl::ssl::{SslConnector, SslMethod, SslStream, SslVerifyMode, SslVersion};

    use super::*;
    use crate::tls;

    struct Players;

    impl Accounts for Players {
        fn password(&self, _: &str) -> Option<String> {
            Some("0123456789abcdef".to_owned())
        }
    }

    fn read_until(r: &mut impl Read, end: &str) -> String {
        let mut got = Vec::new();
        let mut buf = [0; 4096];
        while !String::from_utf8_lossy(&got).contains(end) {
            let n = r.read(&mut buf).unwrap();
            assert!(n > 0, "closed before {end}: {}", String::from_utf8_lossy(&got));
            got.extend_from_slice(&buf[..n]);
        }
        String::from_utf8(got).unwrap()
    }

    /// Log in as the game does over TLS 1.0.
    fn login(addr: std::net::SocketAddr, user: &str) -> SslStream<std::net::TcpStream> {
        let open = "<?xml version='1.0' ?><stream:stream to='127.0.0.1' xmlns='jabber:client' xmlns:stream='http://etherx.jabber.org/streams' version='1.0'>";
        let mut tcp = std::net::TcpStream::connect(addr).unwrap();
        // A missing stanza fails the test instead of hanging it.
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
        tcp.write_all(open.as_bytes()).unwrap();
        read_until(&mut tcp, "</stream:features>");
        tcp.write_all(b"<starttls xmlns='urn:ietf:params:xml:ns:xmpp-tls'/>").unwrap();
        read_until(&mut tcp, "<proceed ");
        let mut connector = SslConnector::builder(SslMethod::tls_client()).unwrap();
        connector.set_verify(SslVerifyMode::NONE);
        connector.set_security_level(0);
        connector.set_min_proto_version(Some(SslVersion::TLS1)).unwrap();
        connector.set_max_proto_version(Some(SslVersion::TLS1)).unwrap();
        connector.set_cipher_list(tls::DEFAULT_CIPHERS).unwrap();
        let mut tls = connector.build().configure().unwrap().verify_hostname(false).connect("127.0.0.1", tcp).unwrap();
        tls.write_all(open.as_bytes()).unwrap();
        read_until(&mut tls, "</stream:features>");
        let auth = format!(
            "<iq id='auth2' type='set'><query xmlns='jabber:iq:auth'><username>{user}</username><password>0123456789abcdef</password><resource>EA-Chat</resource></query></iq>"
        );
        tls.write_all(auth.as_bytes()).unwrap();
        read_until(&mut tls, &format!(r#"<iq type="result" id="auth2" to="{user}@127.0.0.1/EA-Chat"/>"#));
        tls.write_all(b"<presence/>").unwrap();
        read_until(&mut tls, &format!(r#"to="{user}@127.0.0.1/EA-Chat"/>"#));
        tls
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn two_players_chat_in_a_room_and_get_pushes() {
        let acceptor = tls::acceptor(&tls::self_signed("127.0.0.1").unwrap(), tls::DEFAULT_CIPHERS).unwrap();
        let mut ids = 0;
        let mut hub = Hub::new(
            "127.0.0.1",
            Box::new(move || {
                ids += 1;
                format!("uuid{ids}")
            }),
            Box::new(time::OffsetDateTime::now_utc),
        );
        hub.create_public_room("channel.EN__1");
        let server = Arc::new(Server::new("127.0.0.1", acceptor, Arc::new(Players), hub));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let shutdown = CancellationToken::new();
        let running = tokio::spawn(run(listener, server.clone(), shutdown.clone()));

        let pusher = server.clone();
        let (a, b) = tokio::task::spawn_blocking(move || {
            let room = "channel.EN__1@conference.127.0.0.1";
            let mut a = login(addr, "sbrw.100");
            a.write_all(
                format!("<presence to='{room}/sbrw.100'><x xmlns='http://jabber.org/protocol/muc'/></presence>")
                    .as_bytes(),
            )
            .unwrap();
            let a_joined = read_until(&mut a, "<subject/></message>");
            let mut b = login(addr, "sbrw.101");
            b.write_all(
                format!("<presence to='{room}/sbrw.101'><x xmlns='http://jabber.org/protocol/muc'/></presence>")
                    .as_bytes(),
            )
            .unwrap();
            let b_joined = read_until(&mut b, "<subject/></message>");
            let a_saw_b = read_until(&mut a, "</presence>");
            a.write_all(format!("<message to='{room}' type='groupchat'><body>hello</body></message>").as_bytes())
                .unwrap();
            let b_heard = read_until(&mut b, "</message>");
            let a_echo = read_until(&mut a, "</message>");
            pusher.with_hub(|hub| hub.push(101, "<response/>"));
            let b_pushed = read_until(&mut b, "</message>");
            let mut a2 = login(addr, "sbrw.100");
            let a_kicked = read_until(&mut a, "</stream:stream>");
            let b_saw_a_go = read_until(&mut b, "</presence>");
            a2.write_all(b"</stream:stream>").unwrap();
            (vec![a_joined, a_saw_b, a_echo, a_kicked], vec![b_joined, b_heard, b_pushed, b_saw_a_go])
        })
        .await
        .unwrap();
        shutdown.cancel();
        running.await.unwrap().unwrap();

        // Stanzas as in fixtures/xmpp/openfire.txt.
        let room = "channel.en__1@conference.127.0.0.1";
        assert!(a[0].starts_with(&format!(r#"<presence to="sbrw.100@127.0.0.1/EA-Chat" from="{room}/sbrw.100"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant"/><status code="110"/></x></presence>"#)), "{}", a[0]);
        assert_eq!(
            a[1],
            format!(
                r#"<presence to="sbrw.100@127.0.0.1/EA-Chat" from="{room}/sbrw.101"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="participant"/></x></presence>"#
            )
        );
        assert_eq!(
            a[2],
            format!(
                r#"<message to="sbrw.100@127.0.0.1/EA-Chat" type="groupchat" from="{room}/sbrw.100"><body>hello</body><stanza-id xmlns="urn:xmpp:sid:0" id="uuid3" by="{room}"/></message>"#
            )
        );
        assert_eq!(a[3], CONFLICT);
        assert!(
            b[0].starts_with(&format!(r#"<presence to="sbrw.101@127.0.0.1/EA-Chat" from="{room}/sbrw.100">"#)),
            "{}",
            b[0]
        );
        assert_eq!(
            b[1],
            format!(
                r#"<message to="sbrw.101@127.0.0.1/EA-Chat" type="groupchat" from="{room}/sbrw.100"><body>hello</body><stanza-id xmlns="urn:xmpp:sid:0" id="uuid3" by="{room}"/></message>"#
            )
        );
        assert_eq!(
            b[2],
            r#"<message to="sbrw.101@127.0.0.1" id="JN_1234567" from="sbrw.engine.engine@127.0.0.1/EA_Chat"><subject>1337733113377331</subject><body>&lt;response/&gt;</body></message>"#
        );
        assert_eq!(
            b[3],
            format!(
                r#"<presence type="unavailable" from="{room}/sbrw.100" to="sbrw.101@127.0.0.1/EA-Chat"><x xmlns="http://jabber.org/protocol/muc#user"><item affiliation="none" role="none"/></x></presence>"#
            )
        );
    }
}

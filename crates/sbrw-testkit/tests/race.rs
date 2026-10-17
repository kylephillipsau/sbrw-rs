//! The roadmap's M6 check, with fake racers in place of the game: two drivers
//! fill a lobby, the countdown launches it, both race through the relay with
//! the tickets the world gave them, and both appear in the results.

use std::path::Path;

use sbrw_common::CaptureSink;
use sbrw_testkit::race::{FakeRacer, Received, base64_decode};
use sbrw_testkit::scenario::{HttpRequest, Scenario};
use sbrw_testkit::server::SbrwServer;
use sbrw_udp::race::Settings;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

/// The relay ticket in a driver's session, from `/crypto/relaycryptoticket`.
async fn ticket(server: &mut SbrwServer, user: u32, persona: i64) -> Vec<u8> {
    let request = HttpRequest {
        method: "GET".into(),
        path: format!("/Engine.svc/crypto/relaycryptoticket/{persona}"),
        query: None,
        headers: Some(
            [("userId", user.to_string()), ("securityToken", format!("00000000-0000-4000-8000-{:012}", 2 * user))]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
        ),
        form: None,
        body: None,
        content_type: None,
        remote_addr: None,
    };
    let body = server.http(&request).await.unwrap().body;
    let encoded = body.split("<CryptoTicket>").nth(1).and_then(|t| t.split('<').next()).expect("a relay ticket");
    base64_decode(encoded).unwrap()
}

#[tokio::test]
async fn two_drivers_race_through_the_relay() {
    // Two drivers fill a drag lobby; the steps up to its launch, then the race's results.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scenarios/lobbies/no_race_again.json");
    let scenario: Scenario = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let launch = scenario.steps.iter().position(|s| s.fire_timers == Some(true)).expect("the launch");
    let (mut server, _) = SbrwServer::boot(&scenario).unwrap();
    for step in &scenario.steps[..=launch] {
        if let Some(http) = &step.http {
            server.http(http).await.unwrap();
        } else if let Some(ms) = step.advance_ms {
            server.advance(ms);
        } else if step.fire_timers == Some(true) {
            assert_eq!(server.fire_timers().await.unwrap(), 1, "the lobby launches");
        } else if let Some(sql) = &step.sql {
            server.sql(sql).await.unwrap();
        }
    }

    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = socket.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let relay = tokio::spawn(sbrw_udp::run_race(
        socket,
        Settings::default(),
        server.relay(),
        CaptureSink::disabled(),
        shutdown.clone(),
    ));

    // Driver 101 has grid 0, as the grid goes by persona id, highest first.
    let tickets = [ticket(&mut server, 1, 100).await, ticket(&mut server, 2, 101).await];
    assert_eq!([tickets[0][0], tickets[1][0]], [1, 0]);
    assert_eq!(tickets[0][1..5], [10, 11, 12, 13]);
    let mut racers = [FakeRacer::connect(relay_addr).await.unwrap(), FakeRacer::connect(relay_addr).await.unwrap()];

    for (racer, ticket) in racers.iter_mut().zip(&tickets) {
        racer.hello(ticket, 0x0a00).await.unwrap();
    }
    for racer in &mut racers {
        assert_eq!(racer.receive().await.unwrap(), Received::HelloReply { client_time: 0x0a00 });
    }

    for (racer, ticket) in racers.iter_mut().zip(&tickets) {
        racer.sync_hello(ticket, 2).await.unwrap();
        let start = racer.receive().await.unwrap();
        assert_eq!(start, Received::SyncStart { session: ticket[5..9].try_into().unwrap(), slot_bits: 0x03 });
        racer.keep_alive().await.unwrap();
        racer.receive_until(|r| (r == Received::KeepAlive).then_some(())).await.unwrap();
    }

    // Each sends its player info and a car state; they meet, then race.
    let info = |n: u8| vec![0x02, 3, b'P', b'0', b'0' + n];
    let car = |n: u8, step: u8| vec![0x12, 3, n, step, 0x5a];
    for (n, racer) in racers.iter_mut().enumerate() {
        racer.player(true, &[&info(n as u8), &car(n as u8, 0)]).await.unwrap();
    }
    for (n, racer) in racers.iter_mut().enumerate() {
        let other = 1 - n as u8;
        let met = racer
            .receive_until(|r| match r {
                Received::Player { second_counter: 0xffff, payload, .. } => Some(payload),
                _ => None,
            })
            .await
            .unwrap();
        assert_eq!(met, info(other), "racer {n} meets the other");
    }
    for step in 1..=5 {
        racers[0].player(false, &[&car(0, step)]).await.unwrap();
        let seen = racers[1]
            .receive_until(|r| match r {
                Received::Player { second_counter: 2, payload, .. } => Some(payload),
                _ => None,
            })
            .await
            .unwrap();
        assert_eq!(seen, car(0, step), "driver 100's car state reaches driver 101");
    }

    shutdown.cancel();
    relay.await.unwrap().unwrap();

    // The race's results: both drivers are in them.
    let mut last = None;
    for step in &scenario.steps[launch + 1..] {
        if let Some(http) = &step.http {
            last = Some(server.http(http).await.unwrap());
        } else if let Some(ms) = step.advance_ms {
            server.advance(ms);
        } else if let Some(sql) = &step.sql {
            server.sql(sql).await.unwrap();
        }
    }
    let results = last.expect("the results").body;
    let results = results.split("<Entrants>").nth(1).expect("entrants");
    for persona in [100, 101] {
        assert!(results.contains(&format!("<PersonaId>{persona}</PersonaId>")), "{persona} in {results}");
    }
}

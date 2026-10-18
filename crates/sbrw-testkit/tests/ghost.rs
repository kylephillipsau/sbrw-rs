//! The roadmap's M3 check, with a fake player in place of the game: a ghost
//! drives a real player's captured car states through the free-roam server,
//! and a player parked where it starts sees it appear and move.

use std::collections::HashSet;
use std::time::Duration;

use sbrw_common::CaptureSink;
use sbrw_testkit::freeroam::{FakeFreeroam, Slot, Track, drive};
use sbrw_udp::freeroam::Settings;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn a_watcher_sees_the_ghost_appear_and_move() {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let server = socket.local_addr().unwrap();
    let shutdown = CancellationToken::new();
    let task = tokio::spawn(sbrw_udp::run_freeroam(
        socket,
        Settings::default(),
        // The default limits, which real traffic must pass.
        sbrw_udp::guard::Limits::from_config(&sbrw_common::config::UdpConfig::default()),
        CaptureSink::disabled(),
        shutdown.clone(),
    ));

    let track = Track::ghost();
    let mut watcher_player = track.player.clone();
    watcher_player[1..33].copy_from_slice(&{
        let mut name = [0u8; 32];
        name[..7].copy_from_slice(b"WATCHER");
        name
    });
    let mut watcher = FakeFreeroam::connect(server).await.unwrap();
    let alone = watcher.ready(&track.channel, &watcher_player, track.cars[0].state()).await.unwrap().unwrap();
    assert!(alone.iter().all(|s| *s == Slot::Empty));

    // The ghost drives at twice the captured pace; the watcher sends as often.
    let mut ghost = FakeFreeroam::connect(server).await.unwrap();
    let driving = tokio::spawn({
        let track = track.clone();
        async move { drive(&mut ghost, &track, 2).await }
    });
    let mut seen = Vec::new();
    while !driving.is_finished() {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let slots = watcher.send(&[]).await.unwrap().expect("the server answers a ready player");
        seen.push(slots[0].clone());
    }
    driving.await.unwrap().unwrap();
    shutdown.cancel();
    task.await.unwrap().unwrap();

    // First the full slot: the ghost's channel, player and a car state from the track.
    let index_of = |car: &[u8]| track.cars.iter().position(|c| c.state() == car);
    let first = seen.iter().position(|s| *s != Slot::Empty).expect("the ghost appears");
    let Slot::Player { channel: Some(channel), player: Some(player), car: Some(car) } = &seen[first] else {
        panic!("the ghost's first slot is full: {:?}", seen[first]);
    };
    assert_eq!((channel, player), (&track.channel, &track.player));
    assert!(index_of(car).is_some());
    assert!(seen[..first].iter().all(|s| *s == Slot::Empty));

    // Then the track's car states, in order, without the ghost leaving the slot.
    let mut cars = vec![car.clone()];
    for slot in &seen[first + 1..] {
        match slot {
            Slot::Unchanged => {}
            Slot::Player { car: Some(car), .. } => cars.push(car.clone()),
            other => panic!("the ghost left the slot: {other:?}"),
        }
    }
    let indexes: Vec<usize> = cars.iter().map(|c| index_of(c).expect("a car state the ghost sent")).collect();
    assert!(indexes.is_sorted(), "out of order: {indexes:?}");
    let distinct: HashSet<_> = indexes.iter().collect();
    assert!(distinct.len() * 4 >= track.cars.len() * 3, "saw {} of {} car states", distinct.len(), track.cars.len());

    // And it moves: the decoded positions trace most of the track's path.
    let points: Vec<(f64, f64)> = cars
        .iter()
        .map(|c| {
            let pos = sbrw_udp::carstate::decode(c).pos;
            (pos.x, pos.y)
        })
        .collect();
    let path: f64 = points.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
    assert!(path > 200.0, "the ghost moved {path:.0} units");
}

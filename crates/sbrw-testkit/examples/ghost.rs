//! Drive the ghost round and round against a running free-roam server, for
//! a real game client to watch:
//!
//!     cargo run -p sbrw-testkit --example ghost -- 127.0.0.1:9999
//!
//! It parks at the start of the track between laps, so a client should look
//! for it there: about where the captured player drove, in Tri-City.

use std::net::SocketAddr;

use sbrw_testkit::freeroam::{FakeFreeroam, Track, drive};

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let server: SocketAddr =
        std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:9999".into()).parse().map_err(std::io::Error::other)?;
    let track = Track::ghost();
    let mut ghost = FakeFreeroam::connect(server).await?;
    println!("ghost connected to {server}; driving {} car states a lap", track.cars.len());
    loop {
        drive(&mut ghost, &track, 1).await?;
    }
}

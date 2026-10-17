//! Replays reference recordings of soapbox-race-core against sbrw-rs.
//!
//! `reference/record.sh` runs each `scenarios/**/*.json` against the original
//! Java core and writes `fixtures/recorded/**/*.json`: every HTTP response, the
//! side effects of each step and the final database. This crate runs the same
//! scenario against sbrw-rs under the same seed, clock and token counter, and
//! reports every difference.
//!
//! `cargo test -p sbrw-testkit --test replay` replays everything. Scenarios the
//! port doesn't reproduce yet are listed in `port/replay-pending.toml`.
//!
//! `freeroam` has a fake free-roam client and a ghost driving a real player's
//! captured car states.

pub mod compare;
pub mod freeroam;
pub mod scenario;
pub mod server;
pub mod snapshot;

mod run;

pub use compare::Mismatch;
pub use run::{Replay, ReplayError, ScenarioFiles, discover, replay};

//! Bit-exact Rust versions of the JDK behaviour soapbox-race-core relies on.
//!
//! The port replays recordings of the original server, so anything whose output
//! feeds game state (random rolls, shuffles) must match the JVM exactly. Each
//! item is tested against vectors produced by a real JVM
//! (`fixtures/vectors/*.json`, written by `reference/run.sh vectors`).

mod random;

pub use random::JavaRandom;

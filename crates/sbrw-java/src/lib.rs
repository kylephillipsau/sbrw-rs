//! Bit-exact Rust versions of the JDK and Java-library behaviour soapbox-race-core
//! relies on.
//!
//! The port replays recordings of the original server, so anything whose output
//! feeds game state (random rolls, shuffles) must match the JVM exactly. Each
//! item is tested against vectors produced by a real JVM
//! (`fixtures/vectors/*.json`, written by `reference/run.sh vectors`).

pub mod lang;
mod random;
pub mod time;
pub mod validator;

pub use random::JavaRandom;

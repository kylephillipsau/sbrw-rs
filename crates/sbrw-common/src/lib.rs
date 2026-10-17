//! Configuration, protocol capture, and the environment (clock, randomness,
//! ids, effect log) shared by every sbrw-rs subsystem.

pub mod capture;
pub mod config;
pub mod effects;
pub mod env;
pub mod relay;

pub use capture::{CaptureSink, CaptureStream, Direction};
pub use config::Config;
pub use effects::{EffectLog, Effects};
pub use env::Env;
pub use relay::{RelayRegistry, RelaySession};

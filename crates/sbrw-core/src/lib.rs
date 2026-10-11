//! The game server's logic: a 1:1 port of soapbox-race-core.
//!
//! The module layout follows upstream's packages: `dao` (entities and their
//! named queries), `bo` (business objects), `vo` (value objects returned to the
//! client). Every ported upstream method carries a `// port: <unit id>` marker
//! (see `tools/port.py`); behaviour is checked against recordings of the
//! original by `sbrw-testkit`.
//!
//! All state lives in a [`World`], owned by one thread and reached through a
//! [`WorldHandle`]. Each call is one SQLite transaction, which is how upstream's
//! container ran each EJB call from the (non-EJB) REST resources.

pub mod bo;
pub mod build_info;
pub mod dao;
pub mod datetime;
pub mod engine;
pub mod openfire;
pub mod params;
pub mod password;
pub mod redis;
pub mod store;
pub mod vo;
pub mod world;

pub use build_info::BuildInfo;
pub use engine::{EngineException, EngineExceptionCode, Fault};
pub use world::{World, WorldHandle, WorldOptions};

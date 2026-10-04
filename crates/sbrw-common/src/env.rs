//! Time, randomness and ids for the game logic.
//!
//! Ported code reads the clock, draws random numbers and makes session tokens
//! only through an [`Env`]. In production that is the system clock, a
//! `java.util.Random` seeded from OS entropy and random v4 UUIDs. Replays of
//! reference recordings fix all three the same way the patched Java does
//! (`reference/harness/.../Determinism.java`), so the port must reproduce
//! recordings exactly.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sbrw_java::JavaRandom;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct Env {
    inner: Arc<Inner>,
}

struct Inner {
    clock: Clock,
    random: Mutex<JavaRandom>,
    ids: Ids,
}

enum Clock {
    System,
    Manual(Mutex<OffsetDateTime>),
}

enum Ids {
    Random,
    /// `00000000-0000-4000-8000-000000000001`, `…002`, ...
    Counter(AtomicU64),
}

impl Env {
    /// The real clock, an entropy-seeded `java.util.Random` and random UUIDs.
    pub fn system() -> Self {
        let seed = Uuid::new_v4().as_u64_pair().0 as i64;
        Self::new(Clock::System, seed, Ids::Random)
    }

    /// A frozen clock starting at `start`, a `java.util.Random` seeded with
    /// `seed`, and counter UUIDs: the reference recorder's conditions.
    pub fn deterministic(seed: i64, start: OffsetDateTime) -> Self {
        Self::new(Clock::Manual(Mutex::new(start)), seed, Ids::Counter(AtomicU64::new(0)))
    }

    fn new(clock: Clock, seed: i64, ids: Ids) -> Self {
        Self { inner: Arc::new(Inner { clock, random: Mutex::new(JavaRandom::new(seed)), ids }) }
    }

    /// The current time in UTC.
    pub fn now(&self) -> OffsetDateTime {
        match &self.inner.clock {
            Clock::System => OffsetDateTime::now_utc(),
            Clock::Manual(now) => *now.lock().expect("poisoned"),
        }
    }

    /// Move a deterministic clock forward. Panics on the system clock.
    pub fn advance(&self, by: Duration) {
        match &self.inner.clock {
            Clock::System => panic!("the system clock cannot be advanced"),
            Clock::Manual(now) => *now.lock().expect("poisoned") += by,
        }
    }

    /// Draw from the shared `java.util.Random`. Upstream draws from one
    /// generator in a fixed order, so every random decision must go through here.
    pub fn random<R>(&self, f: impl FnOnce(&mut JavaRandom) -> R) -> R {
        f(&mut self.inner.random.lock().expect("poisoned"))
    }

    /// What upstream's `UUID.randomUUID()` produces: session tokens and the like.
    pub fn uuid(&self) -> Uuid {
        match &self.inner.ids {
            Ids::Random => Uuid::new_v4(),
            Ids::Counter(n) => {
                let n = n.fetch_add(1, Ordering::Relaxed) + 1;
                Uuid::from_u64_pair(0x4000, 0x8000_0000_0000_0000 | n)
            }
        }
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::system()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn deterministic_env_matches_the_reference_recorder() {
        let env = Env::deterministic(1, datetime!(2026-01-01 12:00 UTC));
        // Same strings the patched Java produced for the first two session tokens.
        assert_eq!(env.uuid().to_string(), "00000000-0000-4000-8000-000000000001");
        assert_eq!(env.uuid().to_string(), "00000000-0000-4000-8000-000000000002");
        assert_eq!(env.now(), datetime!(2026-01-01 12:00 UTC));
        env.advance(Duration::milliseconds(1500));
        assert_eq!(env.now(), datetime!(2026-01-01 12:00:01.5 UTC));
        assert_eq!(env.random(|r| r.next_int()), JavaRandom::new(1).next_int());
    }

    #[test]
    fn clones_share_state() {
        let env = Env::deterministic(1, datetime!(2026-01-01 0:00 UTC));
        let other = env.clone();
        other.uuid();
        other.advance(Duration::SECOND);
        assert_eq!(env.uuid().to_string(), "00000000-0000-4000-8000-000000000002");
        assert_eq!(env.now(), datetime!(2026-01-01 0:00:01 UTC));
    }

    #[test]
    fn system_env_uses_random_v4_uuids() {
        let env = Env::system();
        let (a, b) = (env.uuid(), env.uuid());
        assert_ne!(a, b);
        assert_eq!(a.get_version_num(), 4);
    }
}

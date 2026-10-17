//! The race relay's guest list: the event sessions a lobby launched, and
//! which persona has which place on each grid. The world writes it when a
//! lobby launches; the race relay checks each racer's ticket against it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a launched session waits for its racers before it's forgotten.
const EXPECTED_FOR: Duration = Duration::from_secs(60 * 60);

/// A launched event session's racers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySession {
    pub event_session_id: i64,
    /// Persona ids by grid index.
    pub racers: Vec<i64>,
}

/// Shared between the world and the race relay. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct RelayRegistry(Arc<Mutex<HashMap<i64, (RelaySession, Instant)>>>);

impl RelayRegistry {
    /// Expect a launched session's racers. Sessions launched more than an
    /// hour ago are forgotten.
    pub fn expect(&self, session: RelaySession) {
        let now = Instant::now();
        let mut sessions = self.0.lock().expect("relay registry lock");
        sessions.retain(|_, (_, at)| now.duration_since(*at) < EXPECTED_FOR);
        sessions.insert(session.event_session_id, (session, now));
    }

    /// The session, if it was launched and is still expected.
    pub fn session(&self, event_session_id: i64) -> Option<RelaySession> {
        self.0.lock().expect("relay registry lock").get(&event_session_id).map(|(s, _)| s.clone())
    }

    /// The relay is done with the session.
    pub fn finish(&self, event_session_id: i64) {
        self.0.lock().expect("relay registry lock").remove(&event_session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_are_expected_until_finished() {
        let registry = RelayRegistry::default();
        let shared = registry.clone();
        registry.expect(RelaySession { event_session_id: 7, racers: vec![102, 101] });
        assert_eq!(shared.session(7).unwrap().racers, [102, 101]);
        assert!(shared.session(8).is_none());
        shared.finish(7);
        assert!(registry.session(7).is_none());
    }
}

//! Upstream's single-action EJB timers (`TimerService.createSingleActionTimer`):
//! work due at a time, kept in memory as the container kept non-persistent
//! timers.

/// What a timer does when it fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerKind {
    /// `DNFTimerBO`: tell a racer their time ran out.
    Dnf { event_session_id: i64, persona_id: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingTimer {
    due_millis: i64,
    /// Creation order, which breaks ties between timers due together.
    seq: u64,
    kind: TimerKind,
}

/// The timers waiting to fire.
#[derive(Debug, Default)]
pub struct Timers {
    pending: Vec<PendingTimer>,
    next_seq: u64,
}

impl Timers {
    pub fn schedule(&mut self, due_millis: i64, kind: TimerKind) {
        self.pending.push(PendingTimer { due_millis, seq: self.next_seq, kind });
        self.next_seq += 1;
    }

    /// The earliest timer due by `now_millis`, taken off the list.
    pub fn take_due(&mut self, now_millis: i64) -> Option<TimerKind> {
        let (index, _) = self
            .pending
            .iter()
            .enumerate()
            .filter(|(_, t)| t.due_millis <= now_millis)
            .min_by_key(|(_, t)| (t.due_millis, t.seq))?;
        Some(self.pending.remove(index).kind)
    }

    /// When the next timer is due.
    pub fn next_due_millis(&self) -> Option<i64> {
        self.pending.iter().map(|t| t.due_millis).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dnf(n: i64) -> TimerKind {
        TimerKind::Dnf { event_session_id: n, persona_id: 100 }
    }

    #[test]
    fn fires_earliest_first_then_in_creation_order() {
        let mut timers = Timers::default();
        timers.schedule(2000, dnf(1));
        timers.schedule(1000, dnf(2));
        timers.schedule(1000, dnf(3));
        assert_eq!(timers.next_due_millis(), Some(1000));
        assert_eq!(timers.take_due(999), None);
        assert_eq!(timers.take_due(5000), Some(dnf(2)));
        assert_eq!(timers.take_due(5000), Some(dnf(3)));
        assert_eq!(timers.take_due(1999), None);
        assert_eq!(timers.take_due(2000), Some(dnf(1)));
        assert_eq!(timers.next_due_millis(), None);
    }
}

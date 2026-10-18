//! Limits in front of the free-roam server, which the Go server didn't have:
//! a cap on clients, and a packet rate per sender. Packets within them reach
//! the ported server untouched.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Clients at once; a hello from a new address past this is ignored. 0 is no cap.
    pub max_clients: usize,
    /// Packets a second from one address, with bursts of twice that; the
    /// rest are dropped. 0 is no limit.
    pub max_packets_per_sec: u32,
}

impl Limits {
    pub fn from_config(udp: &sbrw_common::config::UdpConfig) -> Self {
        Limits { max_clients: udp.max_clients, max_packets_per_sec: udp.max_packets_per_sec }
    }
}

/// A token bucket per sender.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct Guard {
    limits: Limits,
    buckets: HashMap<SocketAddr, Bucket>,
    /// When the cap was last logged, so a flood of hellos logs once a minute.
    cap_logged: Option<Instant>,
}

/// Why a packet was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Drop {
    Full,
    TooFast,
}

impl Guard {
    pub fn new(limits: Limits) -> Self {
        Guard { limits, buckets: HashMap::new(), cap_logged: None }
    }

    /// Whether to pass a packet from `from` on: `is_new_client` when it is a
    /// hello from an address the server doesn't have, of which it has
    /// `clients`.
    pub fn check(&mut self, from: SocketAddr, is_new_client: bool, clients: usize, now: Instant) -> Result<(), Drop> {
        let rate = f64::from(self.limits.max_packets_per_sec);
        if rate > 0.0 {
            let burst = 2.0 * rate;
            let bucket = self.buckets.entry(from).or_insert(Bucket { tokens: burst, last: now });
            let elapsed = now.saturating_duration_since(bucket.last).as_secs_f64();
            bucket.tokens = (bucket.tokens + elapsed * rate).min(burst);
            bucket.last = now;
            if bucket.tokens < 1.0 {
                return Err(Drop::TooFast);
            }
            bucket.tokens -= 1.0;
        }
        if is_new_client && self.limits.max_clients > 0 && clients >= self.limits.max_clients {
            if self.cap_logged.is_none_or(|t| now.saturating_duration_since(t) >= Duration::from_secs(60)) {
                self.cap_logged = Some(now);
                tracing::warn!(%from, clients, "free-roam is full (udp.max_clients); ignoring new clients");
            }
            return Err(Drop::Full);
        }
        Ok(())
    }

    /// Forget senders quiet for `idle`, so the buckets don't grow forever.
    pub fn sweep(&mut self, now: Instant, idle: Duration) {
        self.buckets.retain(|_, b| now.saturating_duration_since(b.last) < idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([203, 0, 113, 7], port))
    }

    #[test]
    fn a_sender_gets_a_burst_then_the_rate() {
        let mut guard = Guard::new(Limits { max_clients: 0, max_packets_per_sec: 10 });
        let start = Instant::now();
        let passed = (0..30).filter(|_| guard.check(addr(1), false, 0, start).is_ok()).count();
        assert_eq!(passed, 20, "a burst of twice the rate");
        assert_eq!(guard.check(addr(2), false, 0, start), Ok(()), "senders are counted apart");
        let later = start + Duration::from_millis(500);
        let passed = (0..30).filter(|_| guard.check(addr(1), false, 0, later).is_ok()).count();
        assert_eq!(passed, 5, "half a second refills half the rate");
    }

    #[test]
    fn new_clients_past_the_cap_are_turned_away() {
        let mut guard = Guard::new(Limits { max_clients: 2, max_packets_per_sec: 0 });
        let now = Instant::now();
        assert_eq!(guard.check(addr(1), true, 1, now), Ok(()));
        assert_eq!(guard.check(addr(2), true, 2, now), Err(Drop::Full));
        assert_eq!(guard.check(addr(1), false, 2, now), Ok(()), "clients already in carry on");
        let mut open = Guard::new(Limits { max_clients: 0, max_packets_per_sec: 0 });
        assert_eq!(open.check(addr(3), true, 10_000, now), Ok(()));
    }

    #[test]
    fn quiet_senders_are_forgotten() {
        let mut guard = Guard::new(Limits { max_clients: 0, max_packets_per_sec: 1 });
        let now = Instant::now();
        guard.check(addr(1), false, 0, now).unwrap();
        guard.sweep(now + Duration::from_secs(10), Duration::from_secs(5));
        assert!(guard.buckets.is_empty());
    }
}

//! A limit on failed logins, registrations and password recovery attempts
//! per client address, against guessing passwords, invite tickets and reset
//! keys. The original had none.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct AuthLimiter {
    max: usize,
    window: Duration,
    failures: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl AuthLimiter {
    /// `max` failures in `window`; 0 is no limit.
    pub fn new(max: u32, window: Duration) -> Self {
        AuthLimiter { max: max as usize, window, failures: Mutex::new(HashMap::new()) }
    }

    /// Whether `ip` has failed too often lately to try again yet.
    pub fn blocked(&self, ip: IpAddr, now: Instant) -> bool {
        if self.max == 0 {
            return false;
        }
        let mut failures = self.failures.lock().expect("limiter lock");
        let Some(times) = failures.get_mut(&ip) else { return false };
        while times.front().is_some_and(|&t| now.saturating_duration_since(t) >= self.window) {
            times.pop_front();
        }
        if times.is_empty() {
            failures.remove(&ip);
            return false;
        }
        times.len() >= self.max
    }

    pub fn failed(&self, ip: IpAddr, now: Instant) {
        if self.max == 0 {
            return;
        }
        let mut failures = self.failures.lock().expect("limiter lock");
        // Keep the map small under a spray of addresses: drop those whose
        // failures have all aged out.
        if failures.len() > 10_000 {
            failures.retain(|_, times| times.back().is_some_and(|&t| now.saturating_duration_since(t) < self.window));
        }
        let times = failures.entry(ip).or_default();
        times.push_back(now);
        while times.len() > self.max {
            times.pop_front();
        }
    }

    /// When `ip` may try again, for the message.
    pub fn wait(&self) -> Duration {
        self.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_block_an_address_until_the_oldest_ages_out() {
        let limiter = AuthLimiter::new(3, Duration::from_secs(60));
        let (a, b): (IpAddr, IpAddr) = ("203.0.113.7".parse().unwrap(), "203.0.113.8".parse().unwrap());
        let start = Instant::now();
        for i in 0..3 {
            assert!(!limiter.blocked(a, start), "{i}");
            limiter.failed(a, start + Duration::from_secs(i * 10));
        }
        assert!(limiter.blocked(a, start + Duration::from_secs(30)));
        assert!(!limiter.blocked(b, start + Duration::from_secs(30)), "addresses are counted apart");
        assert!(limiter.blocked(a, start + Duration::from_secs(59)));
        assert!(!limiter.blocked(a, start + Duration::from_secs(60)), "the first failure has aged out");
        assert!(!AuthLimiter::new(0, Duration::from_secs(60)).blocked(a, start));
    }
}

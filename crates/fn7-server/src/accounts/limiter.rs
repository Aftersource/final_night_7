//! Rate limits for the account routes: token buckets in memory, keyed by a class and who is asking.
//!
//! | Class | Who | Per minute | Routes |
//! |---|---|---|---|
//! | `auth` | client address | 10 | `POST /api/auth/guest`, `POST /api/auth/google/start`, `GET /api/auth/google/callback`, `POST /api/game/bind` |
//! | `poll` | client address | 120 | `POST /api/auth/poll` |
//! | `read` | account | 240 | every `GET /api/me…` |
//! | `write` | account | 30 | `PUT /api/me/settings`, logout, delete |
//! | `moderate` | account | 6 | `PUT /api/me/demigods/{hand}` |
//!
//! The client address is the last hop of `X-Forwarded-For` (the one the tunnel in front of the server appends: a
//! client cannot forge it); a request without one shares the bucket `direct`. A refused request is a 429 with `Retry-After` (seconds). `FN7_AUTH_RATE` scales every limit
//! (`2` doubles them, `0` turns limiting off: tests and load runs only).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Class {
    Auth,
    Poll,
    Read,
    Write,
    Moderate,
}

impl Class {
    /// Requests per minute (also the burst).
    pub fn per_minute(self) -> f64 {
        match self {
            Class::Auth => 10.0,
            Class::Poll => 120.0,
            Class::Read => 240.0,
            Class::Write => 30.0,
            Class::Moderate => 6.0,
        }
    }
}

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct Limiter {
    scale: f64,
    buckets: Mutex<HashMap<(Class, String), Bucket>>,
}

/// Buckets kept before the idle ones are dropped.
const MAX_BUCKETS: usize = 20_000;

impl Limiter {
    /// `scale` multiplies every limit; `0` (or less) turns limiting off.
    pub fn new(scale: f64) -> Self {
        Self {
            scale,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Take one request from `who`'s bucket for `class`: `Ok` or the seconds to wait.
    pub fn check(&self, class: Class, who: &str) -> Result<(), u64> {
        self.check_at(class, who, Instant::now())
    }

    pub fn check_at(&self, class: Class, who: &str, now: Instant) -> Result<(), u64> {
        if self.scale <= 0.0 {
            return Ok(());
        }
        let cap = (class.per_minute() * self.scale).max(1.0);
        let rate = cap / 60.0;
        let mut map = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= MAX_BUCKETS {
            // Full buckets are idle clients: forgetting them loses nothing.
            map.retain(|(c, _), b| {
                let cap = (c.per_minute() * self.scale).max(1.0);
                b.tokens + now.saturating_duration_since(b.last).as_secs_f64() * cap / 60.0 < cap
            });
        }
        let who: String = who.chars().take(80).collect();
        let b = map.entry((class, who)).or_insert(Bucket { tokens: cap, last: now });
        b.tokens = (b.tokens + now.saturating_duration_since(b.last).as_secs_f64() * rate).min(cap);
        b.last = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - b.tokens) / rate).ceil().max(1.0) as u64)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_bucket_empties_refills_and_is_per_client() {
        let l = Limiter::new(1.0);
        let t0 = Instant::now();
        for _ in 0..10 {
            assert!(l.check_at(Class::Auth, "1.2.3.4", t0).is_ok());
        }
        let wait = l.check_at(Class::Auth, "1.2.3.4", t0).unwrap_err();
        assert_eq!(wait, 6, "one token every six seconds");
        assert!(l.check_at(Class::Auth, "5.6.7.8", t0).is_ok(), "another client has its own bucket");
        assert!(l.check_at(Class::Poll, "1.2.3.4", t0).is_ok(), "another class has its own bucket");
        assert!(l.check_at(Class::Auth, "1.2.3.4", t0 + Duration::from_secs(5)).is_err());
        assert!(l.check_at(Class::Auth, "1.2.3.4", t0 + Duration::from_secs(7)).is_ok());
        // Scale 0: off.
        let off = Limiter::new(0.0);
        for _ in 0..1000 {
            assert!(off.check(Class::Moderate, "a").is_ok());
        }
    }
}

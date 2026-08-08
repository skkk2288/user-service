//! Email-based login rate limiting.
//!
//! Tracks consecutive failed login attempts per email.  After `max_failures`
//! failures the email is locked for `lockout_minutes`.  A successful login or
//! expiry of the lockout resets the counter.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

/// A single email's failure record.
#[derive(Debug, Clone)]
pub struct RateLimitEntry {
    pub fail_count: u32,
    pub first_fail_at: DateTime<Utc>,
    pub locked_until: Option<DateTime<Utc>>,
}

/// Outcome of a rate-limit check.
#[derive(Debug, PartialEq, Eq)]
pub enum RateLimitStatus {
    /// The email is allowed to attempt a login.
    Allowed,
    /// The email is locked; attempts should be rejected with 429.
    /// Contains the timestamp when the lock expires.
    Locked(DateTime<Utc>),
}

/// In-memory rate limiter keyed by normalized email.
#[derive(Debug)]
pub struct RateLimiter {
    entries: Arc<RwLock<HashMap<String, RateLimitEntry>>>,
    max_failures: u32,
    lockout: Duration,
}

impl RateLimiter {
    pub fn new(max_failures: u32, lockout_minutes: u32) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            max_failures,
            lockout: Duration::from_secs(lockout_minutes as u64 * 60),
        }
    }

    /// Check whether the email is currently locked.
    ///
    /// If a lockout has expired, the entry is lazily reset.
    pub async fn check(&self, email: &str, now: DateTime<Utc>) -> RateLimitStatus {
        let email = email.to_lowercase();
        let mut entries = self.entries.write().await;
        if let Some(entry) = entries.get_mut(&email) {
            if let Some(until) = entry.locked_until {
                if now < until {
                    return RateLimitStatus::Locked(until);
                }
                // Lockout expired — reset.
                entries.remove(&email);
            }
        }
        RateLimitStatus::Allowed
    }

    /// Record a failed attempt.  If the threshold is reached, set the lockout.
    pub async fn record_fail(&self, email: &str, now: DateTime<Utc>) {
        let email = email.to_lowercase();
        let mut entries = self.entries.write().await;
        let entry = entries.entry(email).or_insert(RateLimitEntry {
            fail_count: 0,
            first_fail_at: now,
            locked_until: None,
        });

        // If a previous lockout expired but wasn't cleaned by check(), reset.
        if let Some(until) = entry.locked_until {
            if now >= until {
                entry.fail_count = 0;
                entry.first_fail_at = now;
                entry.locked_until = None;
            } else {
                // Still locked; don't increment further.
                return;
            }
        }

        entry.fail_count += 1;
        if entry.fail_count >= self.max_failures {
            entry.locked_until =
                Some(now + chrono::Duration::seconds(self.lockout.as_secs() as i64));
        }
    }

    /// Reset the counter for an email (called on successful login).
    pub async fn reset(&self, email: &str) {
        let email = email.to_lowercase();
        let mut entries = self.entries.write().await;
        entries.remove(&email);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_limiter() -> RateLimiter {
        RateLimiter::new(5, 15)
    }

    #[tokio::test]
    async fn allowed_until_threshold() {
        let rl = make_limiter();
        let now = Utc::now();
        for _ in 0..4 {
            rl.record_fail("a@b.com", now).await;
        }
        assert_eq!(rl.check("a@b.com", now).await, RateLimitStatus::Allowed);
    }

    #[tokio::test]
    async fn locked_after_threshold() {
        let rl = make_limiter();
        let now = Utc::now();
        for _ in 0..5 {
            rl.record_fail("a@b.com", now).await;
        }
        let status = rl.check("a@b.com", now).await;
        assert!(matches!(status, RateLimitStatus::Locked(_)));
    }

    #[tokio::test]
    async fn reset_clears_counter() {
        let rl = make_limiter();
        let now = Utc::now();
        for _ in 0..4 {
            rl.record_fail("a@b.com", now).await;
        }
        rl.reset("a@b.com").await;
        assert_eq!(rl.check("a@b.com", now).await, RateLimitStatus::Allowed);
        // After reset, 1 fail should not lock.
        rl.record_fail("a@b.com", now).await;
        assert_eq!(rl.check("a@b.com", now).await, RateLimitStatus::Allowed);
    }

    #[tokio::test]
    async fn lockout_expires() {
        let rl = make_limiter();
        let now = Utc::now();
        for _ in 0..5 {
            rl.record_fail("a@b.com", now).await;
        }
        // 16 minutes later
        let later = now + chrono::Duration::minutes(16);
        assert_eq!(rl.check("a@b.com", later).await, RateLimitStatus::Allowed);
    }

    #[tokio::test]
    async fn case_insensitive_email() {
        let rl = make_limiter();
        let now = Utc::now();
        for _ in 0..5 {
            rl.record_fail("A@B.COM", now).await;
        }
        assert!(matches!(
            rl.check("a@b.com", now).await,
            RateLimitStatus::Locked(_)
        ));
    }
}

//! Per-sender sliding window rate limiter for channel messages.
//!
//! Uses a `DashMap` to track per-sender action timestamps within a one-hour
//! sliding window. Thread-safe and lock-free for concurrent access.

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sliding window rate limiter that tracks actions per sender.
#[derive(Clone)]
pub struct SenderRateLimiter {
    /// Map of sender_id -> list of action timestamps within the window.
    windows: Arc<DashMap<String, Vec<Instant>>>,
    /// Maximum actions per sender per hour.
    max_per_hour: u32,
    /// Window duration (1 hour).
    window: Duration,
}

impl SenderRateLimiter {
    /// Create a new rate limiter with the given per-sender hourly limit.
    pub fn new(max_per_hour: u32) -> Self {
        Self {
            windows: Arc::new(DashMap::new()),
            max_per_hour,
            window: Duration::from_secs(3600),
        }
    }

    /// Check if a sender is allowed to perform an action.
    /// Returns `Ok(())` if allowed, `Err(reason)` if rate limited.
    pub fn check(&self, sender_id: &str) -> Result<(), String> {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window);

        let mut entry = self.windows.entry(sender_id.to_string()).or_default();
        // Remove expired entries (if cutoff is None, system uptime < window so keep all)
        if let Some(cutoff) = cutoff {
            entry.retain(|t| *t > cutoff);
        }

        if entry.len() >= self.max_per_hour as usize {
            return Err(format!(
                "sender '{}' exceeded rate limit ({} actions/hour)",
                sender_id, self.max_per_hour
            ));
        }

        entry.push(now);
        Ok(())
    }

    /// Get current action count for a sender (excluding expired entries).
    pub fn current_count(&self, sender_id: &str) -> usize {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window);

        self.windows
            .get(sender_id)
            .map(|entry| {
                entry
                    .iter()
                    .filter(|t| cutoff.map_or(true, |c| **t > c))
                    .count()
            })
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiter_allows_within_limit() {
        let limiter = SenderRateLimiter::new(10);
        for i in 0..5 {
            assert!(
                limiter.check("alice").is_ok(),
                "action {i} should be allowed within limit of 10"
            );
        }
        assert_eq!(limiter.current_count("alice"), 5);
    }

    #[test]
    fn rate_limiter_blocks_over_limit() {
        let limiter = SenderRateLimiter::new(3);
        assert!(limiter.check("bob").is_ok(), "action 1 should be allowed");
        assert!(limiter.check("bob").is_ok(), "action 2 should be allowed");
        assert!(limiter.check("bob").is_ok(), "action 3 should be allowed");

        let result = limiter.check("bob");
        assert!(result.is_err(), "action 4 should be blocked");
        let err = result.expect_err("should have error message");
        assert!(
            err.contains("exceeded rate limit"),
            "error should mention rate limit, got: {err}"
        );
        assert!(
            err.contains("bob"),
            "error should mention sender, got: {err}"
        );
    }

    #[test]
    fn rate_limiter_tracks_per_sender() {
        let limiter = SenderRateLimiter::new(2);

        // Alice uses both slots
        assert!(limiter.check("alice").is_ok());
        assert!(limiter.check("alice").is_ok());
        assert!(limiter.check("alice").is_err(), "alice should be blocked");

        // Bob should still have his own independent limit
        assert!(
            limiter.check("bob").is_ok(),
            "bob should be allowed independently"
        );
        assert!(limiter.check("bob").is_ok());
        assert!(limiter.check("bob").is_err(), "bob should now be blocked");

        assert_eq!(limiter.current_count("alice"), 2);
        assert_eq!(limiter.current_count("bob"), 2);
        assert_eq!(
            limiter.current_count("charlie"),
            0,
            "unknown sender should have 0 count"
        );
    }

    #[test]
    fn current_count_returns_zero_for_unknown_sender() {
        let limiter = SenderRateLimiter::new(10);
        assert_eq!(limiter.current_count("nobody"), 0);
    }
}

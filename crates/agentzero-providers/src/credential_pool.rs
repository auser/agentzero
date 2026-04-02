//! Credential pooling — multiple API keys per provider with rotation strategies.
//!
//! Avoids rate limits by distributing requests across multiple keys.
//! Each key gets independent cooldown tracking (1h on 429, 24h on persistent errors).

use crate::transport::CooldownState;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Selection strategy for choosing which credential to use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PoolStrategy {
    /// Use the first available key until exhausted, then move to next.
    FillFirst,
    /// Cycle through keys sequentially.
    #[default]
    RoundRobin,
    /// Pick randomly.
    Random,
}

/// A pool of API credentials with rotation and cooldown.
pub struct CredentialPool {
    keys: Vec<PooledKey>,
    strategy: PoolStrategy,
    round_robin_index: AtomicUsize,
}

struct PooledKey {
    key: String,
    cooldown: CooldownState,
}

/// Cooldown durations.
const RATE_LIMIT_COOLDOWN: Duration = Duration::from_secs(3600); // 1 hour for 429
const ERROR_COOLDOWN: Duration = Duration::from_secs(86400); // 24 hours for persistent errors

impl CredentialPool {
    /// Create a pool from a list of API keys.
    pub fn new(keys: Vec<String>, strategy: PoolStrategy) -> Self {
        let pooled: Vec<PooledKey> = keys
            .into_iter()
            .map(|key| PooledKey {
                key,
                cooldown: CooldownState::new(),
            })
            .collect();
        Self {
            keys: pooled,
            strategy,
            round_robin_index: AtomicUsize::new(0),
        }
    }

    /// Select the next available credential. Returns `None` if all keys are in cooldown.
    pub fn select(&self) -> Option<&str> {
        if self.keys.is_empty() {
            return None;
        }

        match self.strategy {
            PoolStrategy::FillFirst => self.select_fill_first(),
            PoolStrategy::RoundRobin => self.select_round_robin(),
            PoolStrategy::Random => self.select_random(),
        }
    }

    /// Report a 429 rate limit for the given key. Enters 1-hour cooldown.
    pub fn report_rate_limit(&self, key: &str) {
        if let Some(pooled) = self.keys.iter().find(|k| k.key == key) {
            pooled.cooldown.enter_cooldown(RATE_LIMIT_COOLDOWN);
        }
    }

    /// Report a persistent error for the given key. Enters 24-hour cooldown.
    pub fn report_error(&self, key: &str) {
        if let Some(pooled) = self.keys.iter().find(|k| k.key == key) {
            pooled.cooldown.enter_cooldown(ERROR_COOLDOWN);
        }
    }

    /// Number of keys in the pool.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the pool is empty.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Count of keys currently available (not in cooldown).
    pub fn available_count(&self) -> usize {
        self.keys
            .iter()
            .filter(|k| !k.cooldown.is_cooled_down())
            .count()
    }

    fn select_fill_first(&self) -> Option<&str> {
        self.keys
            .iter()
            .find(|k| !k.cooldown.is_cooled_down())
            .map(|k| k.key.as_str())
    }

    fn select_round_robin(&self) -> Option<&str> {
        let len = self.keys.len();
        let start = self.round_robin_index.fetch_add(1, Ordering::Relaxed) % len;
        // Try each key starting from the index.
        for i in 0..len {
            let idx = (start + i) % len;
            if !self.keys[idx].cooldown.is_cooled_down() {
                return Some(&self.keys[idx].key);
            }
        }
        None
    }

    fn select_random(&self) -> Option<&str> {
        let available: Vec<&PooledKey> = self
            .keys
            .iter()
            .filter(|k| !k.cooldown.is_cooled_down())
            .collect();
        if available.is_empty() {
            return None;
        }
        // Simple pseudo-random using timestamp nanos.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize;
        let idx = nanos % available.len();
        Some(&available[idx].key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keys() -> Vec<String> {
        vec!["key-1".into(), "key-2".into(), "key-3".into()]
    }

    #[test]
    fn fill_first_returns_first_available() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::FillFirst);
        assert_eq!(pool.select(), Some("key-1"));
        assert_eq!(pool.select(), Some("key-1")); // always first
    }

    #[test]
    fn fill_first_skips_cooled_down() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::FillFirst);
        pool.report_rate_limit("key-1");
        assert_eq!(pool.select(), Some("key-2"));
    }

    #[test]
    fn round_robin_rotates() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::RoundRobin);
        let k1 = pool.select().expect("key");
        let k2 = pool.select().expect("key");
        let k3 = pool.select().expect("key");
        // Should have cycled through at least 2 different keys.
        assert!(k1 != k2 || k2 != k3, "round-robin should rotate");
    }

    #[test]
    fn round_robin_skips_cooled_down() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::RoundRobin);
        pool.report_rate_limit("key-1");
        pool.report_rate_limit("key-2");
        // Only key-3 should be available.
        for _ in 0..5 {
            assert_eq!(pool.select(), Some("key-3"));
        }
    }

    #[test]
    fn all_cooled_down_returns_none() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::FillFirst);
        pool.report_rate_limit("key-1");
        pool.report_rate_limit("key-2");
        pool.report_rate_limit("key-3");
        assert!(pool.select().is_none());
    }

    #[test]
    fn random_returns_available() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::Random);
        let key = pool.select().expect("should return a key");
        assert!(["key-1", "key-2", "key-3"].contains(&key));
    }

    #[test]
    fn available_count_tracks_cooldowns() {
        let pool = CredentialPool::new(test_keys(), PoolStrategy::FillFirst);
        assert_eq!(pool.available_count(), 3);
        pool.report_rate_limit("key-1");
        assert_eq!(pool.available_count(), 2);
        pool.report_error("key-2");
        assert_eq!(pool.available_count(), 1);
    }

    #[test]
    fn empty_pool() {
        let pool = CredentialPool::new(vec![], PoolStrategy::RoundRobin);
        assert!(pool.is_empty());
        assert!(pool.select().is_none());
    }

    #[test]
    fn single_key_pool() {
        let pool = CredentialPool::new(vec!["only-key".into()], PoolStrategy::RoundRobin);
        assert_eq!(pool.select(), Some("only-key"));
        assert_eq!(pool.len(), 1);
    }
}

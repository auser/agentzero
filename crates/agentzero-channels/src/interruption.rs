//! Same-sender same-channel message interruption.
//!
//! When `interrupt_on_new_message` is enabled, a new message from the same
//! sender in the same channel cancels any in-flight turn for that sender.
//! The handler task checks the cancellation token and aborts if interrupted.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

/// A lightweight cancellation token backed by `AtomicBool`.
#[derive(Clone)]
pub struct CancelToken {
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Mark this token as cancelled.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check if this token has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Composite key for tracking active turns: (sender, channel).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct TurnKey {
    pub sender: String,
    pub channel: String,
}

impl TurnKey {
    pub fn new(sender: impl Into<String>, channel: impl Into<String>) -> Self {
        Self {
            sender: sender.into(),
            channel: channel.into(),
        }
    }
}

/// Tracks active turns and manages cancellation tokens.
pub struct InterruptionDetector {
    active_turns: Arc<Mutex<HashMap<TurnKey, CancelToken>>>,
}

impl InterruptionDetector {
    pub fn new() -> Self {
        Self {
            active_turns: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a new turn for the given key.
    ///
    /// If there is already an active turn for this key, it is cancelled first.
    /// Returns the cancellation token for the new turn — the handler should
    /// check `token.is_cancelled()` periodically.
    pub async fn start_turn(&self, key: TurnKey) -> CancelToken {
        let mut turns = self.active_turns.lock().await;

        // Cancel any existing turn for this sender+channel
        if let Some(existing) = turns.remove(&key) {
            existing.cancel();
        }

        let token = CancelToken::new();
        turns.insert(key, token.clone());
        token
    }

    /// Finish a turn (normal completion). Removes the token from tracking.
    pub async fn finish_turn(&self, key: &TurnKey) {
        self.active_turns.lock().await.remove(key);
    }

    /// Check if a turn is currently active for the given key.
    pub async fn has_active_turn(&self, key: &TurnKey) -> bool {
        self.active_turns.lock().await.contains_key(key)
    }

    /// Get the number of active turns.
    pub async fn active_count(&self) -> usize {
        self.active_turns.lock().await.len()
    }

    /// Cancel all active turns (e.g. on shutdown).
    pub async fn cancel_all(&self) {
        let mut turns = self.active_turns.lock().await;
        for (_, token) in turns.drain() {
            token.cancel();
        }
    }
}

impl Default for InterruptionDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sender: &str, channel: &str) -> TurnKey {
        TurnKey::new(sender, channel)
    }

    #[tokio::test]
    async fn start_turn_returns_active_token() {
        let detector = InterruptionDetector::new();
        let token = detector.start_turn(key("alice", "telegram")).await;

        assert!(!token.is_cancelled());
        assert!(detector.has_active_turn(&key("alice", "telegram")).await);
        assert_eq!(detector.active_count().await, 1);
    }

    #[tokio::test]
    async fn new_turn_cancels_previous() {
        let detector = InterruptionDetector::new();
        let token1 = detector.start_turn(key("alice", "telegram")).await;
        let token2 = detector.start_turn(key("alice", "telegram")).await;

        assert!(token1.is_cancelled(), "previous turn should be cancelled");
        assert!(!token2.is_cancelled(), "new turn should be active");
        assert_eq!(detector.active_count().await, 1);
    }

    #[tokio::test]
    async fn different_senders_are_independent() {
        let detector = InterruptionDetector::new();
        let token_alice = detector.start_turn(key("alice", "telegram")).await;
        let token_bob = detector.start_turn(key("bob", "telegram")).await;

        assert!(!token_alice.is_cancelled());
        assert!(!token_bob.is_cancelled());
        assert_eq!(detector.active_count().await, 2);
    }

    #[tokio::test]
    async fn different_channels_are_independent() {
        let detector = InterruptionDetector::new();
        let token_tg = detector.start_turn(key("alice", "telegram")).await;
        let token_slack = detector.start_turn(key("alice", "slack")).await;

        assert!(!token_tg.is_cancelled());
        assert!(!token_slack.is_cancelled());
        assert_eq!(detector.active_count().await, 2);
    }

    #[tokio::test]
    async fn finish_turn_removes_tracking() {
        let detector = InterruptionDetector::new();
        let _token = detector.start_turn(key("alice", "telegram")).await;

        detector.finish_turn(&key("alice", "telegram")).await;
        assert!(!detector.has_active_turn(&key("alice", "telegram")).await);
        assert_eq!(detector.active_count().await, 0);
    }

    #[tokio::test]
    async fn cancel_all_cancels_everything() {
        let detector = InterruptionDetector::new();
        let t1 = detector.start_turn(key("alice", "telegram")).await;
        let t2 = detector.start_turn(key("bob", "slack")).await;

        detector.cancel_all().await;

        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert_eq!(detector.active_count().await, 0);
    }

    #[tokio::test]
    async fn rapid_interruption_sequence() {
        let detector = InterruptionDetector::new();
        let k = key("alice", "telegram");

        let t1 = detector.start_turn(k.clone()).await;
        let t2 = detector.start_turn(k.clone()).await;
        let t3 = detector.start_turn(k.clone()).await;

        assert!(t1.is_cancelled());
        assert!(t2.is_cancelled());
        assert!(!t3.is_cancelled());
        assert_eq!(detector.active_count().await, 1);
    }

    #[tokio::test]
    async fn handler_detects_cancellation() {
        let detector = InterruptionDetector::new();
        let k = key("alice", "telegram");

        let token = detector.start_turn(k.clone()).await;

        // Simulate handler checking periodically
        assert!(!token.is_cancelled());

        // New message arrives — interrupts
        let _new_token = detector.start_turn(k).await;

        // Handler detects cancellation
        assert!(token.is_cancelled());
    }
}

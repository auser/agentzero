//! Draft lifecycle orchestration: send_draft → update_draft → finalize_draft.
//!
//! When `stream_mode` is enabled, the agent sends incremental "draft" messages
//! that are progressively updated as the response streams in. The `DraftTracker`
//! manages the state for each in-flight draft and throttles update calls to
//! respect `draft_update_interval_ms`.

use crate::Channel;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Identifies a draft by (recipient, channel_name).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct DraftKey {
    pub recipient: String,
    pub channel_name: String,
}

/// State of an in-flight draft message.
#[derive(Debug)]
struct DraftState {
    message_id: String,
    last_update: Instant,
    latest_text: String,
}

/// Tracks in-flight drafts across channels and throttles updates.
pub struct DraftTracker {
    drafts: Arc<Mutex<HashMap<DraftKey, DraftState>>>,
    update_interval: Duration,
}

impl DraftTracker {
    pub fn new(update_interval_ms: u64) -> Self {
        Self {
            drafts: Arc::new(Mutex::new(HashMap::new())),
            update_interval: Duration::from_millis(update_interval_ms),
        }
    }

    /// Start a new draft. Calls `channel.send_draft()` and tracks the returned message ID.
    ///
    /// Returns `Ok(message_id)` if the channel supports drafts and created one,
    /// or `Ok(None)` if the channel does not support drafts.
    pub async fn start(
        &self,
        key: DraftKey,
        initial_text: &str,
        channel: &Arc<dyn Channel>,
    ) -> anyhow::Result<Option<String>> {
        if !channel.supports_draft_updates() {
            return Ok(None);
        }

        let msg = crate::SendMessage::new(initial_text, &key.recipient);
        let message_id = channel.send_draft(&msg).await?;

        if let Some(id) = &message_id {
            let state = DraftState {
                message_id: id.clone(),
                last_update: Instant::now(),
                latest_text: initial_text.to_string(),
            };
            self.drafts.lock().await.insert(key, state);
        }

        Ok(message_id)
    }

    /// Update an in-flight draft with new text.
    ///
    /// Respects the throttle interval — if called too frequently, the text is
    /// buffered and only the latest version is sent when the interval elapses.
    /// Returns `true` if an update was actually sent to the channel.
    pub async fn update(
        &self,
        key: &DraftKey,
        text: &str,
        channel: &Arc<dyn Channel>,
    ) -> anyhow::Result<bool> {
        let mut drafts = self.drafts.lock().await;
        let Some(state) = drafts.get_mut(key) else {
            return Ok(false);
        };

        state.latest_text = text.to_string();

        if state.last_update.elapsed() < self.update_interval {
            return Ok(false);
        }

        channel
            .update_draft(&key.recipient, &state.message_id, text)
            .await?;
        state.last_update = Instant::now();

        Ok(true)
    }

    /// Finalize a draft — sends the final text and removes tracking.
    pub async fn finalize(
        &self,
        key: &DraftKey,
        final_text: &str,
        channel: &Arc<dyn Channel>,
    ) -> anyhow::Result<()> {
        let state = self.drafts.lock().await.remove(key);
        if let Some(state) = state {
            channel
                .finalize_draft(&key.recipient, &state.message_id, final_text)
                .await?;
        }
        Ok(())
    }

    /// Cancel a draft — removes tracking and notifies the channel.
    pub async fn cancel(&self, key: &DraftKey, channel: &Arc<dyn Channel>) -> anyhow::Result<()> {
        let state = self.drafts.lock().await.remove(key);
        if let Some(state) = state {
            channel
                .cancel_draft(&key.recipient, &state.message_id)
                .await?;
        }
        Ok(())
    }

    /// Check if a draft is currently tracked for the given key.
    pub async fn has_draft(&self, key: &DraftKey) -> bool {
        self.drafts.lock().await.contains_key(key)
    }

    /// Get the number of active drafts.
    pub async fn active_count(&self) -> usize {
        self.drafts.lock().await.len()
    }

    /// Flush a pending update — sends the latest buffered text regardless of throttle.
    pub async fn flush(&self, key: &DraftKey, channel: &Arc<dyn Channel>) -> anyhow::Result<bool> {
        let mut drafts = self.drafts.lock().await;
        let Some(state) = drafts.get_mut(key) else {
            return Ok(false);
        };

        let text = state.latest_text.clone();
        channel
            .update_draft(&key.recipient, &state.message_id, &text)
            .await?;
        state.last_update = Instant::now();

        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A mock channel that tracks draft method calls.
    struct MockDraftChannel {
        draft_sends: AtomicU32,
        draft_updates: AtomicU32,
        draft_finalizes: AtomicU32,
        draft_cancels: AtomicU32,
    }

    impl MockDraftChannel {
        fn new() -> Self {
            Self {
                draft_sends: AtomicU32::new(0),
                draft_updates: AtomicU32::new(0),
                draft_finalizes: AtomicU32::new(0),
                draft_cancels: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Channel for MockDraftChannel {
        fn name(&self) -> &str {
            "mock-draft"
        }

        async fn send(&self, _message: &crate::SendMessage) -> anyhow::Result<()> {
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<crate::ChannelMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn supports_draft_updates(&self) -> bool {
            true
        }

        async fn send_draft(
            &self,
            _message: &crate::SendMessage,
        ) -> anyhow::Result<Option<String>> {
            self.draft_sends.fetch_add(1, Ordering::SeqCst);
            Ok(Some(format!(
                "draft-{}",
                self.draft_sends.load(Ordering::SeqCst)
            )))
        }

        async fn update_draft(
            &self,
            _recipient: &str,
            _message_id: &str,
            _text: &str,
        ) -> anyhow::Result<Option<String>> {
            self.draft_updates.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }

        async fn finalize_draft(
            &self,
            _recipient: &str,
            _message_id: &str,
            _text: &str,
        ) -> anyhow::Result<()> {
            self.draft_finalizes.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> anyhow::Result<()> {
            self.draft_cancels.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    /// A channel that does not support drafts.
    struct NoDraftChannel;

    #[async_trait]
    impl Channel for NoDraftChannel {
        fn name(&self) -> &str {
            "no-draft"
        }
        async fn send(&self, _message: &crate::SendMessage) -> anyhow::Result<()> {
            Ok(())
        }
        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<crate::ChannelMessage>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn test_key() -> DraftKey {
        DraftKey {
            recipient: "user-1".into(),
            channel_name: "mock-draft".into(),
        }
    }

    #[tokio::test]
    async fn start_creates_draft_and_tracks_it() {
        let ch: Arc<dyn Channel> = Arc::new(MockDraftChannel::new());
        let tracker = DraftTracker::new(500);
        let key = test_key();

        let id = tracker.start(key.clone(), "hello", &ch).await.unwrap();
        assert!(id.is_some());
        assert!(tracker.has_draft(&key).await);
        assert_eq!(tracker.active_count().await, 1);
    }

    #[tokio::test]
    async fn start_returns_none_for_non_draft_channel() {
        let ch: Arc<dyn Channel> = Arc::new(NoDraftChannel);
        let tracker = DraftTracker::new(500);
        let key = test_key();

        let id = tracker.start(key.clone(), "hello", &ch).await.unwrap();
        assert!(id.is_none());
        assert!(!tracker.has_draft(&key).await);
    }

    #[tokio::test]
    async fn update_throttles_rapid_calls() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(1000); // 1 second throttle
        let key = test_key();

        tracker.start(key.clone(), "initial", &ch).await.unwrap();

        // Immediate update should be throttled (interval not elapsed)
        let sent = tracker.update(&key, "update-1", &ch).await.unwrap();
        assert!(!sent, "first rapid update should be throttled");

        // Updates counter should still be 0
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn update_sends_after_interval() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(50); // 50ms throttle
        let key = test_key();

        tracker.start(key.clone(), "initial", &ch).await.unwrap();

        // Wait for throttle interval
        tokio::time::sleep(Duration::from_millis(60)).await;

        let sent = tracker.update(&key, "update-1", &ch).await.unwrap();
        assert!(sent, "update after interval should succeed");
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn finalize_sends_final_and_removes_tracking() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(500);
        let key = test_key();

        tracker.start(key.clone(), "initial", &ch).await.unwrap();
        assert!(tracker.has_draft(&key).await);

        tracker.finalize(&key, "final text", &ch).await.unwrap();
        assert!(!tracker.has_draft(&key).await);
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancel_removes_tracking_and_notifies() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(500);
        let key = test_key();

        tracker.start(key.clone(), "initial", &ch).await.unwrap();
        tracker.cancel(&key, &ch).await.unwrap();

        assert!(!tracker.has_draft(&key).await);
        assert_eq!(mock.draft_cancels.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn finalize_without_start_is_noop() {
        let ch: Arc<dyn Channel> = Arc::new(MockDraftChannel::new());
        let tracker = DraftTracker::new(500);
        let key = test_key();

        // Should not error
        tracker.finalize(&key, "text", &ch).await.unwrap();
    }

    #[tokio::test]
    async fn flush_sends_regardless_of_throttle() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(10_000); // very long throttle
        let key = test_key();

        tracker.start(key.clone(), "initial", &ch).await.unwrap();

        // Buffer an update (throttled)
        tracker.update(&key, "latest", &ch).await.unwrap();

        // Flush should send immediately
        let flushed = tracker.flush(&key, &ch).await.unwrap();
        assert!(flushed);
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn full_lifecycle_send_update_finalize() {
        let mock = Arc::new(MockDraftChannel::new());
        let ch: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(10); // short throttle for testing
        let key = test_key();

        // Start
        let id = tracker
            .start(key.clone(), "thinking...", &ch)
            .await
            .unwrap();
        assert!(id.is_some());
        assert_eq!(mock.draft_sends.load(Ordering::SeqCst), 1);

        // Update (after throttle)
        tokio::time::sleep(Duration::from_millis(20)).await;
        tracker.update(&key, "thinking... more", &ch).await.unwrap();
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 1);

        // Finalize
        tracker
            .finalize(&key, "Here is the answer.", &ch)
            .await
            .unwrap();
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 1);
        assert_eq!(tracker.active_count().await, 0);
    }
}

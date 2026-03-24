//! Stream-to-draft bridge: consumes [`StreamChunk`]s from a provider and
//! feeds them into a [`DraftTracker`] so the user sees progressive updates
//! in channels that support drafts.

use crate::drafts::{DraftKey, DraftTracker};
use crate::Channel;
use agentzero_core::StreamChunk;
use std::sync::Arc;

/// Consume streaming chunks from `rx` and forward them to the draft tracker.
///
/// Each chunk's `delta` is appended to an accumulator. Intermediate updates
/// are pushed through [`DraftTracker::update`] (throttled internally), and the
/// final chunk triggers [`DraftTracker::finalize`].
///
/// Returns the fully accumulated response text.
pub async fn consume_stream_to_draft(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<StreamChunk>,
    draft_tracker: &DraftTracker,
    key: &DraftKey,
    channel: &Arc<dyn Channel>,
) -> String {
    let mut accumulated = String::new();
    while let Some(chunk) = rx.recv().await {
        accumulated.push_str(&chunk.delta);
        if chunk.done {
            let _ = draft_tracker.finalize(key, &accumulated, channel).await;
            break;
        } else {
            let _ = draft_tracker.update(key, &accumulated, channel).await;
        }
    }
    accumulated
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A mock channel that tracks draft method calls.
    struct MockStreamChannel {
        draft_updates: AtomicU32,
        draft_finalizes: AtomicU32,
    }

    impl MockStreamChannel {
        fn new() -> Self {
            Self {
                draft_updates: AtomicU32::new(0),
                draft_finalizes: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl Channel for MockStreamChannel {
        fn name(&self) -> &str {
            "mock-stream"
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
            Ok(Some("draft-1".to_string()))
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
            Ok(())
        }
    }

    fn test_key() -> DraftKey {
        DraftKey {
            recipient: "user-1".into(),
            channel_name: "mock-stream".into(),
        }
    }

    #[tokio::test]
    async fn consume_stream_accumulates_and_finalizes() {
        let mock = Arc::new(MockStreamChannel::new());
        let channel: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(10); // short throttle
        let key = test_key();

        // Start a draft so the tracker has state
        tracker
            .start(key.clone(), "", &channel)
            .await
            .expect("start should succeed");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Send chunks in a separate task
        let send_handle = tokio::spawn(async move {
            tx.send(StreamChunk {
                delta: "Hello".into(),
                done: false,
                tool_call_delta: None,
            })
            .expect("send should succeed");
            tx.send(StreamChunk {
                delta: " world".into(),
                done: false,
                tool_call_delta: None,
            })
            .expect("send should succeed");
            tx.send(StreamChunk {
                delta: "!".into(),
                done: true,
                tool_call_delta: None,
            })
            .expect("send should succeed");
        });

        let result = consume_stream_to_draft(rx, &tracker, &key, &channel).await;
        send_handle.await.expect("sender task should complete");

        assert_eq!(result, "Hello world!");
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 1);
        // Draft should be removed after finalization
        assert!(!tracker.has_draft(&key).await);
    }

    #[tokio::test]
    async fn consume_stream_handles_empty_stream() {
        let mock = Arc::new(MockStreamChannel::new());
        let channel: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(10);
        let key = test_key();

        tracker
            .start(key.clone(), "", &channel)
            .await
            .expect("start should succeed");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        // Drop sender immediately — simulates empty stream
        drop(tx);

        let result = consume_stream_to_draft(rx, &tracker, &key, &channel).await;
        assert_eq!(result, "");
        // Finalize was never called because no done=true chunk arrived
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn consume_stream_single_done_chunk() {
        let mock = Arc::new(MockStreamChannel::new());
        let channel: Arc<dyn Channel> = mock.clone();
        let tracker = DraftTracker::new(10);
        let key = test_key();

        tracker
            .start(key.clone(), "", &channel)
            .await
            .expect("start should succeed");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(StreamChunk {
            delta: "Complete response".into(),
            done: true,
            tool_call_delta: None,
        })
        .expect("send should succeed");

        let result = consume_stream_to_draft(rx, &tracker, &key, &channel).await;
        assert_eq!(result, "Complete response");
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 1);
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn consume_stream_updates_are_throttled() {
        let mock = Arc::new(MockStreamChannel::new());
        let channel: Arc<dyn Channel> = mock.clone();
        // Very long throttle — updates will be buffered
        let tracker = DraftTracker::new(10_000);
        let key = test_key();

        tracker
            .start(key.clone(), "", &channel)
            .await
            .expect("start should succeed");

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Send several non-done chunks rapidly
        for i in 0..5 {
            tx.send(StreamChunk {
                delta: format!("chunk-{i} "),
                done: false,
                tool_call_delta: None,
            })
            .expect("send should succeed");
        }
        tx.send(StreamChunk {
            delta: "end".into(),
            done: true,
            tool_call_delta: None,
        })
        .expect("send should succeed");

        let result = consume_stream_to_draft(rx, &tracker, &key, &channel).await;
        assert_eq!(result, "chunk-0 chunk-1 chunk-2 chunk-3 chunk-4 end");
        // Updates were throttled — none should have been sent to the channel
        assert_eq!(mock.draft_updates.load(Ordering::SeqCst), 0);
        // But finalize should always happen
        assert_eq!(mock.draft_finalizes.load(Ordering::SeqCst), 1);
    }
}

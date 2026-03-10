//! Gateway channel: bridges HTTP/WebSocket API requests into the swarm event bus.
//!
//! When the swarm is enabled, API callers publish messages through this channel
//! and receive pipeline responses back. Each request gets a unique correlation ID
//! so the response handler can route the pipeline's output back to the waiting caller.

use agentzero_channels::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot, Mutex};

/// A channel that bridges HTTP API requests into the swarm coordinator.
///
/// - `submit()` pushes a message into the channel's listen loop and waits for
///   the pipeline response (returned by the coordinator via `send()`).
/// - `listen()` drains the internal queue, publishing each message to the bus.
/// - `send()` routes pipeline outputs back to the waiting `submit()` caller
///   via a correlation-keyed oneshot.
pub struct GatewayChannel {
    /// Inbound messages from API callers → swarm.
    inbound_tx: mpsc::Sender<ChannelMessage>,
    inbound_rx: Mutex<Option<mpsc::Receiver<ChannelMessage>>>,
    /// Maps reply_target (correlation ID) → oneshot sender for the response.
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
}

impl GatewayChannel {
    pub fn new(capacity: usize) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(capacity);
        Arc::new(Self {
            inbound_tx: tx,
            inbound_rx: Mutex::new(Some(rx)),
            pending: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Submit a message to the swarm pipeline and wait for the response.
    ///
    /// Returns the pipeline's final output text, or an error on timeout/failure.
    pub async fn submit(
        &self,
        message: String,
        timeout: std::time::Duration,
    ) -> anyhow::Result<String> {
        let correlation_id = new_correlation_id();
        let (response_tx, response_rx) = oneshot::channel();

        // Register the pending response handler.
        self.pending
            .lock()
            .await
            .insert(correlation_id.clone(), response_tx);

        // Push the message into the channel's listen loop.
        let msg = ChannelMessage {
            id: new_message_id(),
            sender: "api".to_string(),
            reply_target: correlation_id.clone(),
            content: message,
            channel: "gateway".to_string(),
            timestamp: now_epoch_secs(),
            thread_ts: None,
            privacy_boundary: String::new(),
        };

        self.inbound_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("gateway channel closed"))?;

        // Wait for the pipeline to respond.
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                self.pending.lock().await.remove(&correlation_id);
                Err(anyhow::anyhow!("pipeline response channel dropped"))
            }
            Err(_) => {
                self.pending.lock().await.remove(&correlation_id);
                Err(anyhow::anyhow!("pipeline timed out"))
            }
        }
    }
}

#[async_trait]
impl Channel for GatewayChannel {
    fn name(&self) -> &str {
        "gateway"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        // The coordinator calls send() with the pipeline's final output.
        // `recipient` is the correlation ID (set as reply_target in the inbound message).
        let mut pending = self.pending.lock().await;
        if let Some(tx) = pending.remove(&message.recipient) {
            let _ = tx.send(message.content.clone());
        } else {
            tracing::debug!(
                recipient = %message.recipient,
                "gateway channel send: no pending request for correlation id"
            );
        }
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        // Take the receiver — listen() should only be called once.
        let mut rx = self
            .inbound_rx
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("gateway channel listen() called more than once"))?;

        while let Some(msg) = rx.recv().await {
            if tx.send(msg).await.is_err() {
                break;
            }
        }
        Ok(())
    }
}

fn new_message_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = now_epoch_secs();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("gw-{ts}-{seq}")
}

fn new_correlation_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("gw-corr-{ts}-{seq}")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gateway_channel_name() {
        let ch = GatewayChannel::new(16);
        assert_eq!(ch.name(), "gateway");
    }

    #[tokio::test]
    async fn submit_and_respond_roundtrip() {
        let ch = GatewayChannel::new(16);

        // Simulate the coordinator's listen + send cycle in a background task.
        let ch_clone = ch.clone();
        let listen_handle = tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(16);
            // Start listener in background.
            let ch2 = ch_clone.clone();
            tokio::spawn(async move {
                ch2.listen(tx).await.ok();
            });

            // Receive one message and respond.
            if let Some(msg) = rx.recv().await {
                ch_clone
                    .send(&SendMessage {
                        recipient: msg.reply_target,
                        content: format!("processed: {}", msg.content),
                        subject: None,
                        thread_ts: None,
                    })
                    .await
                    .unwrap();
            }
        });

        // Give the listener a moment to start.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let result = ch
            .submit("hello".to_string(), std::time::Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(result, "processed: hello");

        listen_handle.await.unwrap();
    }

    #[tokio::test]
    async fn submit_timeout() {
        let ch = GatewayChannel::new(16);

        // Start listener that never responds.
        let ch2 = ch.clone();
        tokio::spawn(async move {
            let (tx, mut _rx) = mpsc::channel(16);
            ch2.listen(tx).await.ok();
        });

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let result = ch
            .submit("hello".to_string(), std::time::Duration::from_millis(50))
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }
}

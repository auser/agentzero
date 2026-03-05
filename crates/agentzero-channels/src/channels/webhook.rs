use crate::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

super::channel_meta!(WEBHOOK_DESCRIPTOR, "webhook", "Webhook");

/// Generic HTTP webhook channel.
/// Messages are injected via `inject_message()` from the gateway webhook handler.
pub struct WebhookChannel {
    injector_tx: mpsc::Sender<ChannelMessage>,
    injector_rx: Arc<Mutex<Option<mpsc::Receiver<ChannelMessage>>>>,
}

impl WebhookChannel {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(256);
        Self {
            injector_tx: tx,
            injector_rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    /// Called by the gateway webhook handler to inject an inbound message.
    pub async fn inject_message(&self, msg: ChannelMessage) -> anyhow::Result<()> {
        self.injector_tx
            .send(msg)
            .await
            .map_err(|_| anyhow::anyhow!("webhook channel listener not running"))
    }
}

impl Default for WebhookChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
        tracing::debug!("webhook channel send is a no-op (inbound-only)");
        Ok(())
    }

    async fn listen(
        &self,
        tx: mpsc::Sender<ChannelMessage>,
    ) -> anyhow::Result<()> {
        let mut rx = self
            .injector_rx
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow::anyhow!("webhook listener already started"))?;

        while let Some(msg) = rx.recv().await {
            if tx.send(msg).await.is_err() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channels::helpers;

    #[test]
    fn webhook_channel_name() {
        let ch = WebhookChannel::new();
        assert_eq!(ch.name(), "webhook");
    }

    #[tokio::test]
    async fn webhook_inject_and_listen() {
        let ch = Arc::new(WebhookChannel::new());
        let (tx, mut rx) = mpsc::channel(16);

        let ch_clone = ch.clone();
        let listen_handle = tokio::spawn(async move {
            ch_clone.listen(tx).await.unwrap();
        });

        let msg = ChannelMessage {
            id: helpers::new_message_id(),
            sender: "external".into(),
            reply_target: "external".into(),
            content: "webhook payload".into(),
            channel: "webhook".into(),
            timestamp: helpers::now_epoch_secs(),
            thread_ts: None,
            privacy_boundary: String::new(),
        };

        ch.inject_message(msg).await.unwrap();

        let received = rx.recv().await.expect("should receive injected message");
        assert_eq!(received.content, "webhook payload");
        assert_eq!(received.channel, "webhook");

        // Abort the listener (it would run forever in production)
        listen_handle.abort();
    }

    #[tokio::test]
    async fn webhook_send_is_noop() {
        let ch = WebhookChannel::new();
        let msg = SendMessage::new("test", "recipient");
        assert!(ch.send(&msg).await.is_ok());
    }
}

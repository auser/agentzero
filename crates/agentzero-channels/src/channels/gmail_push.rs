#[cfg(feature = "channel-gmail-push")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(GMAIL_PUSH_DESCRIPTOR, "gmail-push", "Gmail Push");

    /// Push-based Gmail channel using Google Pub/Sub webhooks.
    /// Receives notifications via webhook, fetches messages via Gmail History API.
    pub struct GmailPushChannel {
        project_id: String,
        subscription_name: String,
    }

    impl GmailPushChannel {
        pub fn new(project_id: String, subscription_name: String) -> Self {
            Self {
                project_id,
                subscription_name,
            }
        }
    }

    #[async_trait]
    impl Channel for GmailPushChannel {
        fn name(&self) -> &str {
            "gmail-push"
        }

        async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
            // TODO: Send reply via Gmail API with RFC 2822 encoding
            anyhow::bail!("gmail-push send not yet implemented")
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // TODO: Webhook handler for Pub/Sub notifications
            tracing::info!("gmail-push channel started");
            tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
            Ok(())
        }
    }
}

#[cfg(feature = "channel-gmail-push")]
pub use impl_::*;

#[cfg(not(feature = "channel-gmail-push"))]
super::channel_stub!(
    GmailPushChannel,
    GMAIL_PUSH_DESCRIPTOR,
    "gmail-push",
    "Gmail Push"
);

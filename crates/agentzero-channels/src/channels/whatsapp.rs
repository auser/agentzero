#[cfg(feature = "channel-whatsapp")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(WHATSAPP_DESCRIPTOR, "whatsapp", "WhatsApp");

    const MAX_MESSAGE_LENGTH: usize = 4096;
    const POLL_INTERVAL_SECS: u64 = 5;

    /// WhatsApp Cloud API channel.
    pub struct WhatsappChannel {
        access_token: String,
        phone_number_id: String,
        verify_token: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl WhatsappChannel {
        pub fn new(access_token: String, phone_number_id: String, verify_token: String, allowed_users: Vec<String>) -> Self {
            let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("reqwest client should build");
            Self { access_token, phone_number_id, verify_token, allowed_users, client }
        }

        fn api_url(&self, path: &str) -> String {
            format!("https://graph.facebook.com/v18.0/{}{}", self.phone_number_id, path)
        }
    }

    #[async_trait]
    impl Channel for WhatsappChannel {
        fn name(&self) -> &str { "whatsapp" }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "messaging_product": "whatsapp",
                    "to": message.recipient,
                    "type": "text",
                    "text": {"body": chunk}
                });
                let resp = self.client.post(self.api_url("/messages")).bearer_auth(&self.access_token).json(&body).send().await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("whatsapp send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
            // WhatsApp Cloud API uses webhooks (push-based). In standalone mode, poll a local
            // webhook receiver or wait for injected messages. This implementation provides the
            // webhook verification and message parsing infrastructure.
            tracing::info!("whatsapp: listening requires webhook registration. Configure a webhook endpoint that forwards to this channel.");
            // Keep the listener alive — a real deployment would integrate with the gateway webhook handler
            loop { tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await; }
        }

        async fn health_check(&self) -> bool {
            self.client.get(self.api_url("")).bearer_auth(&self.access_token).send().await.map(|r| r.status().is_success()).unwrap_or(false)
        }
    }

    /// Parse an inbound WhatsApp webhook payload into a ChannelMessage.
    pub fn parse_webhook_message(payload: &serde_json::Value, allowed_users: &[String]) -> Option<ChannelMessage> {
        let entry = payload["entry"].as_array()?.first()?;
        let change = entry["changes"].as_array()?.first()?;
        let value = &change["value"];
        let message = value["messages"].as_array()?.first()?;
        let from = message["from"].as_str()?;
        if !helpers::is_user_allowed(from, allowed_users) { return None; }
        let text = message["text"]["body"].as_str()?;
        if text.is_empty() { return None; }
        Some(ChannelMessage {
            id: helpers::new_message_id(),
            sender: from.to_string(),
            reply_target: from.to_string(),
            content: text.to_string(),
            channel: "whatsapp".to_string(),
            timestamp: helpers::now_epoch_secs(),
            thread_ts: None,
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn whatsapp_channel_name() {
            let ch = WhatsappChannel::new("token".into(), "12345".into(), "verify".into(), vec![]);
            assert_eq!(ch.name(), "whatsapp");
        }

        #[test]
        fn whatsapp_api_url_format() {
            let ch = WhatsappChannel::new("t".into(), "12345".into(), "v".into(), vec![]);
            assert_eq!(ch.api_url("/messages"), "https://graph.facebook.com/v18.0/12345/messages");
        }

        #[test]
        fn parse_webhook_message_valid_payload() {
            let payload = serde_json::json!({
                "entry": [{"changes": [{"value": {"messages": [{"from": "1234567890", "text": {"body": "Hello"}}]}}]}]
            });
            let msg = parse_webhook_message(&payload, &[]).expect("should parse");
            assert_eq!(msg.sender, "1234567890");
            assert_eq!(msg.content, "Hello");
        }

        #[test]
        fn parse_webhook_message_filters_user() {
            let payload = serde_json::json!({
                "entry": [{"changes": [{"value": {"messages": [{"from": "blocked", "text": {"body": "Hi"}}]}}]}]
            });
            assert!(parse_webhook_message(&payload, &["allowed".to_string()]).is_none());
        }
    }
}

#[cfg(feature = "channel-whatsapp")]
pub use impl_::*;

#[cfg(not(feature = "channel-whatsapp"))]
super::channel_stub!(WhatsappChannel, WHATSAPP_DESCRIPTOR, "whatsapp", "WhatsApp");

#[cfg(feature = "channel-clawdtalk")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(CLAWDTALK_DESCRIPTOR, "clawdtalk", "ClawdTalk");

    const MAX_MESSAGE_LENGTH: usize = 32000;
    const POLL_INTERVAL_SECS: u64 = 2;

    /// ClawdTalk channel — a self-hosted chat bridge for AI agents.
    pub struct ClawdtalkChannel {
        base_url: String,
        api_key: String,
        room_id: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl ClawdtalkChannel {
        pub fn new(
            base_url: String,
            api_key: String,
            room_id: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                api_key,
                room_id,
                allowed_users,
                client,
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}/api/v1{}", self.base_url, path)
        }
    }

    #[async_trait]
    impl Channel for ClawdtalkChannel {
        fn name(&self) -> &str {
            "clawdtalk"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "room": message.recipient,
                    "text": chunk,
                });
                let resp = self
                    .client
                    .post(self.api_url("/messages"))
                    .bearer_auth(&self.api_key)
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("clawdtalk send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let mut cursor = String::new();
            loop {
                let mut url = self.api_url(&format!(
                    "/messages/stream?room={}&timeout={POLL_INTERVAL_SECS}",
                    self.room_id
                ));
                if !cursor.is_empty() {
                    url.push_str(&format!("&cursor={cursor}"));
                }
                let resp = match self
                    .client
                    .get(&url)
                    .bearer_auth(&self.api_key)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "clawdtalk poll failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "clawdtalk parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                if let Some(c) = json["cursor"].as_str() {
                    cursor = c.to_string();
                }
                if let Some(messages) = json["messages"].as_array() {
                    for msg in messages {
                        let sender = msg["sender"].as_str().unwrap_or("");
                        if sender.is_empty() {
                            continue;
                        }
                        if !helpers::is_user_allowed(sender, &self.allowed_users) {
                            continue;
                        }
                        let text = msg["text"].as_str().unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: sender.to_string(),
                            reply_target: self.room_id.clone(),
                            content: text.to_string(),
                            channel: "clawdtalk".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: None,
                            privacy_boundary: String::new(),
                        };
                        if tx.send(channel_msg).await.is_err() {
                            return Ok(());
                        }
                    }
                }
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(self.api_url("/health"))
                .bearer_auth(&self.api_key)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn clawdtalk_channel_name() {
            let ch = ClawdtalkChannel::new(
                "http://localhost:9000".into(),
                "key".into(),
                "room1".into(),
                vec![],
            );
            assert_eq!(ch.name(), "clawdtalk");
        }

        #[test]
        fn clawdtalk_api_url_format() {
            let ch = ClawdtalkChannel::new(
                "http://localhost:9000/".into(),
                "k".into(),
                "r".into(),
                vec![],
            );
            assert_eq!(
                ch.api_url("/messages"),
                "http://localhost:9000/api/v1/messages"
            );
        }
    }
}

#[cfg(feature = "channel-clawdtalk")]
pub use impl_::*;

#[cfg(not(feature = "channel-clawdtalk"))]
super::channel_stub!(ClawdtalkChannel, CLAWDTALK_DESCRIPTOR, "clawdtalk", "ClawdTalk");

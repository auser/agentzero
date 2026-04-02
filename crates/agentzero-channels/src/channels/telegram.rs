#[cfg(feature = "channel-telegram")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(TELEGRAM_DESCRIPTOR, "telegram", "Telegram");

    const DEFAULT_API_BASE: &str = "https://api.telegram.org/bot";
    const POLL_TIMEOUT_SECS: u64 = 30;
    const MAX_MESSAGE_LENGTH: usize = 4096;

    pub struct TelegramChannel {
        bot_token: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
        api_base: String,
    }

    impl TelegramChannel {
        pub fn new(bot_token: String, allowed_users: Vec<String>) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECS + 10))
                .build()
                .expect("reqwest client should build");
            Self {
                bot_token,
                allowed_users,
                client,
                api_base: DEFAULT_API_BASE.to_string(),
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        /// Override the API base URL (for testing with mock servers).
        pub fn with_base_url(mut self, base_url: String) -> Self {
            self.api_base = base_url;
            self
        }

        fn api_url(&self, method: &str) -> String {
            format!("{}{}/{}", self.api_base, self.bot_token, method)
        }
    }

    #[async_trait]
    impl Channel for TelegramChannel {
        fn name(&self) -> &str {
            "telegram"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "chat_id": message.recipient,
                    "text": chunk,
                });
                let resp = self
                    .client
                    .post(self.api_url("sendMessage"))
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("telegram sendMessage failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let mut offset: i64 = 0;

            loop {
                let body = serde_json::json!({
                    "offset": offset,
                    "timeout": POLL_TIMEOUT_SECS,
                    "allowed_updates": ["message"],
                });

                let resp = match self
                    .client
                    .post(self.api_url("getUpdates"))
                    .json(&body)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "telegram getUpdates request failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };

                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "telegram getUpdates parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };

                let updates = json["result"].as_array();
                let Some(updates) = updates else {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                };

                for update in updates {
                    if let Some(update_id) = update["update_id"].as_i64() {
                        offset = update_id + 1;
                    }

                    let message = &update["message"];
                    let text = message["text"]
                        .as_str()
                        .or_else(|| message["caption"].as_str())
                        .unwrap_or("");

                    // Extract native media attachments from Telegram message.
                    let mut attachments = Vec::new();
                    if let Some(photos) = message["photo"].as_array() {
                        // Telegram sends multiple sizes; take the largest (last).
                        if let Some(photo) = photos.last() {
                            if let Some(file_id) = photo["file_id"].as_str() {
                                attachments.push(crate::media::MediaAttachment {
                                    mime_type: "image/jpeg".to_string(),
                                    url: Some(format!(
                                        "https://api.telegram.org/file/bot{}/{}",
                                        self.bot_token, file_id
                                    )),
                                    transcript: None,
                                    description: None,
                                });
                            }
                        }
                    }
                    for key in ["document", "audio", "voice", "video", "video_note"] {
                        if let Some(file_id) = message[key]["file_id"].as_str() {
                            let mime = message[key]["mime_type"]
                                .as_str()
                                .unwrap_or("application/octet-stream");
                            attachments.push(crate::media::MediaAttachment {
                                mime_type: mime.to_string(),
                                url: Some(format!(
                                    "https://api.telegram.org/file/bot{}/{}",
                                    self.bot_token, file_id
                                )),
                                transcript: None,
                                description: None,
                            });
                        }
                    }

                    // Skip messages with no text AND no attachments.
                    if text.is_empty() && attachments.is_empty() {
                        continue;
                    }

                    let sender_id = message["from"]["id"]
                        .as_i64()
                        .map(|id| id.to_string())
                        .unwrap_or_default();

                    if !helpers::is_user_allowed(&sender_id, &self.allowed_users) {
                        tracing::debug!(sender = %sender_id, "telegram: ignoring message from unallowed user");
                        continue;
                    }

                    let chat_id = message["chat"]["id"]
                        .as_i64()
                        .map(|id| id.to_string())
                        .unwrap_or_default();

                    let msg = ChannelMessage {
                        id: helpers::new_message_id(),
                        sender: sender_id,
                        reply_target: chat_id,
                        content: text.to_string(),
                        channel: "telegram".to_string(),
                        timestamp: helpers::now_epoch_secs(),
                        thread_ts: None,
                        privacy_boundary: String::new(),
                        attachments,
                    };

                    if tx.send(msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }

        async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
            let body = serde_json::json!({
                "chat_id": recipient,
                "action": "typing",
            });
            let _ = self
                .client
                .post(self.api_url("sendChatAction"))
                .json(&body)
                .send()
                .await;
            Ok(())
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(self.api_url("getMe"))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }

        async fn register_webhook(&self, webhook_url: &str) -> anyhow::Result<()> {
            let url = format!("{}/v1/webhook/telegram", webhook_url.trim_end_matches('/'));
            let body = serde_json::json!({
                "url": url,
                "allowed_updates": ["message"],
                "drop_pending_updates": false,
            });
            let resp = self
                .client
                .post(self.api_url("setWebhook"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("telegram setWebhook failed: {status} {text}");
            }
            let json: serde_json::Value = resp.json().await?;
            if json["ok"].as_bool() != Some(true) {
                anyhow::bail!(
                    "telegram setWebhook returned error: {}",
                    json["description"].as_str().unwrap_or("unknown")
                );
            }
            tracing::info!(webhook_url = %url, "telegram webhook registered");
            Ok(())
        }

        async fn deregister_webhook(&self) -> anyhow::Result<()> {
            let resp = self
                .client
                .post(self.api_url("deleteWebhook"))
                .json(&serde_json::json!({}))
                .send()
                .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                anyhow::bail!("telegram deleteWebhook failed: {status} {text}");
            }
            tracing::info!("telegram webhook deregistered");
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn telegram_channel_name() {
            let ch = TelegramChannel::new("test-token".into(), vec![]);
            assert_eq!(ch.name(), "telegram");
        }

        #[test]
        fn telegram_api_url_format() {
            let ch = TelegramChannel::new("123:ABC".into(), vec![]);
            assert_eq!(
                ch.api_url("sendMessage"),
                "https://api.telegram.org/bot123:ABC/sendMessage"
            );
        }
    }
}

#[cfg(feature = "channel-telegram")]
pub use impl_::*;

#[cfg(not(feature = "channel-telegram"))]
super::channel_stub!(TelegramChannel, TELEGRAM_DESCRIPTOR, "telegram", "Telegram");

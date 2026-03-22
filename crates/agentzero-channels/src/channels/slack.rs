#[cfg(feature = "channel-slack")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(SLACK_DESCRIPTOR, "slack", "Slack");

    const API_BASE: &str = "https://slack.com/api";
    const POLL_INTERVAL_SECS: u64 = 3;
    const MAX_MESSAGE_LENGTH: usize = 40000;

    pub struct SlackChannel {
        bot_token: String,
        app_token: Option<String>,
        channel_id: Option<String>,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl SlackChannel {
        pub fn new(
            bot_token: String,
            app_token: Option<String>,
            channel_id: Option<String>,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                bot_token,
                app_token,
                channel_id,
                allowed_users,
                client,
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        /// Get the bot's own user ID via auth.test.
        async fn get_bot_user_id(&self) -> anyhow::Result<String> {
            let resp = self
                .client
                .post(format!("{API_BASE}/auth.test"))
                .bearer_auth(&self.bot_token)
                .send()
                .await?;
            let json: serde_json::Value = resp.json().await?;
            json["user_id"]
                .as_str()
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("failed to get slack bot user id"))
        }
    }

    #[async_trait]
    impl Channel for SlackChannel {
        fn name(&self) -> &str {
            "slack"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let mut body = serde_json::json!({
                    "channel": message.recipient,
                    "text": chunk,
                });
                if let Some(ref ts) = message.thread_ts {
                    body["thread_ts"] = serde_json::json!(ts);
                }

                let resp = self
                    .client
                    .post(format!("{API_BASE}/chat.postMessage"))
                    .bearer_auth(&self.bot_token)
                    .json(&body)
                    .send()
                    .await?;

                let status = resp.status();
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(5);
                    tracing::warn!(retry_after, "slack rate limited, waiting");
                    tokio::time::sleep(Duration::from_secs(retry_after)).await;
                    continue;
                }

                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("slack chat.postMessage failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // Socket Mode with app_token if available
            if let Some(ref app_token) = self.app_token {
                return self.listen_socket_mode(app_token, tx).await;
            }

            // Fallback: poll conversations.history
            let channel_id = self
                .channel_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("slack channel_id required for polling mode"))?;

            let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
            let mut latest_ts = String::new();

            loop {
                let mut url = format!(
                    "{API_BASE}/conversations.history?channel={channel_id}&limit=10"
                );
                if !latest_ts.is_empty() {
                    url.push_str(&format!("&oldest={latest_ts}"));
                }

                let resp = match self
                    .client
                    .get(&url)
                    .bearer_auth(&self.bot_token)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "slack conversations.history failed");
                        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                        continue;
                    }
                };

                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "slack parse failed");
                        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                        continue;
                    }
                };

                if let Some(messages) = json["messages"].as_array() {
                    // Messages come newest-first, reverse for chronological order
                    for msg in messages.iter().rev() {
                        let user = msg["user"].as_str().unwrap_or("");
                        let text = msg["text"].as_str().unwrap_or("");
                        let ts = msg["ts"].as_str().unwrap_or("");

                        // Skip bot messages and empty
                        if user == bot_user_id || user.is_empty() || text.is_empty() {
                            continue;
                        }

                        if !helpers::is_user_allowed(user, &self.allowed_users) {
                            continue;
                        }

                        latest_ts = ts.to_string();

                        let thread_ts = msg["thread_ts"].as_str().map(String::from);

                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: user.to_string(),
                            reply_target: channel_id.to_string(),
                            content: text.to_string(),
                            channel: "slack".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts,
                            privacy_boundary: String::new(),
                            attachments: Vec::new(),
                        };

                        if tx.send(channel_msg).await.is_err() {
                            return Ok(());
                        }
                    }
                }

                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .post(format!("{API_BASE}/auth.test"))
                .bearer_auth(&self.bot_token)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }
    }

    impl SlackChannel {
        /// Listen via Slack Socket Mode (WebSocket).
        async fn listen_socket_mode(
            &self,
            app_token: &str,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            use futures_util::{SinkExt, StreamExt};
            use tokio_tungstenite::tungstenite::Message;

            let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();

            // Get WebSocket URL via apps.connections.open
            let resp = self
                .client
                .post(format!("{API_BASE}/apps.connections.open"))
                .bearer_auth(app_token)
                .send()
                .await?;
            let json: serde_json::Value = resp.json().await?;
            let ws_url = json["url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("failed to get slack socket mode URL"))?;

            let (ws_stream, _) = tokio_tungstenite::connect_async(ws_url).await?;
            let (mut write, mut read) = ws_stream.split();

            while let Some(msg_result) = read.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!(error = %e, "slack socket mode error");
                        break;
                    }
                };

                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break,
                    _ => continue,
                };

                let event: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Acknowledge the envelope
                if let Some(envelope_id) = event["envelope_id"].as_str() {
                    let ack = serde_json::json!({"envelope_id": envelope_id});
                    let _ = write.send(Message::Text(ack.to_string())).await;
                }

                let event_type = event["type"].as_str().unwrap_or("");
                if event_type != "events_api" {
                    continue;
                }

                let payload = &event["payload"];
                let inner_event = &payload["event"];
                let inner_type = inner_event["type"].as_str().unwrap_or("");

                if inner_type != "message" {
                    continue;
                }

                // Skip bot messages
                let user = inner_event["user"].as_str().unwrap_or("");
                if user == bot_user_id || user.is_empty() {
                    continue;
                }
                if inner_event["subtype"].as_str().is_some() {
                    continue;
                }

                if !helpers::is_user_allowed(user, &self.allowed_users) {
                    continue;
                }

                let content = inner_event["text"].as_str().unwrap_or("").to_string();
                if content.is_empty() {
                    continue;
                }

                let channel_id = inner_event["channel"].as_str().unwrap_or("").to_string();
                let thread_ts = inner_event["thread_ts"].as_str().map(String::from);

                let channel_msg = ChannelMessage {
                    id: helpers::new_message_id(),
                    sender: user.to_string(),
                    reply_target: channel_id,
                    content,
                    channel: "slack".to_string(),
                    timestamp: helpers::now_epoch_secs(),
                    thread_ts,
                    privacy_boundary: String::new(),
                    attachments: Vec::new(),
                };

                if tx.send(channel_msg).await.is_err() {
                    break;
                }
            }

            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn slack_channel_name() {
            let ch = SlackChannel::new("xoxb-test".into(), None, None, vec![]);
            assert_eq!(ch.name(), "slack");
        }
    }
}

#[cfg(feature = "channel-slack")]
pub use impl_::*;

#[cfg(not(feature = "channel-slack"))]
super::channel_stub!(SlackChannel, SLACK_DESCRIPTOR, "slack", "Slack");

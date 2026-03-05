#[cfg(feature = "channel-discord")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    super::super::channel_meta!(DISCORD_DESCRIPTOR, "discord", "Discord");

    const GATEWAY_URL: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
    const API_BASE: &str = "https://discord.com/api/v10";
    const MAX_MESSAGE_LENGTH: usize = 2000;
    // Intents: GUILD_MESSAGES (1<<9) | DIRECT_MESSAGES (1<<12) | MESSAGE_CONTENT (1<<15)
    const INTENTS: u64 = (1 << 9) | (1 << 12) | (1 << 15);

    pub struct DiscordChannel {
        bot_token: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl DiscordChannel {
        pub fn new(bot_token: String, allowed_users: Vec<String>) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                bot_token,
                allowed_users,
                client,
            }
        }
    }

    #[async_trait]
    impl Channel for DiscordChannel {
        fn name(&self) -> &str {
            "discord"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({ "content": chunk });
                let resp = self
                    .client
                    .post(format!("{API_BASE}/channels/{}/messages", message.recipient))
                    .header("Authorization", format!("Bot {}", self.bot_token))
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("discord send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let (ws_stream, _) =
                tokio_tungstenite::connect_async(GATEWAY_URL).await?;
            let (mut write, mut read) = ws_stream.split();

            // Read Hello event to get heartbeat interval
            let hello = read
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("discord gateway closed before Hello"))??;
            let hello_json: serde_json::Value = match hello {
                Message::Text(text) => serde_json::from_str(&text)?,
                _ => anyhow::bail!("expected text Hello from discord gateway"),
            };
            let heartbeat_interval_ms = hello_json["d"]["heartbeat_interval"]
                .as_u64()
                .unwrap_or(41250);

            // Send Identify
            let identify = serde_json::json!({
                "op": 2,
                "d": {
                    "token": self.bot_token,
                    "intents": INTENTS,
                    "properties": {
                        "os": std::env::consts::OS,
                        "browser": "agentzero",
                        "device": "agentzero",
                    }
                }
            });
            write
                .send(Message::Text(identify.to_string()))
                .await?;

            // Track sequence number for heartbeats
            let sequence = Arc::new(AtomicI64::new(-1));
            let seq_clone = sequence.clone();

            // Spawn heartbeat task
            let mut heartbeat_write = write;
            let heartbeat_handle = tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_millis(heartbeat_interval_ms));
                loop {
                    interval.tick().await;
                    let seq = seq_clone.load(Ordering::Relaxed);
                    let payload = if seq < 0 {
                        serde_json::json!({"op": 1, "d": null})
                    } else {
                        serde_json::json!({"op": 1, "d": seq})
                    };
                    if heartbeat_write
                        .send(Message::Text(payload.to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            });

            // Get our own user ID to ignore self-messages
            let me_resp = self
                .client
                .get(format!("{API_BASE}/users/@me"))
                .header("Authorization", format!("Bot {}", self.bot_token))
                .send()
                .await?;
            let me: serde_json::Value = me_resp.json().await?;
            let bot_user_id = me["id"].as_str().unwrap_or("").to_string();

            // Read events
            while let Some(msg_result) = read.next().await {
                let msg = match msg_result {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::error!(error = %e, "discord websocket error");
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

                // Update sequence
                if let Some(s) = event["s"].as_i64() {
                    sequence.store(s, Ordering::Relaxed);
                }

                let op = event["op"].as_u64().unwrap_or(0);
                let event_type = event["t"].as_str().unwrap_or("");

                match op {
                    0 if event_type == "MESSAGE_CREATE" => {
                        let d = &event["d"];
                        let author_id = d["author"]["id"].as_str().unwrap_or("");
                        let is_bot = d["author"]["bot"].as_bool().unwrap_or(false);

                        // Skip bot's own messages
                        if author_id == bot_user_id || is_bot {
                            continue;
                        }

                        if !helpers::is_user_allowed(author_id, &self.allowed_users) {
                            continue;
                        }

                        let content = d["content"].as_str().unwrap_or("").to_string();
                        if content.is_empty() {
                            continue;
                        }

                        let channel_id = d["channel_id"].as_str().unwrap_or("").to_string();

                        let msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: author_id.to_string(),
                            reply_target: channel_id,
                            content,
                            channel: "discord".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: None,
                            privacy_boundary: String::new(),
                        };

                        if tx.send(msg).await.is_err() {
                            break;
                        }
                    }
                    7 => {
                        // Reconnect requested
                        tracing::info!("discord gateway requested reconnect");
                        break;
                    }
                    9 => {
                        // Invalid session
                        tracing::warn!("discord invalid session");
                        break;
                    }
                    _ => {}
                }
            }

            heartbeat_handle.abort();
            Ok(())
        }

        async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
            let _ = self
                .client
                .post(format!("{API_BASE}/channels/{recipient}/typing"))
                .header("Authorization", format!("Bot {}", self.bot_token))
                .send()
                .await;
            Ok(())
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(format!("{API_BASE}/users/@me"))
                .header("Authorization", format!("Bot {}", self.bot_token))
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
        fn discord_channel_name() {
            let ch = DiscordChannel::new("test-token".into(), vec![]);
            assert_eq!(ch.name(), "discord");
        }
    }
}

#[cfg(feature = "channel-discord")]
pub use impl_::*;

#[cfg(not(feature = "channel-discord"))]
super::channel_stub!(DiscordChannel, DISCORD_DESCRIPTOR, "discord", "Discord");

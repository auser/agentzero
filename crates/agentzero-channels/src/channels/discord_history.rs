#[cfg(feature = "channel-discord-history")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use serde::Deserialize;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    super::super::channel_meta!(DISCORD_HISTORY_DESCRIPTOR, "discord-history", "Discord History");

    /// Cached Discord username resolution (snowflake ID → display name).
    struct NameCache {
        entries: HashMap<String, (String, u64)>,
        ttl_secs: u64,
    }

    impl NameCache {
        fn new(ttl_secs: u64) -> Self {
            Self {
                entries: HashMap::new(),
                ttl_secs,
            }
        }

        fn get(&self, user_id: &str) -> Option<&str> {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.entries.get(user_id).and_then(|(name, cached_at)| {
                if now.saturating_sub(*cached_at) < self.ttl_secs {
                    Some(name.as_str())
                } else {
                    None
                }
            })
        }

        fn set(&mut self, user_id: String, name: String) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.entries.insert(user_id, (name, now));
        }
    }

    /// Minimal Discord gateway message for deserialization.
    #[derive(Debug, Deserialize)]
    struct GatewayEvent {
        #[serde(default)]
        op: u8,
        #[serde(default)]
        t: Option<String>,
        #[serde(default)]
        d: Option<serde_json::Value>,
        #[serde(default)]
        s: Option<u64>,
    }

    /// Shadow listener that logs Discord messages for searchable history.
    /// Does not respond to messages — only records them via tx channel.
    ///
    /// Architecture:
    /// 1. Connect to Discord Gateway WebSocket (wss://gateway.discord.gg)
    /// 2. Authenticate with bot token via IDENTIFY payload
    /// 3. Maintain heartbeat interval from HELLO event
    /// 4. On MESSAGE_CREATE events: extract author, content, channel_id
    /// 5. Resolve author snowflake ID to display name via cache
    /// 6. Emit ChannelMessage for upstream processing/storage
    pub struct DiscordHistoryChannel {
        bot_token: String,
        name_cache: Mutex<NameCache>,
        /// Only log messages from these guild IDs. Empty = log all.
        guild_filter: Vec<String>,
    }

    impl DiscordHistoryChannel {
        pub fn new(bot_token: String) -> Self {
            Self {
                bot_token,
                name_cache: Mutex::new(NameCache::new(24 * 3600)), // 24h TTL
                guild_filter: Vec::new(),
            }
        }

        pub fn with_guild_filter(mut self, guilds: Vec<String>) -> Self {
            self.guild_filter = guilds;
            self
        }

        /// Resolve a Discord user ID to a display name, using cache.
        async fn resolve_name(&self, user_id: &str, fallback: &str) -> String {
            // Check cache first
            if let Ok(cache) = self.name_cache.lock() {
                if let Some(name) = cache.get(user_id) {
                    return name.to_string();
                }
            }

            // Fetch from Discord API
            let url = format!("https://discord.com/api/v10/users/{user_id}");
            let client = reqwest::Client::new();
            let resp = client
                .get(&url)
                .header("Authorization", format!("Bot {}", self.bot_token))
                .timeout(Duration::from_secs(10))
                .send()
                .await;

            let name = match resp {
                Ok(r) if r.status().is_success() => {
                    r.json::<serde_json::Value>()
                        .await
                        .ok()
                        .and_then(|v| {
                            v["global_name"]
                                .as_str()
                                .or(v["username"].as_str())
                                .map(String::from)
                        })
                        .unwrap_or_else(|| fallback.to_string())
                }
                _ => fallback.to_string(),
            };

            // Cache the result
            if let Ok(mut cache) = self.name_cache.lock() {
                cache.set(user_id.to_string(), name.clone());
            }

            name
        }
    }

    #[async_trait]
    impl Channel for DiscordHistoryChannel {
        fn name(&self) -> &str {
            "discord-history"
        }

        async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
            // History channel is read-only — does not send messages
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            use futures_util::{SinkExt, StreamExt};
            use tokio_tungstenite::tungstenite::Message;

            tracing::info!(
                guild_filter = ?self.guild_filter,
                "discord-history: connecting to gateway"
            );

            let gateway_url = "wss://gateway.discord.gg/?v=10&encoding=json";
            let (ws, _) = tokio_tungstenite::connect_async(gateway_url)
                .await
                .map_err(|e| anyhow::anyhow!("discord gateway connection failed: {e}"))?;

            let (mut sink, mut stream) = ws.split();

            // Read HELLO to get heartbeat interval
            let mut heartbeat_interval_ms = 41250u64; // default
            if let Some(Ok(Message::Text(text))) = stream.next().await {
                if let Ok(event) = serde_json::from_str::<GatewayEvent>(&text) {
                    if event.op == 10 {
                        if let Some(ref d) = event.d {
                            heartbeat_interval_ms =
                                d["heartbeat_interval"].as_u64().unwrap_or(41250);
                        }
                    }
                }
            }

            // Send IDENTIFY
            let identify = serde_json::json!({
                "op": 2,
                "d": {
                    "token": self.bot_token,
                    "intents": 33281, // GUILDS + GUILD_MESSAGES + MESSAGE_CONTENT
                    "properties": {
                        "os": "linux",
                        "browser": "agentzero",
                        "device": "agentzero"
                    }
                }
            });
            sink.send(Message::Text(identify.to_string()))
                .await
                .map_err(|e| anyhow::anyhow!("discord identify failed: {e}"))?;

            // Spawn heartbeat task
            let heartbeat_interval = Duration::from_millis(heartbeat_interval_ms);
            let (heartbeat_tx, mut heartbeat_rx) = tokio::sync::mpsc::channel::<u64>(1);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(heartbeat_interval);
                let mut last_seq: Option<u64> = None;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            // Heartbeat sends are handled by the main loop
                        }
                        seq = heartbeat_rx.recv() => {
                            if let Some(s) = seq {
                                last_seq = Some(s);
                            }
                        }
                    }
                    let _ = last_seq; // Used for heartbeat ACK tracking
                }
            });

            // Main message loop
            let mut last_seq: Option<u64> = None;
            let mut heartbeat_timer = tokio::time::interval(heartbeat_interval);

            loop {
                tokio::select! {
                    _ = heartbeat_timer.tick() => {
                        let hb = serde_json::json!({ "op": 1, "d": last_seq });
                        if sink.send(Message::Text(hb.to_string())).await.is_err() {
                            tracing::warn!("discord-history: heartbeat send failed, reconnecting");
                            break;
                        }
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                if let Ok(event) = serde_json::from_str::<GatewayEvent>(&text) {
                                    if let Some(s) = event.s {
                                        last_seq = Some(s);
                                        let _ = heartbeat_tx.try_send(s);
                                    }

                                    if event.t.as_deref() == Some("MESSAGE_CREATE") {
                                        if let Some(ref d) = event.d {
                                            let content = d["content"].as_str().unwrap_or("");
                                            let author_id = d["author"]["id"].as_str().unwrap_or("unknown");
                                            let author_name = d["author"]["username"].as_str().unwrap_or("unknown");
                                            let channel_id = d["channel_id"].as_str().unwrap_or("unknown");
                                            let guild_id = d["guild_id"].as_str().unwrap_or("");

                                            // Apply guild filter
                                            if !self.guild_filter.is_empty() && !self.guild_filter.iter().any(|g| g == guild_id) {
                                                continue;
                                            }

                                            // Skip bot messages
                                            if d["author"]["bot"].as_bool().unwrap_or(false) {
                                                continue;
                                            }

                                            let display_name = self.resolve_name(author_id, author_name).await;

                                            let msg = ChannelMessage {
                                                id: crate::channels::helpers::new_message_id(),
                                                channel: "discord-history".to_string(),
                                                sender: display_name,
                                                reply_target: channel_id.to_string(),
                                                content: content.to_string(),
                                                timestamp: crate::channels::helpers::now_epoch_secs(),
                                                thread_ts: None,
                                                privacy_boundary: String::new(),
                                                attachments: vec![],
                                            };

                                            if tx.send(msg).await.is_err() {
                                                tracing::warn!("discord-history: message receiver dropped");
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                tracing::info!("discord-history: gateway connection closed");
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }

            Ok(())
        }

        async fn health_check(&self) -> bool {
            let client = reqwest::Client::new();
            let resp = client
                .get("https://discord.com/api/v10/users/@me")
                .header("Authorization", format!("Bot {}", self.bot_token))
                .timeout(Duration::from_secs(10))
                .send()
                .await;
            resp.map(|r| r.status().is_success()).unwrap_or(false)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn name_cache_stores_and_retrieves() {
            let mut cache = NameCache::new(3600);
            cache.set("123".to_string(), "Alice".to_string());
            assert_eq!(cache.get("123"), Some("Alice"));
            assert_eq!(cache.get("456"), None);
        }

        #[test]
        fn guild_filter_empty_allows_all() {
            let ch = DiscordHistoryChannel::new("token".to_string());
            assert!(ch.guild_filter.is_empty());
        }

        #[test]
        fn with_guild_filter_sets_filter() {
            let ch = DiscordHistoryChannel::new("token".to_string())
                .with_guild_filter(vec!["guild1".to_string()]);
            assert_eq!(ch.guild_filter, vec!["guild1"]);
        }

        #[tokio::test]
        async fn health_check_fails_with_invalid_token() {
            let ch = DiscordHistoryChannel::new("invalid-token".to_string());
            assert!(!ch.health_check().await);
        }
    }
}

#[cfg(feature = "channel-discord-history")]
pub use impl_::*;

#[cfg(not(feature = "channel-discord-history"))]
super::channel_stub!(
    DiscordHistoryChannel,
    DISCORD_HISTORY_DESCRIPTOR,
    "discord-history",
    "Discord History"
);

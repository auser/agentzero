#[cfg(feature = "channel-nostr")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use futures_util::{SinkExt, StreamExt};
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    super::super::channel_meta!(NOSTR_DESCRIPTOR, "nostr", "Nostr");

    const MAX_MESSAGE_LENGTH: usize = 65536;

    /// Nostr channel — communicates via NIP-01 relays over WebSocket.
    pub struct NostrChannel {
        relay_url: String,
        private_key_hex: String,
        allowed_pubkeys: Vec<String>,
    }

    impl NostrChannel {
        pub fn new(
            relay_url: String,
            private_key_hex: String,
            allowed_pubkeys: Vec<String>,
        ) -> Self {
            Self {
                relay_url,
                private_key_hex,
                allowed_pubkeys,
            }
        }
    }

    #[async_trait]
    impl Channel for NostrChannel {
        fn name(&self) -> &str {
            "nostr"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            let (ws_stream, _) =
                tokio_tungstenite::connect_async(&self.relay_url).await?;
            let (mut write, _read) = ws_stream.split();

            for chunk in chunks {
                let created_at = helpers::now_epoch_secs();
                // NIP-01 kind 1 (text note) event — simplified without real signing.
                // A production implementation would compute the event id hash and
                // sign with the private key. We include the private_key_hex field
                // for future use and send a minimal relay-compatible payload.
                let event = serde_json::json!({
                    "id": helpers::new_message_id(),
                    "pubkey": self.private_key_hex,
                    "created_at": created_at,
                    "kind": 1,
                    "tags": [["p", message.recipient]],
                    "content": chunk,
                    "sig": "",
                });
                let relay_msg =
                    serde_json::json!(["EVENT", event]).to_string();
                write.send(WsMessage::Text(relay_msg)).await?;
            }
            write.close().await?;
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            loop {
                let (ws_stream, _) = match tokio_tungstenite::connect_async(
                    &self.relay_url,
                )
                .await
                {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!(error = %e, "nostr: relay connect failed");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                };

                let (mut write, mut read) = ws_stream.split();

                // Subscribe to kind-1 text notes mentioning our pubkey.
                let sub = serde_json::json!([
                    "REQ",
                    "sub1",
                    {
                        "kinds": [1],
                        "#p": [self.private_key_hex],
                        "since": helpers::now_epoch_secs()
                    }
                ]);
                if write
                    .send(WsMessage::Text(sub.to_string()))
                    .await
                    .is_err()
                {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }

                while let Some(msg) = read.next().await {
                    let text = match msg {
                        Ok(WsMessage::Text(t)) => t.to_string(),
                        Ok(WsMessage::Close(_)) => break,
                        Err(e) => {
                            tracing::error!(error = %e, "nostr: ws error");
                            break;
                        }
                        _ => continue,
                    };

                    let parsed: serde_json::Value =
                        match serde_json::from_str(&text) {
                            Ok(v) => v,
                            Err(_) => continue,
                        };

                    if parsed[0].as_str() != Some("EVENT") {
                        continue;
                    }

                    let event = &parsed[2];
                    let pubkey = event["pubkey"].as_str().unwrap_or("");
                    if pubkey.is_empty() || pubkey == self.private_key_hex {
                        continue;
                    }
                    if !helpers::is_user_allowed(pubkey, &self.allowed_pubkeys) {
                        continue;
                    }
                    let content = event["content"].as_str().unwrap_or("");
                    if content.is_empty() {
                        continue;
                    }

                    let channel_msg = ChannelMessage {
                        id: helpers::new_message_id(),
                        sender: pubkey.to_string(),
                        reply_target: pubkey.to_string(),
                        content: content.to_string(),
                        channel: "nostr".to_string(),
                        timestamp: helpers::now_epoch_secs(),
                        thread_ts: None,
                        privacy_boundary: String::new(),
                    };
                    if tx.send(channel_msg).await.is_err() {
                        return Ok(());
                    }
                }

                tracing::warn!("nostr: relay connection lost, reconnecting...");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }

        async fn health_check(&self) -> bool {
            tokio_tungstenite::connect_async(&self.relay_url)
                .await
                .is_ok()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn nostr_channel_name() {
            let ch = NostrChannel::new(
                "wss://relay.example.com".into(),
                "deadbeef".into(),
                vec![],
            );
            assert_eq!(ch.name(), "nostr");
        }
    }
}

#[cfg(feature = "channel-nostr")]
pub use impl_::*;

#[cfg(not(feature = "channel-nostr"))]
super::channel_stub!(NostrChannel, NOSTR_DESCRIPTOR, "nostr", "Nostr");

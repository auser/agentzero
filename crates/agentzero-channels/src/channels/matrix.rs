#[cfg(feature = "channel-matrix")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(MATRIX_DESCRIPTOR, "matrix", "Matrix");

    const POLL_TIMEOUT_SECS: u64 = 30;
    const MAX_MESSAGE_LENGTH: usize = 65536;

    pub struct MatrixChannel {
        homeserver: String,
        access_token: String,
        room_id: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl MatrixChannel {
        pub fn new(
            homeserver: String,
            access_token: String,
            room_id: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(POLL_TIMEOUT_SECS + 10))
                .build()
                .expect("reqwest client should build");
            Self {
                homeserver: homeserver.trim_end_matches('/').to_string(),
                access_token,
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
            format!("{}/_matrix/client/v3{}", self.homeserver, path)
        }
    }

    #[async_trait]
    impl Channel for MatrixChannel {
        fn name(&self) -> &str {
            "matrix"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let txn_id = helpers::new_message_id();
                let url = self.api_url(&format!(
                    "/rooms/{}/send/m.room.message/{txn_id}",
                    message.recipient
                ));
                let body = serde_json::json!({"msgtype": "m.text", "body": chunk});
                let resp = self.client.put(&url).bearer_auth(&self.access_token).json(&body).send().await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("matrix send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
            let whoami: serde_json::Value = self.client.get(self.api_url("/account/whoami"))
                .bearer_auth(&self.access_token).send().await?.json().await?;
            let my_user_id = whoami["user_id"].as_str().unwrap_or("").to_string();
            let mut since = String::new();

            loop {
                let mut url = format!("{}/_matrix/client/v3/sync?timeout={}", self.homeserver, POLL_TIMEOUT_SECS * 1000);
                if !since.is_empty() { url.push_str(&format!("&since={since}")); }
                let resp = match self.client.get(&url).bearer_auth(&self.access_token).send().await {
                    Ok(r) => r,
                    Err(e) => { tracing::error!(error = %e, "matrix sync failed"); tokio::time::sleep(Duration::from_secs(2)).await; continue; }
                };
                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => { tracing::error!(error = %e, "matrix sync parse failed"); tokio::time::sleep(Duration::from_secs(2)).await; continue; }
                };
                if let Some(token) = json["next_batch"].as_str() { since = token.to_string(); }
                if let Some(rooms) = json["rooms"]["join"].as_object() {
                    for (room_id, room_data) in rooms {
                        if let Some(events) = room_data["timeline"]["events"].as_array() {
                            for event in events {
                                if event["type"].as_str() != Some("m.room.message") { continue; }
                                let sender = event["sender"].as_str().unwrap_or("");
                                if sender == my_user_id || sender.is_empty() { continue; }
                                if !helpers::is_user_allowed(sender, &self.allowed_users) { continue; }
                                let body = event["content"]["body"].as_str().unwrap_or("");
                                if body.is_empty() { continue; }
                                let msg = ChannelMessage { id: helpers::new_message_id(), sender: sender.to_string(), reply_target: room_id.to_string(), content: body.to_string(), channel: "matrix".to_string(), timestamp: helpers::now_epoch_secs(), thread_ts: None, privacy_boundary: String::new() };
                                if tx.send(msg).await.is_err() { return Ok(()); }
                            }
                        }
                    }
                }
            }
        }

        async fn health_check(&self) -> bool {
            self.client.get(self.api_url("/account/whoami")).bearer_auth(&self.access_token).send().await.map(|r| r.status().is_success()).unwrap_or(false)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn matrix_channel_name() {
            let ch = MatrixChannel::new("https://m.example.com".into(), "t".into(), "!r:e".into(), vec![]);
            assert_eq!(ch.name(), "matrix");
        }

        #[test]
        fn matrix_api_url_strips_slash() {
            let ch = MatrixChannel::new("https://m.example.com/".into(), "t".into(), "!r:e".into(), vec![]);
            assert_eq!(ch.api_url("/sync"), "https://m.example.com/_matrix/client/v3/sync");
        }
    }
}

#[cfg(feature = "channel-matrix")]
pub use impl_::*;

#[cfg(not(feature = "channel-matrix"))]
super::channel_stub!(MatrixChannel, MATRIX_DESCRIPTOR, "matrix", "Matrix");

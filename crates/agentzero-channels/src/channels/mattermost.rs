#[cfg(feature = "channel-mattermost")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(MATTERMOST_DESCRIPTOR, "mattermost", "Mattermost");

    const MAX_MESSAGE_LENGTH: usize = 16383;
    const POLL_INTERVAL_SECS: u64 = 3;

    pub struct MattermostChannel {
        base_url: String,
        token: String,
        channel_id: Option<String>,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl MattermostChannel {
        pub fn new(
            base_url: String,
            token: String,
            channel_id: Option<String>,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                token,
                channel_id,
                allowed_users,
                client,
            }
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}/api/v4{}", self.base_url, path)
        }

        async fn get_bot_user_id(&self) -> anyhow::Result<String> {
            let resp = self
                .client
                .get(self.api_url("/users/me"))
                .bearer_auth(&self.token)
                .send()
                .await?;
            let json: serde_json::Value = resp.json().await?;
            json["id"]
                .as_str()
                .map(String::from)
                .ok_or_else(|| anyhow::anyhow!("failed to get mattermost bot user id"))
        }
    }

    #[async_trait]
    impl Channel for MattermostChannel {
        fn name(&self) -> &str {
            "mattermost"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let mut body = serde_json::json!({
                    "channel_id": message.recipient,
                    "message": chunk,
                });
                if let Some(ref root_id) = message.thread_ts {
                    body["root_id"] = serde_json::json!(root_id);
                }

                let resp = self
                    .client
                    .post(self.api_url("/posts"))
                    .bearer_auth(&self.token)
                    .json(&body)
                    .send()
                    .await?;

                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("mattermost create post failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let channel_id = self
                .channel_id
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("mattermost channel_id required for polling"))?;

            let bot_user_id = self.get_bot_user_id().await.unwrap_or_default();
            let mut last_post_time: u64 = helpers::now_epoch_secs() * 1000;

            loop {
                let url = self.api_url(&format!(
                    "/channels/{channel_id}/posts?since={last_post_time}&per_page=25"
                ));

                let resp = match self
                    .client
                    .get(&url)
                    .bearer_auth(&self.token)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "mattermost get posts failed");
                        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                        continue;
                    }
                };

                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "mattermost parse failed");
                        tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
                        continue;
                    }
                };

                // Mattermost returns { order: [...ids], posts: { id: post } }
                if let Some(order) = json["order"].as_array() {
                    let posts = &json["posts"];
                    for post_id in order {
                        let post_id = post_id.as_str().unwrap_or("");
                        let post = &posts[post_id];

                        let user_id = post["user_id"].as_str().unwrap_or("");
                        let message = post["message"].as_str().unwrap_or("");
                        let create_at = post["create_at"].as_u64().unwrap_or(0);

                        if user_id == bot_user_id || user_id.is_empty() || message.is_empty() {
                            continue;
                        }

                        if create_at <= last_post_time {
                            continue;
                        }

                        if !helpers::is_user_allowed(user_id, &self.allowed_users) {
                            continue;
                        }

                        last_post_time = create_at;

                        let root_id = post["root_id"].as_str().filter(|s| !s.is_empty());

                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: user_id.to_string(),
                            reply_target: channel_id.to_string(),
                            content: message.to_string(),
                            channel: "mattermost".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: root_id.map(String::from),
                            privacy_boundary: String::new(),
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
                .get(self.api_url("/users/me"))
                .bearer_auth(&self.token)
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }

        async fn start_typing(&self, recipient: &str) -> anyhow::Result<()> {
            // Mattermost WebSocket typing indicator requires WS connection.
            // For HTTP-only mode, we use the HTTP userTyping action.
            let body = serde_json::json!({
                "channel_id": recipient,
            });
            let _ = self
                .client
                .post(self.api_url("/users/me/typing"))
                .bearer_auth(&self.token)
                .json(&body)
                .send()
                .await;
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn mattermost_channel_name() {
            let ch = MattermostChannel::new(
                "https://mm.example.com".into(),
                "test-token".into(),
                None,
                vec![],
            );
            assert_eq!(ch.name(), "mattermost");
        }

        #[test]
        fn mattermost_api_url_format() {
            let ch = MattermostChannel::new(
                "https://mm.example.com".into(),
                "tok".into(),
                None,
                vec![],
            );
            assert_eq!(
                ch.api_url("/posts"),
                "https://mm.example.com/api/v4/posts"
            );
        }

        #[test]
        fn mattermost_api_url_strips_trailing_slash() {
            let ch = MattermostChannel::new(
                "https://mm.example.com/".into(),
                "tok".into(),
                None,
                vec![],
            );
            assert_eq!(
                ch.api_url("/users/me"),
                "https://mm.example.com/api/v4/users/me"
            );
        }
    }
}

#[cfg(feature = "channel-mattermost")]
pub use impl_::*;

#[cfg(not(feature = "channel-mattermost"))]
super::channel_stub!(MattermostChannel, MATTERMOST_DESCRIPTOR, "mattermost", "Mattermost");

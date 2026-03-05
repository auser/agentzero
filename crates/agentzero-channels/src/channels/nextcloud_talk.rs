#[cfg(feature = "channel-nextcloud-talk")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(NEXTCLOUD_TALK_DESCRIPTOR, "nextcloud-talk", "NextCloud Talk");

    const MAX_MESSAGE_LENGTH: usize = 32000;
    const POLL_INTERVAL_SECS: u64 = 3;

    /// Nextcloud Talk (Spreed) channel via OCS API.
    pub struct NextcloudTalkChannel {
        base_url: String,
        username: String,
        password: String,
        room_token: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl NextcloudTalkChannel {
        pub fn new(
            base_url: String,
            username: String,
            password: String,
            room_token: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                username,
                password,
                room_token,
                allowed_users,
                client,
            }
        }

        fn api_url(&self, path: &str) -> String {
            format!(
                "{}/ocs/v2.php/apps/spreed/api/v1{}",
                self.base_url, path
            )
        }
    }

    #[async_trait]
    impl Channel for NextcloudTalkChannel {
        fn name(&self) -> &str {
            "nextcloud-talk"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let url = self.api_url(&format!("/chat/{}", self.room_token));
                let body = serde_json::json!({"message": chunk});
                let resp = self
                    .client
                    .post(&url)
                    .basic_auth(&self.username, Some(&self.password))
                    .header("OCS-APIRequest", "true")
                    .header("Accept", "application/json")
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("nextcloud-talk send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let mut last_known_id: i64 = 0;
            loop {
                let url = self.api_url(&format!(
                    "/chat/{}?lookIntoFuture=1&timeout={POLL_INTERVAL_SECS}&setReadMarker=0&lastKnownMessageId={last_known_id}",
                    self.room_token
                ));
                let resp = match self
                    .client
                    .get(&url)
                    .basic_auth(&self.username, Some(&self.password))
                    .header("OCS-APIRequest", "true")
                    .header("Accept", "application/json")
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "nextcloud-talk poll failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };

                if resp.status() == 304 {
                    continue;
                }

                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "nextcloud-talk parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };

                if let Some(messages) = json["ocs"]["data"].as_array() {
                    for msg in messages {
                        let id = msg["id"].as_i64().unwrap_or(0);
                        if id > last_known_id {
                            last_known_id = id;
                        }
                        let actor = msg["actorId"].as_str().unwrap_or("");
                        if actor == self.username || actor.is_empty() {
                            continue;
                        }
                        if !helpers::is_user_allowed(actor, &self.allowed_users) {
                            continue;
                        }
                        let text = msg["message"].as_str().unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: actor.to_string(),
                            reply_target: self.room_token.clone(),
                            content: text.to_string(),
                            channel: "nextcloud-talk".to_string(),
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
            let url = self.api_url(&format!("/room/{}", self.room_token));
            self.client
                .get(&url)
                .basic_auth(&self.username, Some(&self.password))
                .header("OCS-APIRequest", "true")
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
        fn nextcloud_talk_channel_name() {
            let ch = NextcloudTalkChannel::new(
                "https://nc.example.com".into(),
                "user".into(),
                "pass".into(),
                "abc123".into(),
                vec![],
            );
            assert_eq!(ch.name(), "nextcloud-talk");
        }

        #[test]
        fn nextcloud_talk_api_url_format() {
            let ch = NextcloudTalkChannel::new(
                "https://nc.example.com/".into(),
                "u".into(),
                "p".into(),
                "r".into(),
                vec![],
            );
            assert_eq!(
                ch.api_url("/chat/room1"),
                "https://nc.example.com/ocs/v2.php/apps/spreed/api/v1/chat/room1"
            );
        }
    }
}

#[cfg(feature = "channel-nextcloud-talk")]
pub use impl_::*;

#[cfg(not(feature = "channel-nextcloud-talk"))]
super::channel_stub!(
    NextcloudTalkChannel,
    NEXTCLOUD_TALK_DESCRIPTOR,
    "nextcloud-talk",
    "NextCloud Talk"
);

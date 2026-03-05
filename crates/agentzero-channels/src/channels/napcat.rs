#[cfg(feature = "channel-napcat")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(NAPCAT_DESCRIPTOR, "napcat", "Napcat (QQ via OneBot)");

    const MAX_MESSAGE_LENGTH: usize = 4500;
    const POLL_INTERVAL_SECS: u64 = 2;

    /// Napcat channel (QQ via OneBot v11 HTTP API).
    pub struct NapcatChannel {
        base_url: String,
        access_token: Option<String>,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl NapcatChannel {
        pub fn new(
            base_url: String,
            access_token: Option<String>,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                access_token,
                allowed_users,
                client,
            }
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}{}", self.base_url, path)
        }

        fn add_auth(
            &self,
            req: reqwest::RequestBuilder,
        ) -> reqwest::RequestBuilder {
            if let Some(token) = &self.access_token {
                req.bearer_auth(token)
            } else {
                req
            }
        }
    }

    #[async_trait]
    impl Channel for NapcatChannel {
        fn name(&self) -> &str {
            "napcat"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            let (msg_type, target_id) =
                if let Some(gid) = message.recipient.strip_prefix("group:") {
                    ("group", gid)
                } else {
                    (
                        "private",
                        message
                            .recipient
                            .strip_prefix("user:")
                            .unwrap_or(&message.recipient),
                    )
                };
            for chunk in chunks {
                let body = if msg_type == "group" {
                    serde_json::json!({
                        "group_id": target_id.parse::<i64>().unwrap_or(0),
                        "message": [{"type": "text", "data": {"text": chunk}}],
                    })
                } else {
                    serde_json::json!({
                        "user_id": target_id.parse::<i64>().unwrap_or(0),
                        "message": [{"type": "text", "data": {"text": chunk}}],
                    })
                };
                let endpoint = if msg_type == "group" {
                    "/send_group_msg"
                } else {
                    "/send_private_msg"
                };
                let req = self.client.post(self.api_url(endpoint)).json(&body);
                let resp = self.add_auth(req).send().await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("napcat send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            loop {
                let req = self
                    .client
                    .post(self.api_url("/get_latest_events"))
                    .json(&serde_json::json!({"limit": 20}));
                let resp = match self.add_auth(req).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "napcat poll failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "napcat parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                if let Some(events) = json["data"].as_array() {
                    for event in events {
                        if event["post_type"].as_str() != Some("message") {
                            continue;
                        }
                        let sender = event["sender"]["user_id"]
                            .as_i64()
                            .map(|id| id.to_string())
                            .unwrap_or_default();
                        if sender.is_empty() {
                            continue;
                        }
                        if !helpers::is_user_allowed(&sender, &self.allowed_users) {
                            continue;
                        }
                        let text = event["raw_message"].as_str().unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let reply_target =
                            if event["message_type"].as_str() == Some("group") {
                                format!(
                                    "group:{}",
                                    event["group_id"].as_i64().unwrap_or(0)
                                )
                            } else {
                                format!("user:{sender}")
                            };
                        let msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender,
                            reply_target,
                            content: text.to_string(),
                            channel: "napcat".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: None,
                            privacy_boundary: String::new(),
                        };
                        if tx.send(msg).await.is_err() {
                            return Ok(());
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            let req = self.client.get(self.api_url("/get_status"));
            self.add_auth(req)
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
        fn napcat_channel_name() {
            let ch = NapcatChannel::new("http://localhost:3000".into(), None, vec![]);
            assert_eq!(ch.name(), "napcat");
        }

        #[test]
        fn napcat_api_url_format() {
            let ch =
                NapcatChannel::new("http://localhost:3000/".into(), None, vec![]);
            assert_eq!(
                ch.api_url("/send_private_msg"),
                "http://localhost:3000/send_private_msg"
            );
        }
    }
}

#[cfg(feature = "channel-napcat")]
pub use impl_::*;

#[cfg(not(feature = "channel-napcat"))]
super::channel_stub!(NapcatChannel, NAPCAT_DESCRIPTOR, "napcat", "Napcat (QQ via OneBot)");

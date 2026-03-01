#[cfg(feature = "channel-acp")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(ACP_DESCRIPTOR, "acp", "ACP (Agent Client Protocol)");

    const MAX_MESSAGE_LENGTH: usize = 65536;
    const POLL_INTERVAL_SECS: u64 = 2;

    /// Agent Client Protocol (ACP) channel for agent-to-agent communication.
    pub struct AcpChannel {
        base_url: String,
        agent_id: String,
        api_key: Option<String>,
        allowed_agents: Vec<String>,
        client: reqwest::Client,
    }

    impl AcpChannel {
        pub fn new(
            base_url: String,
            agent_id: String,
            api_key: Option<String>,
            allowed_agents: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                agent_id,
                api_key,
                allowed_agents,
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
            if let Some(key) = &self.api_key {
                req.bearer_auth(key)
            } else {
                req
            }
        }
    }

    #[async_trait]
    impl Channel for AcpChannel {
        fn name(&self) -> &str {
            "acp"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "from": self.agent_id,
                    "to": message.recipient,
                    "content": chunk,
                    "timestamp": helpers::now_epoch_secs(),
                });
                let req = self.client.post(self.api_url("/messages")).json(&body);
                let resp = self.add_auth(req).send().await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("acp send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            loop {
                let url = self.api_url(&format!(
                    "/messages/receive?agent_id={}&timeout={}",
                    self.agent_id, POLL_INTERVAL_SECS
                ));
                let req = self.client.get(&url);
                let resp = match self.add_auth(req).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "acp poll failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "acp parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                if let Some(messages) = json["messages"].as_array() {
                    for msg in messages {
                        let sender = msg["from"].as_str().unwrap_or("");
                        if sender.is_empty() {
                            continue;
                        }
                        if !helpers::is_user_allowed(sender, &self.allowed_agents) {
                            continue;
                        }
                        let content = msg["content"].as_str().unwrap_or("");
                        if content.is_empty() {
                            continue;
                        }
                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: sender.to_string(),
                            reply_target: sender.to_string(),
                            content: content.to_string(),
                            channel: "acp".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: None,
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
            let req = self.client.get(self.api_url("/health"));
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
        fn acp_channel_name() {
            let ch = AcpChannel::new(
                "http://localhost:8080".into(),
                "agent-1".into(),
                None,
                vec![],
            );
            assert_eq!(ch.name(), "acp");
        }

        #[test]
        fn acp_api_url_format() {
            let ch = AcpChannel::new(
                "http://localhost:8080/".into(),
                "a".into(),
                None,
                vec![],
            );
            assert_eq!(ch.api_url("/messages"), "http://localhost:8080/messages");
        }
    }
}

#[cfg(feature = "channel-acp")]
pub use impl_::*;

#[cfg(not(feature = "channel-acp"))]
super::channel_stub!(AcpChannel, ACP_DESCRIPTOR, "acp", "ACP (Agent Client Protocol)");

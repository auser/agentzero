#[cfg(feature = "channel-signal")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(SIGNAL_DESCRIPTOR, "signal", "Signal");

    const MAX_MESSAGE_LENGTH: usize = 6000;
    const POLL_INTERVAL_SECS: u64 = 2;

    /// Signal channel via signal-cli REST API.
    pub struct SignalChannel {
        base_url: String,
        phone_number: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl SignalChannel {
        pub fn new(
            base_url: String,
            phone_number: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                phone_number,
                allowed_users,
                client,
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}{}", self.base_url, path)
        }
    }

    #[async_trait]
    impl Channel for SignalChannel {
        fn name(&self) -> &str {
            "signal"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "message": chunk,
                    "number": self.phone_number,
                    "recipients": [message.recipient],
                });
                let resp = self
                    .client
                    .post(self.api_url("/v2/send"))
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("signal send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            loop {
                let url =
                    self.api_url(&format!("/v1/receive/{}", self.phone_number));
                let resp = match self.client.get(&url).send().await {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::error!(error = %e, "signal receive failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                let json: serde_json::Value = match resp.json().await {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "signal parse failed");
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                };
                if let Some(messages) = json.as_array() {
                    for msg in messages {
                        let envelope = &msg["envelope"];
                        let sender =
                            envelope["sourceNumber"].as_str().unwrap_or("");
                        if sender.is_empty() {
                            continue;
                        }
                        if !helpers::is_user_allowed(sender, &self.allowed_users) {
                            continue;
                        }
                        let text = envelope["dataMessage"]["message"]
                            .as_str()
                            .unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let channel_msg = ChannelMessage {
                            id: helpers::new_message_id(),
                            sender: sender.to_string(),
                            reply_target: sender.to_string(),
                            content: text.to_string(),
                            channel: "signal".to_string(),
                            timestamp: helpers::now_epoch_secs(),
                            thread_ts: None,
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
                .get(self.api_url("/v1/about"))
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
        fn signal_channel_name() {
            let ch = SignalChannel::new(
                "http://localhost:8080".into(),
                "+1234567890".into(),
                vec![],
            );
            assert_eq!(ch.name(), "signal");
        }

        #[test]
        fn signal_api_url_format() {
            let ch = SignalChannel::new(
                "http://localhost:8080/".into(),
                "+1".into(),
                vec![],
            );
            assert_eq!(ch.api_url("/v2/send"), "http://localhost:8080/v2/send");
        }
    }
}

#[cfg(feature = "channel-signal")]
pub use impl_::*;

#[cfg(not(feature = "channel-signal"))]
super::channel_stub!(SignalChannel, SIGNAL_DESCRIPTOR, "signal", "Signal");

#[cfg(feature = "channel-wati")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(WATI_DESCRIPTOR, "wati", "WATI");

    const MAX_MESSAGE_LENGTH: usize = 4096;
    const POLL_INTERVAL_SECS: u64 = 5;

    /// WATI (WhatsApp Team Inbox) channel.
    pub struct WatiChannel {
        base_url: String,
        api_token: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl WatiChannel {
        pub fn new(
            base_url: String,
            api_token: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                base_url: base_url.trim_end_matches('/').to_string(),
                api_token,
                allowed_users,
                client,
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}/api/v1{}", self.base_url, path)
        }
    }

    #[async_trait]
    impl Channel for WatiChannel {
        fn name(&self) -> &str {
            "wati"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let url = self.api_url(&format!(
                    "/sendSessionMessage/{}",
                    message.recipient
                ));
                let body = serde_json::json!({"messageText": chunk});
                let resp = self
                    .client
                    .post(&url)
                    .bearer_auth(&self.api_token)
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("wati send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!("wati: listening requires webhook registration. Configure a webhook endpoint that forwards to this channel.");
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(self.api_url("/getContacts"))
                .bearer_auth(&self.api_token)
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
        fn wati_channel_name() {
            let ch = WatiChannel::new(
                "https://live.wati.io".into(),
                "tok".into(),
                vec![],
            );
            assert_eq!(ch.name(), "wati");
        }

        #[test]
        fn wati_api_url_format() {
            let ch = WatiChannel::new(
                "https://live.wati.io/".into(),
                "t".into(),
                vec![],
            );
            assert_eq!(
                ch.api_url("/sendSessionMessage/123"),
                "https://live.wati.io/api/v1/sendSessionMessage/123"
            );
        }
    }
}

#[cfg(feature = "channel-wati")]
pub use impl_::*;

#[cfg(not(feature = "channel-wati"))]
super::channel_stub!(WatiChannel, WATI_DESCRIPTOR, "wati", "WATI");

#[cfg(feature = "channel-dingtalk")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(DINGTALK_DESCRIPTOR, "dingtalk", "DingTalk");

    const MAX_MESSAGE_LENGTH: usize = 20000;
    const POLL_INTERVAL_SECS: u64 = 5;

    const DEFAULT_API_BASE: &str = "https://oapi.dingtalk.com";

    /// DingTalk Robot channel via outgoing webhook.
    pub struct DingtalkChannel {
        access_token: String,
        secret: Option<String>,
        allowed_users: Vec<String>,
        client: reqwest::Client,
        api_base: String,
    }

    impl DingtalkChannel {
        pub fn new(
            access_token: String,
            secret: Option<String>,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                access_token,
                secret,
                allowed_users,
                client,
                api_base: DEFAULT_API_BASE.to_string(),
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        /// Override the API base URL (for testing with mock servers).
        pub fn with_base_url(mut self, base_url: String) -> Self {
            self.api_base = base_url;
            self
        }

        fn webhook_url(&self) -> String {
            format!(
                "{}/robot/send?access_token={}",
                self.api_base, self.access_token
            )
        }
    }

    #[async_trait]
    impl Channel for DingtalkChannel {
        fn name(&self) -> &str {
            "dingtalk"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "msgtype": "text",
                    "text": {"content": chunk},
                });
                let resp = self
                    .client
                    .post(self.webhook_url())
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("dingtalk send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!("dingtalk: listening requires webhook registration. Configure a webhook endpoint that forwards to this channel.");
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .post(self.webhook_url())
                .json(&serde_json::json!({"msgtype": "text", "text": {"content": ""}}))
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
        fn dingtalk_channel_name() {
            let ch = DingtalkChannel::new("tok".into(), None, vec![]);
            assert_eq!(ch.name(), "dingtalk");
        }

        #[test]
        fn dingtalk_webhook_url_format() {
            let ch = DingtalkChannel::new("abc123".into(), None, vec![]);
            assert_eq!(
                ch.webhook_url(),
                "https://oapi.dingtalk.com/robot/send?access_token=abc123"
            );

            let ch_custom = DingtalkChannel::new("abc123".into(), None, vec![])
                .with_base_url("http://localhost:9999".into());
            assert_eq!(
                ch_custom.webhook_url(),
                "http://localhost:9999/robot/send?access_token=abc123"
            );
        }
    }
}

#[cfg(feature = "channel-dingtalk")]
pub use impl_::*;

#[cfg(not(feature = "channel-dingtalk"))]
super::channel_stub!(DingtalkChannel, DINGTALK_DESCRIPTOR, "dingtalk", "DingTalk");

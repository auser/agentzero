#[cfg(feature = "channel-lark")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(LARK_DESCRIPTOR, "lark", "Lark");

    const MAX_MESSAGE_LENGTH: usize = 30000;
    const POLL_INTERVAL_SECS: u64 = 3;

    const DEFAULT_API_BASE: &str = "https://open.larksuite.com";

    /// Lark (Larksuite) Open Platform channel.
    pub struct LarkChannel {
        app_id: String,
        app_secret: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
        api_base: String,
    }

    impl LarkChannel {
        pub fn new(
            app_id: String,
            app_secret: String,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                app_id,
                app_secret,
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

        async fn get_tenant_token(&self) -> anyhow::Result<String> {
            let body = serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            });
            let resp: serde_json::Value = self
                .client
                .post(format!("{}/open-apis/auth/v3/tenant_access_token/internal", self.api_base))
                .json(&body)
                .send()
                .await?
                .json()
                .await?;
            resp["tenant_access_token"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("lark: failed to obtain tenant token"))
        }
    }

    #[async_trait]
    impl Channel for LarkChannel {
        fn name(&self) -> &str {
            "lark"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let token = self.get_tenant_token().await?;
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let body = serde_json::json!({
                    "receive_id": message.recipient,
                    "msg_type": "text",
                    "content": serde_json::json!({"text": chunk}).to_string(),
                });
                let resp = self
                    .client
                    .post(format!("{}/open-apis/im/v1/messages?receive_id_type=chat_id", self.api_base))
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("lark send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!("lark: listening requires event subscription. Configure a webhook endpoint that forwards to this channel.");
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.get_tenant_token().await.is_ok()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lark_channel_name() {
            let ch = LarkChannel::new("id".into(), "secret".into(), vec![]);
            assert_eq!(ch.name(), "lark");
        }
    }
}

#[cfg(feature = "channel-lark")]
pub use impl_::*;

#[cfg(not(feature = "channel-lark"))]
super::channel_stub!(LarkChannel, LARK_DESCRIPTOR, "lark", "Lark");

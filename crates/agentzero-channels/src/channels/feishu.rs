#[cfg(feature = "channel-feishu")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(FEISHU_DESCRIPTOR, "feishu", "Feishu");

    const MAX_MESSAGE_LENGTH: usize = 30000;
    const POLL_INTERVAL_SECS: u64 = 3;

    /// Feishu (Chinese Lark) Open Platform channel.
    pub struct FeishuChannel {
        app_id: String,
        app_secret: String,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl FeishuChannel {
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
            }
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        async fn get_tenant_token(&self) -> anyhow::Result<String> {
            let body = serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            });
            let resp: serde_json::Value = self
                .client
                .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
                .json(&body)
                .send()
                .await?
                .json()
                .await?;
            resp["tenant_access_token"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("feishu: failed to obtain tenant token"))
        }
    }

    #[async_trait]
    impl Channel for FeishuChannel {
        fn name(&self) -> &str {
            "feishu"
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
                    .post("https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type=chat_id")
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("feishu send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!("feishu: listening requires event subscription. Configure a webhook endpoint that forwards to this channel.");
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
        fn feishu_channel_name() {
            let ch = FeishuChannel::new("id".into(), "secret".into(), vec![]);
            assert_eq!(ch.name(), "feishu");
        }
    }
}

#[cfg(feature = "channel-feishu")]
pub use impl_::*;

#[cfg(not(feature = "channel-feishu"))]
super::channel_stub!(FeishuChannel, FEISHU_DESCRIPTOR, "feishu", "Feishu");

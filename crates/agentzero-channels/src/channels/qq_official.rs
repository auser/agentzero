#[cfg(feature = "channel-qq-official")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(QQ_OFFICIAL_DESCRIPTOR, "qq-official", "QQ Official");

    const MAX_MESSAGE_LENGTH: usize = 4500;
    const POLL_INTERVAL_SECS: u64 = 3;

    /// QQ Official Bot channel via QQ Bot Open Platform.
    pub struct QqOfficialChannel {
        app_id: String,
        bot_token: String,
        sandbox: bool,
        allowed_users: Vec<String>,
        client: reqwest::Client,
    }

    impl QqOfficialChannel {
        pub fn new(
            app_id: String,
            bot_token: String,
            sandbox: bool,
            allowed_users: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                app_id,
                bot_token,
                sandbox,
                allowed_users,
                client,
            }
        }

        fn api_base(&self) -> &str {
            if self.sandbox {
                "https://sandbox.api.sgroup.qq.com"
            } else {
                "https://api.sgroup.qq.com"
            }
        }

        fn api_url(&self, path: &str) -> String {
            format!("{}{}", self.api_base(), path)
        }

        fn auth_header(&self) -> String {
            format!("Bot {}.{}", self.app_id, self.bot_token)
        }
    }

    #[async_trait]
    impl Channel for QqOfficialChannel {
        fn name(&self) -> &str {
            "qq-official"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let url = self.api_url(&format!("/channels/{}/messages", message.recipient));
                let body = serde_json::json!({"content": chunk});
                let resp = self
                    .client
                    .post(&url)
                    .header("Authorization", self.auth_header())
                    .json(&body)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("qq-official send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!("qq-official: listening requires WebSocket gateway connection. Configure event handling to forward messages to this channel.");
            loop {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(self.api_url("/gateway"))
                .header("Authorization", self.auth_header())
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
        fn qq_official_channel_name() {
            let ch = QqOfficialChannel::new("app".into(), "tok".into(), false, vec![]);
            assert_eq!(ch.name(), "qq-official");
        }

        #[test]
        fn qq_official_api_url_sandbox() {
            let ch = QqOfficialChannel::new("app".into(), "tok".into(), true, vec![]);
            assert_eq!(
                ch.api_url("/gateway"),
                "https://sandbox.api.sgroup.qq.com/gateway"
            );
        }

        #[test]
        fn qq_official_api_url_production() {
            let ch = QqOfficialChannel::new("app".into(), "tok".into(), false, vec![]);
            assert_eq!(
                ch.api_url("/gateway"),
                "https://api.sgroup.qq.com/gateway"
            );
        }

        #[test]
        fn qq_official_auth_header_format() {
            let ch = QqOfficialChannel::new("123".into(), "abc".into(), false, vec![]);
            assert_eq!(ch.auth_header(), "Bot 123.abc");
        }
    }
}

#[cfg(feature = "channel-qq-official")]
pub use impl_::*;

#[cfg(not(feature = "channel-qq-official"))]
super::channel_stub!(QqOfficialChannel, QQ_OFFICIAL_DESCRIPTOR, "qq-official", "QQ Official");

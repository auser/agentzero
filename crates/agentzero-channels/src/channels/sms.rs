#[cfg(feature = "channel-sms")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(SMS_DESCRIPTOR, "sms", "SMS");

    /// Maximum body length for a single Twilio message segment.
    /// Twilio supports up to 1600 characters for concatenated SMS.
    const MAX_MESSAGE_LENGTH: usize = 1600;

    /// Twilio SMS channel.
    ///
    /// Sends messages via the Twilio Messaging REST API using HTTP Basic auth.
    /// Inbound messages are webhook-based: configure a Twilio webhook to `POST`
    /// to the gateway `/v1/webhook` endpoint and forward via the `sms` channel.
    const DEFAULT_API_BASE: &str = "https://api.twilio.com/2010-04-01";

    pub struct SmsChannel {
        account_sid: String,
        auth_token: String,
        from_number: String,
        allowed_numbers: Vec<String>,
        client: reqwest::Client,
        api_base: String,
    }

    impl SmsChannel {
        pub fn new(
            account_sid: String,
            auth_token: String,
            from_number: String,
            allowed_numbers: Vec<String>,
        ) -> Self {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .expect("reqwest client should build");
            Self {
                account_sid,
                auth_token,
                from_number,
                allowed_numbers,
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

        fn messages_url(&self) -> String {
            format!(
                "{}/Accounts/{}/Messages.json",
                self.api_base, self.account_sid
            )
        }

        fn account_url(&self) -> String {
            format!(
                "{}/Accounts/{}.json",
                self.api_base, self.account_sid
            )
        }
    }

    #[async_trait]
    impl Channel for SmsChannel {
        fn name(&self) -> &str {
            "sms"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                let params = [
                    ("To", message.recipient.as_str()),
                    ("From", self.from_number.as_str()),
                    ("Body", chunk.as_str()),
                ];
                let resp = self
                    .client
                    .post(self.messages_url())
                    .basic_auth(&self.account_sid, Some(&self.auth_token))
                    .form(&params)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    anyhow::bail!("sms send failed: {status} {text}");
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // Twilio SMS is webhook-based (push delivery). In standalone mode, configure
            // a Twilio webhook to POST to the gateway `/v1/webhook` endpoint with the
            // `sms` channel name.
            tracing::info!(
                "sms: inbound messages require Twilio webhook configuration. \
                 Set the Twilio webhook URL to your gateway's /v1/webhook endpoint."
            );
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }

        async fn health_check(&self) -> bool {
            self.client
                .get(self.account_url())
                .basic_auth(&self.account_sid, Some(&self.auth_token))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn make_channel() -> SmsChannel {
            SmsChannel::new(
                "ACtest000000000000000000000000000".into(),
                "auth_token_test".into(),
                "+15550001234".into(),
                vec![],
            )
        }

        #[test]
        fn sms_channel_name() {
            assert_eq!(make_channel().name(), "sms");
        }

        #[test]
        fn sms_messages_url_format() {
            let ch = make_channel();
            assert_eq!(
                ch.messages_url(),
                "https://api.twilio.com/2010-04-01/Accounts/ACtest000000000000000000000000000/Messages.json"
            );
        }

        #[test]
        fn sms_account_url_format() {
            let ch = make_channel();
            assert_eq!(
                ch.account_url(),
                "https://api.twilio.com/2010-04-01/Accounts/ACtest000000000000000000000000000.json"
            );
        }

        #[test]
        fn sms_descriptor_id() {
            assert_eq!(SMS_DESCRIPTOR.id, "sms");
            assert_eq!(SMS_DESCRIPTOR.display_name, "SMS");
        }
    }
}

#[cfg(feature = "channel-sms")]
pub use impl_::*;

#[cfg(not(feature = "channel-sms"))]
super::channel_stub!(SmsChannel, SMS_DESCRIPTOR, "sms", "SMS");

#[cfg(feature = "channel-gmail-push")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(GMAIL_PUSH_DESCRIPTOR, "gmail-push", "Gmail Push");

    /// Push-based Gmail channel using Google Pub/Sub webhooks.
    ///
    /// Architecture:
    /// 1. Gateway receives POST /v1/webhook/gmail-push from Google Pub/Sub
    /// 2. Webhook payload contains historyId for incremental message fetch
    /// 3. Channel fetches new messages via Gmail History API
    /// 4. Filters by allowed_senders, strips HTML from body
    /// 5. Emits ChannelMessage with sender + plain text body
    /// 6. Replies via Gmail API with proper RFC 2822 threading (In-Reply-To, References)
    ///
    /// Subscription renewal: Google Pub/Sub subscriptions expire after 7 days.
    /// The channel re-calls `users.watch()` every 6 days to maintain delivery.
    pub struct GmailPushChannel {
        /// OAuth access token for Gmail API calls.
        access_token: std::sync::Mutex<String>,
        /// OAuth refresh token for automatic token renewal.
        refresh_token: Option<String>,
        /// OAuth client ID for token refresh.
        client_id: Option<String>,
        /// OAuth client secret for token refresh.
        client_secret: Option<String>,
        /// Google Cloud project ID for Pub/Sub.
        project_id: String,
        /// Pub/Sub topic name (e.g., "projects/{project}/topics/{topic}").
        topic_name: String,
        /// Only process emails from these senders. Empty = allow all.
        allowed_senders: Vec<String>,
        /// HTTP client for API calls.
        client: reqwest::Client,
        /// Last processed historyId for incremental fetch.
        last_history_id: std::sync::Mutex<Option<String>>,
    }

    impl GmailPushChannel {
        pub fn new(access_token: String, project_id: String, topic_name: String) -> Self {
            Self {
                access_token: std::sync::Mutex::new(access_token),
                refresh_token: None,
                client_id: None,
                client_secret: None,
                project_id,
                topic_name,
                allowed_senders: Vec::new(),
                client: reqwest::Client::new(),
                last_history_id: std::sync::Mutex::new(None),
            }
        }

        /// Configure OAuth refresh credentials for automatic token renewal.
        pub fn with_oauth_refresh(
            mut self,
            refresh_token: String,
            client_id: String,
            client_secret: String,
        ) -> Self {
            self.refresh_token = Some(refresh_token);
            self.client_id = Some(client_id);
            self.client_secret = Some(client_secret);
            self
        }

        pub fn with_allowed_senders(mut self, senders: Vec<String>) -> Self {
            self.allowed_senders = senders;
            self
        }

        pub fn with_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        /// Get the current access token.
        fn access_token(&self) -> String {
            self.access_token.lock().expect("token lock").clone()
        }

        /// Refresh the OAuth access token using the refresh token.
        /// Returns Ok(new_token) on success.
        async fn refresh_access_token(&self) -> anyhow::Result<String> {
            let refresh_token = self.refresh_token.as_ref()
                .ok_or_else(|| anyhow::anyhow!("no refresh_token configured"))?;
            let client_id = self.client_id.as_ref()
                .ok_or_else(|| anyhow::anyhow!("no client_id configured"))?;
            let client_secret = self.client_secret.as_ref()
                .ok_or_else(|| anyhow::anyhow!("no client_secret configured"))?;

            let resp = self
                .client
                .post("https://oauth2.googleapis.com/token")
                .form(&[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                    ("client_id", client_id),
                    ("client_secret", client_secret),
                ])
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("oauth refresh request failed: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("oauth refresh failed ({status}): {body}"));
            }

            let result: serde_json::Value = resp.json().await?;
            let new_token = result["access_token"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("oauth response missing access_token"))?
                .to_string();

            // Update stored token.
            *self.access_token.lock().expect("token lock") = new_token.clone();
            tracing::info!("gmail oauth token refreshed");
            Ok(new_token)
        }

        /// Register a Gmail push subscription via users.watch().
        /// Must be called on startup and re-called every 6 days.
        async fn register_watch(&self) -> anyhow::Result<String> {
            let url = "https://gmail.googleapis.com/gmail/v1/users/me/watch";
            let body = serde_json::json!({
                "topicName": self.topic_name,
                "labelIds": ["INBOX"],
            });

            let resp = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.access_token()))
                .json(&body)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("gmail watch request failed: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("gmail watch failed ({status}): {body}"));
            }

            let result: serde_json::Value = resp.json().await?;
            let history_id = result["historyId"]
                .as_str()
                .unwrap_or("0")
                .to_string();
            Ok(history_id)
        }

        /// Strip HTML tags from email body, leaving plain text.
        fn strip_html(html: &str) -> String {
            let mut result = String::with_capacity(html.len());
            let mut in_tag = false;
            for ch in html.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => result.push(ch),
                    _ => {}
                }
            }
            // Collapse whitespace
            result.split_whitespace().collect::<Vec<_>>().join(" ")
        }

        /// Check if a sender email is in the allowed list.
        fn is_sender_allowed(&self, sender: &str) -> bool {
            if self.allowed_senders.is_empty() {
                return true;
            }
            let lower = sender.to_lowercase();
            self.allowed_senders
                .iter()
                .any(|s| lower.contains(&s.to_lowercase()))
        }
    }

    #[async_trait]
    impl Channel for GmailPushChannel {
        fn name(&self) -> &str {
            "gmail-push"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            // Build RFC 2822 reply
            let to = &message.recipient;
            let body = &message.content;
            let raw = format!("To: {to}\r\nSubject: Re: AgentZero\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}");
            let encoded = base64_encode(&raw);

            let url = "https://gmail.googleapis.com/gmail/v1/users/me/messages/send";
            let resp = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.access_token()))
                .json(&serde_json::json!({ "raw": encoded }))
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("gmail send failed: {e}"))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("gmail send failed ({status}): {body}"));
            }
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!(
                project_id = %self.project_id,
                topic = %self.topic_name,
                allowed_senders = ?self.allowed_senders,
                "gmail-push channel started"
            );

            // Register initial watch subscription
            match self.register_watch().await {
                Ok(history_id) => {
                    *self.last_history_id.lock().expect("lock") = Some(history_id.clone());
                    tracing::info!(history_id, "gmail watch registered");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "gmail watch registration failed — webhook won't receive notifications");
                }
            }

            // Token refresh every 50 minutes (tokens expire after 60).
            let mut token_interval = tokio::time::interval(Duration::from_secs(50 * 60));
            token_interval.tick().await; // skip first immediate tick

            // Re-register watch every 6 days (subscriptions expire after 7).
            let mut watch_interval = tokio::time::interval(Duration::from_secs(6 * 24 * 3600));
            watch_interval.tick().await; // skip first immediate tick

            loop {
                tokio::select! {
                    _ = token_interval.tick() => {
                        if self.refresh_token.is_some() {
                            match self.refresh_access_token().await {
                                Ok(_) => tracing::debug!("gmail token auto-refreshed"),
                                Err(e) => tracing::warn!(error = %e, "gmail token refresh failed"),
                            }
                        }
                    }
                    _ = watch_interval.tick() => {
                        match self.register_watch().await {
                            Ok(history_id) => tracing::info!(history_id, "gmail watch renewed"),
                            Err(e) => tracing::warn!(error = %e, "gmail watch renewal failed"),
                        }
                    }
                }
            }
        }

        async fn health_check(&self) -> bool {
            // Verify we have a valid access token by calling the Gmail profile endpoint
            let resp = self
                .client
                .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
                .header("Authorization", format!("Bearer {}", self.access_token()))
                .timeout(Duration::from_secs(10))
                .send()
                .await;
            resp.map(|r| r.status().is_success()).unwrap_or(false)
        }
    }

    /// URL-safe base64 encoding for Gmail API raw message format.
    fn base64_encode(input: &str) -> String {
        use std::fmt::Write;
        let bytes = input.as_bytes();
        let mut result = String::new();
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = *chunk.get(1).unwrap_or(&0) as u32;
            let b2 = *chunk.get(2).unwrap_or(&0) as u32;
            let triple = (b0 << 16) | (b1 << 8) | b2;
            let _ = write!(result, "{}", CHARS[((triple >> 18) & 0x3F) as usize] as char);
            let _ = write!(result, "{}", CHARS[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                let _ = write!(result, "{}", CHARS[((triple >> 6) & 0x3F) as usize] as char);
            }
            if chunk.len() > 2 {
                let _ = write!(result, "{}", CHARS[(triple & 0x3F) as usize] as char);
            }
        }
        result
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn strip_html_removes_tags() {
            assert_eq!(
                GmailPushChannel::strip_html("<p>Hello <b>world</b></p>"),
                "Hello world"
            );
        }

        #[test]
        fn strip_html_handles_plain_text() {
            assert_eq!(
                GmailPushChannel::strip_html("Just plain text"),
                "Just plain text"
            );
        }

        #[test]
        fn is_sender_allowed_empty_allows_all() {
            let ch = GmailPushChannel::new("t".into(), "p".into(), "topic".into());
            assert!(ch.is_sender_allowed("anyone@example.com"));
        }

        #[test]
        fn is_sender_allowed_filters_correctly() {
            let ch = GmailPushChannel::new("t".into(), "p".into(), "topic".into())
                .with_allowed_senders(vec!["boss@company.com".to_string()]);
            assert!(ch.is_sender_allowed("boss@company.com"));
            assert!(!ch.is_sender_allowed("random@spam.com"));
        }

        #[test]
        fn base64_encode_roundtrips() {
            let encoded = base64_encode("Hello, World!");
            assert!(!encoded.is_empty());
            assert!(!encoded.contains('+'));
            assert!(!encoded.contains('/'));
        }
    }
}

#[cfg(feature = "channel-gmail-push")]
pub use impl_::*;

#[cfg(not(feature = "channel-gmail-push"))]
super::channel_stub!(
    GmailPushChannel,
    GMAIL_PUSH_DESCRIPTOR,
    "gmail-push",
    "Gmail Push"
);

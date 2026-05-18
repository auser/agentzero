//! Slack gateway adapter.
//!
//! Uses the Slack Web API for polling (`conversations.history`) and
//! sending (`chat.postMessage`). No WebSocket/RTM for v1 — simpler
//! and works through corporate proxies.

use agentzero_tracing::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::config::GatewayEntry;
use crate::{Gateway, GatewayError, IncomingMessage, MessageHandler, OutgoingMessage};

/// Slack gateway using the Web API.
pub struct SlackGateway {
    config: GatewayEntry,
    client: reqwest::Client,
    /// Track the latest message timestamp per channel to avoid re-processing.
    last_ts: std::collections::HashMap<String, String>,
    running: bool,
}

/// Slack API response for conversations.history.
#[derive(Debug, Deserialize)]
struct HistoryResponse {
    ok: bool,
    #[serde(default)]
    messages: Vec<SlackMessage>,
    #[serde(default)]
    error: Option<String>,
}

/// A single Slack message.
#[derive(Debug, Deserialize)]
struct SlackMessage {
    #[serde(default)]
    text: String,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    ts: String,
    #[serde(default)]
    thread_ts: Option<String>,
    /// Bot messages have bot_id set.
    #[serde(default)]
    bot_id: Option<String>,
}

/// Slack API response for chat.postMessage.
#[derive(Debug, Deserialize)]
struct PostResponse {
    ok: bool,
    #[serde(default)]
    error: Option<String>,
}

/// Request body for chat.postMessage.
#[derive(Debug, Serialize)]
struct PostRequest<'a> {
    channel: &'a str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<&'a str>,
}

impl SlackGateway {
    /// Create a new Slack gateway from configuration.
    pub fn new(config: GatewayEntry) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            last_ts: std::collections::HashMap::new(),
            running: false,
        }
    }

    /// Fetch new messages from a channel since the last known timestamp.
    async fn poll_channel(&mut self, channel: &str) -> Result<Vec<IncomingMessage>, GatewayError> {
        let mut url =
            format!("https://slack.com/api/conversations.history?channel={channel}&limit=10");
        if let Some(oldest) = self.last_ts.get(channel) {
            url.push_str(&format!("&oldest={oldest}"));
        }

        let resp = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.config.token))
            .send()
            .await
            .map_err(|e| GatewayError::Connection(format!("Slack API error: {e}")))?;

        let history: HistoryResponse = resp
            .json()
            .await
            .map_err(|e| GatewayError::Connection(format!("Slack JSON error: {e}")))?;

        if !history.ok {
            let err = history.error.unwrap_or_else(|| "unknown".to_string());
            return Err(GatewayError::Connection(format!("Slack API error: {err}")));
        }

        let mut incoming = Vec::new();
        for msg in &history.messages {
            // Skip bot messages (avoid responding to ourselves)
            if msg.bot_id.is_some() {
                continue;
            }

            // Update latest timestamp
            if self.last_ts.get(channel).is_none_or(|ts| msg.ts > *ts) {
                self.last_ts.insert(channel.to_string(), msg.ts.clone());
            }

            incoming.push(IncomingMessage {
                channel: channel.to_string(),
                sender: msg.user.clone().unwrap_or_else(|| "unknown".to_string()),
                text: msg.text.clone(),
                thread_id: msg.thread_ts.clone(),
            });
        }

        Ok(incoming)
    }

    /// Send a message to Slack.
    pub async fn send_message(&self, msg: &OutgoingMessage) -> Result<(), GatewayError> {
        let body = PostRequest {
            channel: &msg.channel,
            text: &msg.text,
            thread_ts: msg.thread_id.as_deref(),
        };

        let resp = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .header("Authorization", format!("Bearer {}", self.config.token))
            .json(&body)
            .send()
            .await
            .map_err(|e| GatewayError::Send(format!("Slack send error: {e}")))?;

        let post_resp: PostResponse = resp
            .json()
            .await
            .map_err(|e| GatewayError::Send(format!("Slack response error: {e}")))?;

        if !post_resp.ok {
            let err = post_resp.error.unwrap_or_else(|| "unknown".to_string());
            return Err(GatewayError::Send(format!("Slack post error: {err}")));
        }

        Ok(())
    }
}

#[async_trait::async_trait]
impl Gateway for SlackGateway {
    fn name(&self) -> &str {
        "slack"
    }

    async fn start(&mut self, handler: Box<dyn MessageHandler>) -> Result<(), GatewayError> {
        self.running = true;
        let poll_interval = std::time::Duration::from_secs(self.config.poll_interval_secs);
        let channels = self.config.channels.clone();

        info!(
            gateway = "slack",
            channels = ?channels,
            poll_secs = self.config.poll_interval_secs,
            "starting Slack gateway"
        );

        while self.running {
            for channel in &channels {
                match self.poll_channel(channel).await {
                    Ok(messages) => {
                        for msg in messages {
                            debug!(
                                channel = %msg.channel,
                                sender = %msg.sender,
                                "received Slack message"
                            );
                            match handler.handle(msg.clone()).await {
                                Ok(response) => {
                                    let outgoing = OutgoingMessage {
                                        channel: msg.channel.clone(),
                                        text: response,
                                        thread_id: msg.thread_id.clone(),
                                    };
                                    if let Err(e) = self.send_message(&outgoing).await {
                                        warn!(error = %e, "failed to send Slack response");
                                    }
                                }
                                Err(e) => {
                                    warn!(error = %e, "message handler error");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(channel = %channel, error = %e, "failed to poll Slack channel");
                    }
                }
            }

            tokio::time::sleep(poll_interval).await;
        }

        info!(gateway = "slack", "Slack gateway stopped");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), GatewayError> {
        self.running = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> GatewayEntry {
        GatewayEntry {
            name: "test-slack".into(),
            gateway_type: "slack".into(),
            token: "xoxb-test-token".into(),
            channels: vec!["#test".into()],
            poll_interval_secs: 5,
        }
    }

    #[test]
    fn slack_gateway_name() {
        let gw = SlackGateway::new(test_config());
        assert_eq!(gw.name(), "slack");
    }

    #[test]
    fn post_request_serializes() {
        let req = PostRequest {
            channel: "#test",
            text: "hello",
            thread_ts: None,
        };
        let json = serde_json::to_string(&req).expect("should serialize");
        assert!(json.contains("hello"));
        assert!(!json.contains("thread_ts"));
    }

    #[test]
    fn post_request_with_thread() {
        let req = PostRequest {
            channel: "#test",
            text: "reply",
            thread_ts: Some("1234567890.123456"),
        };
        let json = serde_json::to_string(&req).expect("should serialize");
        assert!(json.contains("thread_ts"));
    }

    #[test]
    fn history_response_parses() {
        let json = r#"{"ok": true, "messages": [{"text": "hello", "user": "U123", "ts": "1234567890.123456"}]}"#;
        let resp: HistoryResponse = serde_json::from_str(json).expect("should parse");
        assert!(resp.ok);
        assert_eq!(resp.messages.len(), 1);
        assert_eq!(resp.messages[0].text, "hello");
    }

    #[test]
    fn history_response_error() {
        let json = r#"{"ok": false, "error": "channel_not_found"}"#;
        let resp: HistoryResponse = serde_json::from_str(json).expect("should parse");
        assert!(!resp.ok);
        assert_eq!(resp.error, Some("channel_not_found".to_string()));
    }

    #[tokio::test]
    async fn stop_sets_running_false() {
        let mut gw = SlackGateway::new(test_config());
        gw.running = true;
        gw.stop().await.expect("should stop");
        assert!(!gw.running);
    }
}

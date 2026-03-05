//! Platform integrations for AgentZero.
//!
//! Implements Telegram, Discord, and Slack channel adapters that bridge
//! chat messages to the agent loop. Includes command parsing, reaction
//! acknowledgement, and leak-guard middleware for sensitive data filtering.

pub mod ack_reactions;
mod channels;
pub mod commands;
pub mod drafts;
pub mod group_reply;
pub mod image_markers;
pub mod interruption;
pub mod leak_guard;
pub mod outbound;
pub mod pipeline;

pub use channels::channel_setup::{register_configured_channels, ChannelInstanceConfig};
pub use channels::CHANNEL_CATALOG;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// Re-export channel implementations that need public access
pub use channels::{CliChannel, WebhookChannel};

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

/// A message received from a channel (inbound).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,
    pub reply_target: String,
    pub content: String,
    pub channel: String,
    pub timestamp: u64,
    pub thread_ts: Option<String>,
    /// Privacy boundary inherited from the channel configuration.
    /// Empty string means inherit the global privacy mode.
    #[serde(default)]
    pub privacy_boundary: String,
}

/// A message to send through a channel (outbound).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessage {
    pub content: String,
    pub recipient: String,
    pub subject: Option<String>,
    pub thread_ts: Option<String>,
}

impl SendMessage {
    pub fn new(content: impl Into<String>, recipient: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: None,
            thread_ts: None,
        }
    }

    pub fn with_subject(
        content: impl Into<String>,
        recipient: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: Some(subject.into()),
            thread_ts: None,
        }
    }

    pub fn in_thread(mut self, thread_ts: Option<String>) -> Self {
        self.thread_ts = thread_ts;
        self
    }
}

// ---------------------------------------------------------------------------
// Channel trait
// ---------------------------------------------------------------------------

/// Core channel trait — implement for any messaging platform.
#[async_trait]
pub trait Channel: Send + Sync {
    fn name(&self) -> &str;

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()>;

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;

    async fn health_check(&self) -> bool {
        true
    }

    async fn start_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        false
    }

    async fn send_draft(&self, _message: &SendMessage) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn update_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn finalize_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn add_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Channel descriptor & catalog
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
}

pub fn channel_catalog() -> &'static [ChannelDescriptor] {
    CHANNEL_CATALOG
}

pub fn normalize_channel_id(input: &str) -> Option<&'static str> {
    let needle = input.trim();
    if needle.is_empty() {
        return None;
    }

    for channel in CHANNEL_CATALOG {
        if channel.id.eq_ignore_ascii_case(needle)
            || channel
                .display_name
                .replace(' ', "-")
                .eq_ignore_ascii_case(needle)
            || channel.display_name.eq_ignore_ascii_case(needle)
        {
            return Some(channel.id);
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Delivery types (gateway webhook compatibility)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelDelivery {
    pub accepted: bool,
    pub channel: String,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// Channel locality
// ---------------------------------------------------------------------------

/// Channels that operate entirely locally (no network egress).
const LOCAL_CHANNELS: &[&str] = &["cli", "transcription"];

/// Check if a channel operates locally (no outbound network traffic).
pub fn is_local_channel(name: &str) -> bool {
    LOCAL_CHANNELS.contains(&name)
}

// ---------------------------------------------------------------------------
// Channel registry
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ChannelRegistry {
    channels: HashMap<String, Arc<dyn Channel>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtin_handlers() -> Self {
        let mut registry = Self::new();
        registry.register(Arc::new(CliChannel));
        registry
    }

    pub fn register(&mut self, channel: Arc<dyn Channel>) {
        self.channels.insert(channel.name().to_string(), channel);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Channel>> {
        self.channels.get(name).cloned()
    }

    pub fn has_channel(&self, name: &str) -> bool {
        self.channels.contains_key(name)
    }

    pub fn channel_names(&self) -> Vec<&str> {
        self.channels.keys().map(String::as_str).collect()
    }

    pub fn all_channels(&self) -> Vec<Arc<dyn Channel>> {
        self.channels.values().cloned().collect()
    }

    /// Dispatch a message to a channel. If `boundary` is `"local_only"`, only
    /// local channels (CLI, transcription) are allowed; non-local targets are
    /// rejected with `accepted: false`.
    pub async fn dispatch(
        &self,
        channel: &str,
        payload: serde_json::Value,
    ) -> Option<ChannelDelivery> {
        self.dispatch_with_boundary(channel, payload, "").await
    }

    /// Dispatch with an explicit privacy boundary check.
    pub async fn dispatch_with_boundary(
        &self,
        channel: &str,
        payload: serde_json::Value,
        boundary: &str,
    ) -> Option<ChannelDelivery> {
        let ch = self.channels.get(channel)?;

        // Enforce privacy boundary: local_only blocks non-local channels.
        if boundary == "local_only" && !is_local_channel(channel) {
            return Some(ChannelDelivery {
                accepted: false,
                channel: channel.to_string(),
                detail: "blocked by local_only privacy boundary".to_string(),
            });
        }

        let content = payload
            .get("text")
            .or_else(|| payload.get("content"))
            .or_else(|| payload.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let recipient = payload
            .get("recipient")
            .or_else(|| payload.get("channel_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();

        let msg = SendMessage::new(content, recipient);
        match ch.send(&msg).await {
            Ok(()) => Some(ChannelDelivery {
                accepted: true,
                channel: channel.to_string(),
                detail: "message sent".to_string(),
            }),
            Err(e) => Some(ChannelDelivery {
                accepted: false,
                channel: channel.to_string(),
                detail: format!("send failed: {e}"),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoChannel;

    #[async_trait]
    impl Channel for EchoChannel {
        fn name(&self) -> &str {
            "echo"
        }

        async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tx.send(ChannelMessage {
                id: "1".into(),
                sender: "tester".into(),
                reply_target: "tester".into(),
                content: "hello".into(),
                channel: "echo".into(),
                timestamp: 123,
                thread_ts: None,
                privacy_boundary: String::new(),
            })
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
        }
    }

    #[test]
    fn send_message_builder_success_path() {
        let msg = SendMessage::new("hello", "user-1");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.recipient, "user-1");
        assert!(msg.subject.is_none());
        assert!(msg.thread_ts.is_none());

        let threaded = msg.in_thread(Some("ts-123".into()));
        assert_eq!(threaded.thread_ts.as_deref(), Some("ts-123"));
    }

    #[test]
    fn send_message_with_subject_success_path() {
        let msg = SendMessage::with_subject("body", "user", "subject line");
        assert_eq!(msg.subject.as_deref(), Some("subject line"));
    }

    #[test]
    fn channel_message_serde_round_trip_success_path() {
        let msg = ChannelMessage {
            id: "42".into(),
            sender: "alice".into(),
            reply_target: "alice".into(),
            content: "ping".into(),
            channel: "test".into(),
            timestamp: 999,
            thread_ts: Some("thread-1".into()),
            privacy_boundary: String::new(),
        };

        let json = serde_json::to_string(&msg).expect("serialize should succeed");
        let parsed: ChannelMessage =
            serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(parsed.id, "42");
        assert_eq!(parsed.sender, "alice");
        assert_eq!(parsed.thread_ts.as_deref(), Some("thread-1"));
    }

    #[tokio::test]
    async fn default_trait_methods_return_success() {
        let channel = EchoChannel;
        assert!(channel.health_check().await);
        assert!(channel.start_typing("bob").await.is_ok());
        assert!(channel.stop_typing("bob").await.is_ok());
        assert!(!channel.supports_draft_updates());
        assert!(channel
            .send_draft(&SendMessage::new("draft", "bob"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn listen_sends_message_to_channel() {
        let channel = EchoChannel;
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);

        channel.listen(tx).await.unwrap();

        let received = rx.recv().await.expect("message should be received");
        assert_eq!(received.sender, "tester");
        assert_eq!(received.content, "hello");
    }

    #[test]
    fn registry_register_and_get_success_path() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel));

        assert!(registry.has_channel("echo"));
        assert!(!registry.has_channel("missing"));
        assert!(registry.get("echo").is_some());
    }

    #[tokio::test]
    async fn registry_dispatch_success_path() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel));

        let delivery = registry
            .dispatch("echo", serde_json::json!({"text": "hello"}))
            .await
            .expect("dispatch should find channel");

        assert!(delivery.accepted);
        assert_eq!(delivery.channel, "echo");
    }

    #[tokio::test]
    async fn registry_dispatch_unknown_returns_none() {
        let registry = ChannelRegistry::new();
        let result = registry
            .dispatch("missing", serde_json::json!({"text": "hello"}))
            .await;
        assert!(result.is_none());
    }

    #[test]
    fn normalize_channel_id_success_path() {
        assert_eq!(normalize_channel_id("telegram"), Some("telegram"));
        assert_eq!(normalize_channel_id("Telegram"), Some("telegram"));
        assert_eq!(
            normalize_channel_id("NextCloud Talk"),
            Some("nextcloud-talk")
        );
    }

    #[test]
    fn normalize_channel_id_unknown_returns_none() {
        assert_eq!(normalize_channel_id("missing-channel"), None);
    }

    #[test]
    fn channel_catalog_contains_known_entries() {
        let catalog = channel_catalog();
        assert!(!catalog.is_empty());
        let ids: Vec<&str> = catalog.iter().map(|d| d.id).collect();
        assert!(ids.contains(&"cli"));
        assert!(ids.contains(&"telegram"));
        assert!(ids.contains(&"webhook"));
    }

    #[test]
    fn builtin_registry_has_cli_channel() {
        let registry = ChannelRegistry::with_builtin_handlers();
        assert!(registry.has_channel("cli"));
    }

    // --- Phase 2: Channel privacy boundary tests ---

    #[test]
    fn channel_message_serde_backward_compat_without_privacy_boundary() {
        // Old JSON without privacy_boundary should deserialize with default empty string.
        let json = r#"{"id":"1","sender":"a","reply_target":"a","content":"hi","channel":"cli","timestamp":0}"#;
        let msg: ChannelMessage = serde_json::from_str(json).expect("deserialize old format");
        assert_eq!(msg.privacy_boundary, "");
    }

    #[test]
    fn channel_message_serde_with_privacy_boundary() {
        let msg = ChannelMessage {
            id: "1".into(),
            sender: "a".into(),
            reply_target: "a".into(),
            content: "hi".into(),
            channel: "cli".into(),
            timestamp: 0,
            thread_ts: None,
            privacy_boundary: "local_only".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ChannelMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.privacy_boundary, "local_only");
    }

    #[test]
    fn is_local_channel_cli_and_transcription() {
        assert!(is_local_channel("cli"));
        assert!(is_local_channel("transcription"));
    }

    #[test]
    fn is_local_channel_non_local() {
        assert!(!is_local_channel("telegram"));
        assert!(!is_local_channel("discord"));
        assert!(!is_local_channel("slack"));
        assert!(!is_local_channel("webhook"));
        assert!(!is_local_channel("email"));
    }

    #[tokio::test]
    async fn dispatch_local_only_blocks_non_local_channel() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel)); // "echo" is non-local
        let delivery = registry
            .dispatch_with_boundary("echo", serde_json::json!({"text": "secret"}), "local_only")
            .await
            .expect("should return delivery");
        assert!(!delivery.accepted);
        assert!(delivery.detail.contains("local_only"));
    }

    #[tokio::test]
    async fn dispatch_local_only_allows_local_channel() {
        // CLI is local, so local_only should allow it.
        let registry = ChannelRegistry::with_builtin_handlers();
        let delivery = registry
            .dispatch_with_boundary("cli", serde_json::json!({"text": "hello"}), "local_only")
            .await
            .expect("should return delivery");
        assert!(delivery.accepted);
    }

    #[tokio::test]
    async fn dispatch_any_boundary_allows_all() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel));
        let delivery = registry
            .dispatch_with_boundary("echo", serde_json::json!({"text": "hello"}), "any")
            .await
            .expect("should return delivery");
        assert!(delivery.accepted);
    }

    #[tokio::test]
    async fn dispatch_empty_boundary_allows_all() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel));
        let delivery = registry
            .dispatch_with_boundary("echo", serde_json::json!({"text": "hello"}), "")
            .await
            .expect("should return delivery");
        assert!(delivery.accepted);
    }

    #[tokio::test]
    async fn dispatch_encrypted_only_allows_non_local() {
        let mut registry = ChannelRegistry::new();
        registry.register(Arc::new(EchoChannel));
        let delivery = registry
            .dispatch_with_boundary(
                "echo",
                serde_json::json!({"text": "hello"}),
                "encrypted_only",
            )
            .await
            .expect("should return delivery");
        assert!(delivery.accepted);
    }
}

//! Messaging gateway framework for AgentZero.
//!
//! Gateways bridge external messaging platforms (Slack, Telegram, Discord)
//! to the AgentZero agent loop. Each gateway is a native Rust adapter —
//! not a WASM plugin — because gateways are long-running processes that
//! need persistent connections (WebSocket, long-poll, webhook listeners).
//!
//! The `GatewayAgent` wrapper enforces PII redaction on all outbound
//! messages, since gateways send to remote platforms.

pub mod config;
pub mod slack;

use async_trait::async_trait;
use thiserror::Error;

/// Errors that can occur during gateway operations.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("gateway configuration error: {0}")]
    Config(String),
    #[error("gateway connection error: {0}")]
    Connection(String),
    #[error("gateway send error: {0}")]
    Send(String),
    #[error("gateway stopped")]
    Stopped,
}

/// A messaging gateway that bridges an external platform to AgentZero.
///
/// Gateways are long-running — they poll or listen for messages and
/// dispatch them through the agent loop.
#[async_trait]
pub trait Gateway: Send + Sync {
    /// Human-readable name of this gateway (e.g. "slack", "telegram").
    fn name(&self) -> &str;

    /// Start the gateway, processing incoming messages.
    ///
    /// This method should run until stopped or an error occurs.
    /// Incoming messages are dispatched through the `message_handler`.
    async fn start(&mut self, handler: Box<dyn MessageHandler>) -> Result<(), GatewayError>;

    /// Stop the gateway gracefully.
    async fn stop(&mut self) -> Result<(), GatewayError>;
}

/// Handler for incoming gateway messages.
///
/// Implementations route messages through the agent loop and return
/// the response. PII redaction is applied before sending back.
#[async_trait]
pub trait MessageHandler: Send + Sync {
    /// Process an incoming message and return the response text.
    async fn handle(&self, message: IncomingMessage) -> Result<String, GatewayError>;
}

/// A message received from an external platform.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Platform-specific channel or conversation ID.
    pub channel: String,
    /// Display name or ID of the sender.
    pub sender: String,
    /// The message text.
    pub text: String,
    /// Optional thread/reply ID for threading.
    pub thread_id: Option<String>,
}

/// A message to send back to the platform.
#[derive(Debug, Clone)]
pub struct OutgoingMessage {
    /// Platform-specific channel or conversation ID.
    pub channel: String,
    /// The response text (PII-redacted).
    pub text: String,
    /// Optional thread/reply ID for threading.
    pub thread_id: Option<String>,
}

/// Information about a configured gateway.
#[derive(Debug, Clone)]
pub struct GatewayInfo {
    pub name: String,
    pub gateway_type: String,
    pub status: GatewayStatus,
}

/// Runtime status of a gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayStatus {
    Configured,
    Running,
    Stopped,
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_error_display() {
        let err = GatewayError::Config("missing token".to_string());
        assert_eq!(
            err.to_string(),
            "gateway configuration error: missing token"
        );
    }

    #[test]
    fn incoming_message_clone() {
        let msg = IncomingMessage {
            channel: "#dev".to_string(),
            sender: "user1".to_string(),
            text: "hello".to_string(),
            thread_id: None,
        };
        let cloned = msg.clone();
        assert_eq!(cloned.text, "hello");
    }

    #[test]
    fn gateway_status_equality() {
        assert_eq!(GatewayStatus::Running, GatewayStatus::Running);
        assert_ne!(GatewayStatus::Running, GatewayStatus::Stopped);
    }
}

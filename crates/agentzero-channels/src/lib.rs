use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelMessage {
    pub channel: String,
    pub payload: Value,
    pub received_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelDelivery {
    pub accepted: bool,
    pub channel: String,
    pub detail: String,
}

pub trait ChannelHandler: Send + Sync {
    fn name(&self) -> &'static str;
    fn handle(&self, message: &ChannelMessage) -> ChannelDelivery;
}

#[derive(Default)]
pub struct ChannelRegistry {
    handlers: HashMap<String, Arc<dyn ChannelHandler>>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_builtin_handlers() -> Self {
        let mut registry = Self::new();
        registry.register(EchoChannel);
        registry
    }

    pub fn register<T>(&mut self, handler: T)
    where
        T: ChannelHandler + 'static,
    {
        self.handlers
            .insert(handler.name().to_string(), Arc::new(handler));
    }

    pub fn dispatch(&self, channel: &str, payload: Value) -> Option<ChannelDelivery> {
        let handler = self.handlers.get(channel)?;
        let message = ChannelMessage {
            channel: channel.to_string(),
            payload,
            received_at_epoch_secs: now_epoch_secs(),
        };
        Some(handler.handle(&message))
    }

    pub fn has_channel(&self, channel: &str) -> bool {
        self.handlers.contains_key(channel)
    }
}

pub struct EchoChannel;

impl ChannelHandler for EchoChannel {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn handle(&self, message: &ChannelMessage) -> ChannelDelivery {
        ChannelDelivery {
            accepted: true,
            channel: message.channel.clone(),
            detail: format!(
                "accepted payload bytes={}",
                message.payload.to_string().len()
            ),
        }
    }
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builtin_echo_channel_dispatch_success_path() {
        let registry = ChannelRegistry::with_builtin_handlers();
        let delivery = registry
            .dispatch("echo", json!({"text": "hello"}))
            .expect("echo channel should be registered");

        assert!(delivery.accepted);
        assert_eq!(delivery.channel, "echo");
        assert!(delivery.detail.contains("accepted payload bytes="));
    }

    #[test]
    fn dispatch_unknown_channel_returns_none_negative_path() {
        let registry = ChannelRegistry::with_builtin_handlers();
        let delivery = registry.dispatch("missing", json!({"text": "hello"}));

        assert!(delivery.is_none());
    }
}

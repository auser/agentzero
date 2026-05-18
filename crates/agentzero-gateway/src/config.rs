//! Gateway configuration loaded from `.agentzero/gateways.toml`.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaysConfig {
    /// Configured gateways.
    #[serde(default)]
    pub gateway: Vec<GatewayEntry>,
}

/// A single gateway entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayEntry {
    /// Human-readable name for this gateway instance.
    pub name: String,
    /// Gateway type: "slack", "telegram", "discord".
    #[serde(rename = "type")]
    pub gateway_type: String,
    /// Authentication token. Supports "vault:<path>" references.
    pub token: String,
    /// Channels or conversations to monitor.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Poll interval in seconds (for polling-based gateways).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_poll_interval() -> u64 {
    5
}

impl GatewaysConfig {
    /// Load from a TOML file path.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        toml::from_str(&content).map_err(|e| format!("failed to parse {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gateway_config() {
        let toml_str = r##"
[[gateway]]
name = "slack-dev"
type = "slack"
token = "vault:slack/bot_token"
channels = ["#dev-agent", "#general"]
poll_interval_secs = 10

[[gateway]]
name = "telegram-bot"
type = "telegram"
token = "vault:telegram/bot_token"
channels = []
"##;
        let config: GatewaysConfig = toml::from_str(toml_str).expect("should parse");
        assert_eq!(config.gateway.len(), 2);
        assert_eq!(config.gateway[0].name, "slack-dev");
        assert_eq!(config.gateway[0].gateway_type, "slack");
        assert_eq!(config.gateway[0].token, "vault:slack/bot_token");
        assert_eq!(config.gateway[0].channels, vec!["#dev-agent", "#general"]);
        assert_eq!(config.gateway[0].poll_interval_secs, 10);

        // Second gateway gets default poll interval
        assert_eq!(config.gateway[1].name, "telegram-bot");
        assert_eq!(config.gateway[1].poll_interval_secs, 5);
    }

    #[test]
    fn empty_config() {
        let toml_str = "";
        let config: GatewaysConfig = toml::from_str(toml_str).expect("should parse empty");
        assert!(config.gateway.is_empty());
    }

    #[test]
    fn vault_token_reference() {
        let entry = GatewayEntry {
            name: "test".into(),
            gateway_type: "slack".into(),
            token: "vault:slack/bot_token".into(),
            channels: vec![],
            poll_interval_secs: 5,
        };
        assert!(entry.token.starts_with("vault:"));
    }
}

#[allow(unused_imports)]
use crate::ChannelRegistry;
use serde::Deserialize;
use std::collections::HashMap;
#[allow(unused_imports)]
use std::sync::Arc;
#[allow(unused_imports)]
use std::time::Duration;

/// Per-channel instance config from TOML `[channels.<name>]` sections.
/// Uses a common structure with optional fields; each channel type consumes
/// only the fields it needs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ChannelInstanceConfig {
    pub bot_token: Option<String>,
    pub app_token: Option<String>,
    pub base_url: Option<String>,
    pub token: Option<String>,
    pub channel_id: Option<String>,
    pub room_id: Option<String>,
    pub homeserver: Option<String>,
    pub access_token: Option<String>,
    pub server: Option<String>,
    pub port: Option<u16>,
    pub nick: Option<String>,
    pub channel_name: Option<String>,
    pub password: Option<String>,
    pub relay_url: Option<String>,
    pub private_key_hex: Option<String>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub username: Option<String>,
    pub from_address: Option<String>,
    /// Twilio Account SID for the SMS channel.
    pub account_sid: Option<String>,
    /// Twilio-assigned sending number (E.164) for the SMS channel.
    pub from_number: Option<String>,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub allowed_pubkeys: Vec<String>,
    #[serde(default)]
    pub allowed_senders: Vec<String>,
    /// Per-channel privacy boundary override.
    /// Empty string means inherit from `[channels] default_privacy_boundary`.
    #[serde(default)]
    pub privacy_boundary: String,
    /// Per-channel HTTP proxy override (e.g. "http://proxy:8080").
    /// Falls back to global proxy if not set.
    #[serde(default)]
    pub http_proxy: Option<String>,
    /// Per-channel HTTPS proxy override.
    #[serde(default)]
    pub https_proxy: Option<String>,
    /// Per-channel SOCKS proxy override (e.g. "socks5://127.0.0.1:1080").
    #[serde(default)]
    pub socks_proxy: Option<String>,
    /// Per-channel no-proxy bypass list. If non-empty, overrides the global list.
    #[serde(default)]
    pub no_proxy: Vec<String>,
}

impl ChannelInstanceConfig {
    /// Returns `true` if any per-channel proxy is configured.
    pub fn has_proxy(&self) -> bool {
        self.http_proxy.is_some() || self.https_proxy.is_some() || self.socks_proxy.is_some()
    }
}

/// Build a `reqwest::Client` with proxy settings from a channel config.
///
/// If the config has proxy fields set, they are applied to the client builder.
/// SOCKS proxy is applied as `reqwest::Proxy::all` (requires reqwest `socks` feature
/// to actually connect; without it, the URL is accepted but connections will fail).
#[cfg(any(
    feature = "channel-telegram",
    feature = "channel-discord",
    feature = "channel-slack",
    feature = "channel-mattermost",
    feature = "channel-matrix",
    feature = "channel-whatsapp",
    feature = "channel-signal",
    feature = "channel-lark",
    feature = "channel-feishu",
    feature = "channel-dingtalk",
    feature = "channel-qq-official",
    feature = "channel-nextcloud-talk",
    feature = "channel-sms",
    feature = "channel-clawdtalk",
    feature = "channel-linq",
    feature = "channel-wati",
    feature = "channel-napcat",
    feature = "channel-acp",
))]
pub fn build_channel_client(
    config: &ChannelInstanceConfig,
    timeout_secs: u64,
) -> Result<reqwest::Client, String> {
    // When per-channel proxy is set, disable system proxy and use only explicit ones.
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .no_proxy();

    // Build the no_proxy bypass matcher (applied to each proxy rule).
    let no_proxy_matcher = if config.no_proxy.is_empty() {
        None
    } else {
        reqwest::NoProxy::from_string(&config.no_proxy.join(","))
    };

    // Apply SOCKS proxy (covers all traffic).
    if let Some(ref socks) = config.socks_proxy {
        let mut proxy = reqwest::Proxy::all(socks)
            .map_err(|e| format!("invalid socks_proxy URL '{socks}': {e}"))?;
        proxy = proxy.no_proxy(no_proxy_matcher.clone());
        builder = builder.proxy(proxy);
    }

    // Apply HTTP proxy.
    if let Some(ref http) = config.http_proxy {
        let mut proxy = reqwest::Proxy::http(http)
            .map_err(|e| format!("invalid http_proxy URL '{http}': {e}"))?;
        proxy = proxy.no_proxy(no_proxy_matcher.clone());
        builder = builder.proxy(proxy);
    }

    // Apply HTTPS proxy.
    if let Some(ref https) = config.https_proxy {
        let mut proxy = reqwest::Proxy::https(https)
            .map_err(|e| format!("invalid https_proxy URL '{https}': {e}"))?;
        proxy = proxy.no_proxy(no_proxy_matcher.clone());
        builder = builder.proxy(proxy);
    }

    builder
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))
}

/// Register channels into `registry` based on the provided per-channel configs.
///
/// Each entry in `configs` maps a channel name (e.g. `"telegram"`) to its
/// [`ChannelInstanceConfig`]. Only channels whose feature is compiled in
/// will be registered; others are silently skipped.
///
/// Returns a list of `(channel_name, error)` for channels that failed to construct.
pub fn register_configured_channels(
    registry: &mut ChannelRegistry,
    configs: &HashMap<String, ChannelInstanceConfig>,
) -> Vec<(String, String)> {
    let mut errors = Vec::new();

    for (name, config) in configs {
        match register_one(registry, name, config) {
            Ok(true) => {
                tracing::info!(channel = %name, "registered configured channel");
            }
            Ok(false) => {
                tracing::debug!(channel = %name, "channel not compiled in, skipping");
            }
            Err(e) => {
                tracing::warn!(channel = %name, error = %e, "failed to register channel");
                errors.push((name.clone(), e));
            }
        }
    }

    errors
}

/// Build a single channel instance from a name and config.
///
/// Returns `Ok(Some(channel))` if the channel was built successfully,
/// `Ok(None)` if the channel feature is not compiled in,
/// `Err(msg)` if the config is invalid.
#[allow(unused_variables)]
pub fn build_channel_instance(
    name: &str,
    config: &ChannelInstanceConfig,
) -> Result<Option<Arc<dyn crate::Channel>>, String> {
    match name {
        #[cfg(feature = "channel-telegram")]
        "telegram" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("telegram requires bot_token")?;
            let mut channel =
                super::TelegramChannel::new(bot_token.clone(), config.allowed_users.clone());
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 40)?);
            }
            Ok(Some(Arc::new(channel)))
        }
        #[cfg(feature = "channel-discord")]
        "discord" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("discord requires bot_token")?;
            let mut channel =
                super::DiscordChannel::new(bot_token.clone(), config.allowed_users.clone());
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            Ok(Some(Arc::new(channel)))
        }
        #[cfg(feature = "channel-slack")]
        "slack" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("slack requires bot_token")?;
            let mut channel = super::SlackChannel::new(
                bot_token.clone(),
                config.app_token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            Ok(Some(Arc::new(channel)))
        }
        _ => Ok(None),
    }
}

/// Try to register a single channel.
/// Returns `Ok(true)` if registered, `Ok(false)` if feature not compiled in,
/// `Err(msg)` if config is invalid.
#[allow(unused_variables)]
fn register_one(
    registry: &mut ChannelRegistry,
    name: &str,
    config: &ChannelInstanceConfig,
) -> Result<bool, String> {
    match name {
        #[cfg(feature = "channel-telegram")]
        "telegram" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("telegram requires bot_token")?;
            let mut channel =
                super::TelegramChannel::new(bot_token.clone(), config.allowed_users.clone());
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 40)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-discord")]
        "discord" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("discord requires bot_token")?;
            let mut channel =
                super::DiscordChannel::new(bot_token.clone(), config.allowed_users.clone());
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-slack")]
        "slack" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("slack requires bot_token")?;
            let mut channel = super::SlackChannel::new(
                bot_token.clone(),
                config.app_token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-mattermost")]
        "mattermost" => {
            let base_url = config
                .base_url
                .as_ref()
                .ok_or("mattermost requires base_url")?;
            let token = config.token.as_ref().ok_or("mattermost requires token")?;
            let mut channel = super::MattermostChannel::new(
                base_url.clone(),
                token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-matrix")]
        "matrix" => {
            let homeserver = config
                .homeserver
                .as_ref()
                .ok_or("matrix requires homeserver")?;
            let access_token = config
                .access_token
                .as_ref()
                .ok_or("matrix requires access_token")?;
            let room_id = config.room_id.clone().unwrap_or_default();
            let mut channel = super::MatrixChannel::new(
                homeserver.clone(),
                access_token.clone(),
                room_id,
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 40)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-email")]
        "email" => {
            use super::email::EmailConfig;
            let smtp_host = config
                .smtp_host
                .as_ref()
                .ok_or("email requires smtp_host")?;
            let imap_host = config
                .imap_host
                .as_ref()
                .ok_or("email requires imap_host")?;
            let username = config.username.as_ref().ok_or("email requires username")?;
            let password = config.password.as_ref().ok_or("email requires password")?;
            let from_address = config
                .from_address
                .as_ref()
                .ok_or("email requires from_address")?;
            let email_config = EmailConfig {
                smtp_host: smtp_host.clone(),
                smtp_port: config.smtp_port.unwrap_or(587),
                imap_host: imap_host.clone(),
                imap_port: config.imap_port.unwrap_or(993),
                username: username.clone(),
                password: password.clone(),
                from_address: from_address.clone(),
                allowed_senders: config.allowed_senders.clone(),
            };
            let channel = super::EmailChannel::new(email_config);
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-irc")]
        "irc" => {
            let server = config.server.as_ref().ok_or("irc requires server")?;
            let nick = config.nick.as_ref().ok_or("irc requires nick")?;
            let channel_name = config
                .channel_name
                .as_ref()
                .ok_or("irc requires channel_name")?;
            let channel = super::IrcChannel::new(
                server.clone(),
                config.port.unwrap_or(6667),
                nick.clone(),
                channel_name.clone(),
                config.password.clone(),
                config.allowed_users.clone(),
            );
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-nostr")]
        "nostr" => {
            let relay_url = config
                .relay_url
                .as_ref()
                .ok_or("nostr requires relay_url")?;
            let private_key_hex = config
                .private_key_hex
                .as_ref()
                .ok_or("nostr requires private_key_hex")?;
            let channel = super::NostrChannel::new(
                relay_url.clone(),
                private_key_hex.clone(),
                config.allowed_pubkeys.clone(),
            );
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-whatsapp")]
        "whatsapp" => {
            let access_token = config
                .access_token
                .as_ref()
                .ok_or("whatsapp requires access_token")?;
            let phone_number_id = config
                .channel_id
                .as_ref()
                .ok_or("whatsapp requires channel_id (phone_number_id)")?;
            let verify_token = config.token.clone().unwrap_or_default();
            let mut channel = super::WhatsappChannel::new(
                access_token.clone(),
                phone_number_id.clone(),
                verify_token,
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-sms")]
        "sms" => {
            let account_sid = config
                .account_sid
                .as_ref()
                .ok_or("sms requires account_sid")?;
            let auth_token = config
                .token
                .as_ref()
                .ok_or("sms requires token (auth_token)")?;
            let from_number = config
                .from_number
                .as_ref()
                .ok_or("sms requires from_number")?;
            let mut channel = super::SmsChannel::new(
                account_sid.clone(),
                auth_token.clone(),
                from_number.clone(),
                config.allowed_users.clone(),
            );
            if config.has_proxy() {
                channel = channel.with_client(build_channel_client(config, 30)?);
            }
            registry.register(Arc::new(channel));
            Ok(true)
        }

        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_configs_registers_nothing() {
        let mut registry = ChannelRegistry::new();
        let configs = HashMap::new();
        let errors = register_configured_channels(&mut registry, &configs);
        assert!(errors.is_empty());
        assert!(registry.channel_names().is_empty());
    }

    #[test]
    fn unknown_channel_is_silently_skipped() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert(
            "nonexistent-channel".to_string(),
            ChannelInstanceConfig::default(),
        );
        let errors = register_configured_channels(&mut registry, &configs);
        assert!(errors.is_empty());
        assert!(!registry.has_channel("nonexistent-channel"));
    }

    #[cfg(feature = "channel-telegram")]
    #[test]
    fn telegram_missing_bot_token_returns_error() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert("telegram".to_string(), ChannelInstanceConfig::default());
        let errors = register_configured_channels(&mut registry, &configs);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].1.contains("bot_token"));
    }

    #[cfg(feature = "channel-telegram")]
    #[test]
    fn telegram_with_bot_token_registers() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert(
            "telegram".to_string(),
            ChannelInstanceConfig {
                bot_token: Some("fake-token".into()),
                ..Default::default()
            },
        );
        let errors = register_configured_channels(&mut registry, &configs);
        assert!(errors.is_empty());
        assert!(registry.has_channel("telegram"));
    }

    #[test]
    fn channel_instance_config_privacy_boundary_defaults_empty() {
        let cfg = ChannelInstanceConfig::default();
        assert_eq!(cfg.privacy_boundary, "");
    }

    #[test]
    fn channel_instance_config_with_privacy_boundary() {
        let cfg = ChannelInstanceConfig {
            privacy_boundary: "local_only".to_string(),
            ..Default::default()
        };
        assert_eq!(cfg.privacy_boundary, "local_only");
    }

    #[test]
    fn channel_instance_config_proxy_defaults_none() {
        let cfg = ChannelInstanceConfig::default();
        assert!(cfg.http_proxy.is_none());
        assert!(cfg.https_proxy.is_none());
        assert!(cfg.socks_proxy.is_none());
        assert!(cfg.no_proxy.is_empty());
    }

    #[test]
    fn channel_instance_config_with_proxy() {
        let cfg = ChannelInstanceConfig {
            http_proxy: Some("http://proxy:8080".into()),
            socks_proxy: Some("socks5://127.0.0.1:1080".into()),
            no_proxy: vec!["localhost".into()],
            ..Default::default()
        };
        assert_eq!(cfg.http_proxy.as_deref(), Some("http://proxy:8080"));
        assert_eq!(cfg.socks_proxy.as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(cfg.no_proxy, vec!["localhost"]);
    }

    #[test]
    fn channel_instance_config_proxy_deserializes() {
        let json_str = r#"{
            "bot_token": "test-token",
            "http_proxy": "http://proxy:8080",
            "socks_proxy": "socks5://127.0.0.1:1080",
            "no_proxy": ["localhost", "*.internal"]
        }"#;
        let cfg: ChannelInstanceConfig = serde_json::from_str(json_str).expect("should parse JSON");
        assert_eq!(cfg.bot_token.as_deref(), Some("test-token"));
        assert_eq!(cfg.http_proxy.as_deref(), Some("http://proxy:8080"));
        assert!(cfg.https_proxy.is_none());
        assert_eq!(cfg.socks_proxy.as_deref(), Some("socks5://127.0.0.1:1080"));
        assert_eq!(cfg.no_proxy, vec!["localhost", "*.internal"]);
    }

    #[cfg(feature = "channel-whatsapp")]
    #[test]
    fn whatsapp_missing_access_token_returns_error() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert("whatsapp".to_string(), ChannelInstanceConfig::default());
        let errors = register_configured_channels(&mut registry, &configs);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].1.contains("access_token"));
    }

    #[cfg(feature = "channel-whatsapp")]
    #[test]
    fn whatsapp_with_required_fields_registers() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert(
            "whatsapp".to_string(),
            ChannelInstanceConfig {
                access_token: Some("EAAtest".into()),
                channel_id: Some("12345678901".into()),
                ..Default::default()
            },
        );
        let errors = register_configured_channels(&mut registry, &configs);
        assert!(errors.is_empty());
        assert!(registry.has_channel("whatsapp"));
    }

    #[cfg(feature = "channel-sms")]
    #[test]
    fn sms_missing_account_sid_returns_error() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert("sms".to_string(), ChannelInstanceConfig::default());
        let errors = register_configured_channels(&mut registry, &configs);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].1.contains("account_sid"));
    }

    #[cfg(feature = "channel-sms")]
    #[test]
    fn sms_with_required_fields_registers() {
        let mut registry = ChannelRegistry::new();
        let mut configs = HashMap::new();
        configs.insert(
            "sms".to_string(),
            ChannelInstanceConfig {
                account_sid: Some("ACtest000000000000000000000000000".into()),
                token: Some("auth_token_test".into()),
                from_number: Some("+15550001234".into()),
                ..Default::default()
            },
        );
        let errors = register_configured_channels(&mut registry, &configs);
        assert!(errors.is_empty());
        assert!(registry.has_channel("sms"));
    }
}

#[allow(unused_imports)]
use crate::ChannelRegistry;
use serde::Deserialize;
use std::collections::HashMap;
#[allow(unused_imports)]
use std::sync::Arc;

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
            let channel =
                super::TelegramChannel::new(bot_token.clone(), config.allowed_users.clone());
            Ok(Some(Arc::new(channel)))
        }
        #[cfg(feature = "channel-discord")]
        "discord" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("discord requires bot_token")?;
            let channel =
                super::DiscordChannel::new(bot_token.clone(), config.allowed_users.clone());
            Ok(Some(Arc::new(channel)))
        }
        #[cfg(feature = "channel-slack")]
        "slack" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("slack requires bot_token")?;
            let channel = super::SlackChannel::new(
                bot_token.clone(),
                config.app_token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
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
            let channel =
                super::TelegramChannel::new(bot_token.clone(), config.allowed_users.clone());
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-discord")]
        "discord" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("discord requires bot_token")?;
            let channel =
                super::DiscordChannel::new(bot_token.clone(), config.allowed_users.clone());
            registry.register(Arc::new(channel));
            Ok(true)
        }

        #[cfg(feature = "channel-slack")]
        "slack" => {
            let bot_token = config
                .bot_token
                .as_ref()
                .ok_or("slack requires bot_token")?;
            let channel = super::SlackChannel::new(
                bot_token.clone(),
                config.app_token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
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
            let channel = super::MattermostChannel::new(
                base_url.clone(),
                token.clone(),
                config.channel_id.clone(),
                config.allowed_users.clone(),
            );
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
            let channel = super::MatrixChannel::new(
                homeserver.clone(),
                access_token.clone(),
                room_id,
                config.allowed_users.clone(),
            );
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
            let channel = super::WhatsappChannel::new(
                access_token.clone(),
                phone_number_id.clone(),
                verify_token,
                config.allowed_users.clone(),
            );
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
            let channel = super::SmsChannel::new(
                account_sid.clone(),
                auth_token.clone(),
                from_number.clone(),
                config.allowed_users.clone(),
            );
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

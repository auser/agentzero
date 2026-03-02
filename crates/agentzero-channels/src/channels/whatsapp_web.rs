#[cfg(feature = "channel-whatsapp-web")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(WHATSAPP_WEB_DESCRIPTOR, "whatsapp-web", "WhatsApp Web");

    /// Configuration for the WhatsApp Web channel.
    #[derive(Debug, Clone)]
    pub struct WhatsappWebConfig {
        /// Path to session data directory for persistent login.
        pub session_path: String,
        /// Pairing mode: "qr" for QR code, "code" for pairing code.
        pub pairing_mode: String,
        /// Phone number for pairing code mode (e.g. "+1234567890").
        pub phone_number: Option<String>,
        /// Allowed sender JIDs. Empty = allow all.
        pub allowed_users: Vec<String>,
    }

    impl Default for WhatsappWebConfig {
        fn default() -> Self {
            Self {
                session_path: ".agentzero/whatsapp-session".to_string(),
                pairing_mode: "qr".to_string(),
                phone_number: None,
                allowed_users: vec![],
            }
        }
    }

    /// WhatsApp Web channel — connects via the WhatsApp Web multi-device
    /// protocol. Requires a browser-based pairing step (QR code or pairing
    /// code) on first use. Session data is persisted for reconnection.
    ///
    /// This is a structural implementation. The actual WhatsApp Web protocol
    /// integration (Signal encryption, protobuf, noise pipes) requires the
    /// `wa-rs` or equivalent crate, which is deferred to future work.
    pub struct WhatsappWebChannel {
        config: WhatsappWebConfig,
    }

    impl WhatsappWebChannel {
        pub fn new(config: WhatsappWebConfig) -> Self {
            Self { config }
        }

        pub fn from_defaults() -> Self {
            Self::new(WhatsappWebConfig::default())
        }

        /// Check if a sender is allowed based on the allowlist.
        fn is_allowed(&self, sender: &str) -> bool {
            helpers::is_user_allowed(sender, &self.config.allowed_users)
        }
    }

    #[async_trait]
    impl Channel for WhatsappWebChannel {
        fn name(&self) -> &str {
            "whatsapp-web"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            if self.config.session_path.is_empty() {
                anyhow::bail!("whatsapp-web session_path is not configured");
            }
            tracing::debug!(
                recipient = %message.recipient,
                bytes = message.content.len(),
                "sending WhatsApp Web message"
            );
            // Actual send via WhatsApp Web protocol would go here.
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            if self.config.session_path.is_empty() {
                anyhow::bail!("whatsapp-web session_path is not configured");
            }
            tracing::info!(
                session_path = %self.config.session_path,
                pairing_mode = %self.config.pairing_mode,
                "WhatsApp Web listener started (awaiting protocol integration)"
            );
            // Actual Web protocol listener with pairing would go here.
            Ok(())
        }

        async fn health_check(&self) -> bool {
            !self.config.session_path.is_empty()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn whatsapp_web_channel_name() {
            let ch = WhatsappWebChannel::from_defaults();
            assert_eq!(ch.name(), "whatsapp-web");
        }

        #[tokio::test]
        async fn whatsapp_web_send_succeeds() {
            let ch = WhatsappWebChannel::from_defaults();
            let msg = SendMessage::new("hello", "+1234567890@s.whatsapp.net");
            assert!(ch.send(&msg).await.is_ok());
        }

        #[tokio::test]
        async fn whatsapp_web_send_fails_empty_session() {
            let ch = WhatsappWebChannel::new(WhatsappWebConfig {
                session_path: String::new(),
                ..WhatsappWebConfig::default()
            });
            let msg = SendMessage::new("hello", "user");
            let err = ch.send(&msg).await.expect_err("empty session should fail");
            assert!(err.to_string().contains("session_path"));
        }

        #[tokio::test]
        async fn whatsapp_web_listen_succeeds() {
            let ch = WhatsappWebChannel::from_defaults();
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            assert!(ch.listen(tx).await.is_ok());
        }

        #[tokio::test]
        async fn whatsapp_web_health_check() {
            let ch = WhatsappWebChannel::from_defaults();
            assert!(ch.health_check().await);

            let empty = WhatsappWebChannel::new(WhatsappWebConfig {
                session_path: String::new(),
                ..WhatsappWebConfig::default()
            });
            assert!(!empty.health_check().await);
        }

        #[test]
        fn whatsapp_web_allowlist() {
            let ch = WhatsappWebChannel::new(WhatsappWebConfig {
                allowed_users: vec!["alice@s.whatsapp.net".to_string()],
                ..WhatsappWebConfig::default()
            });
            assert!(ch.is_allowed("alice@s.whatsapp.net"));
            assert!(!ch.is_allowed("bob@s.whatsapp.net"));

            // Empty allowlist = allow all
            let open = WhatsappWebChannel::from_defaults();
            assert!(open.is_allowed("anyone"));
        }

        #[test]
        fn whatsapp_web_config_defaults() {
            let config = WhatsappWebConfig::default();
            assert_eq!(config.pairing_mode, "qr");
            assert!(config.phone_number.is_none());
            assert!(config.allowed_users.is_empty());
        }
    }
}

#[cfg(feature = "channel-whatsapp-web")]
pub use impl_::*;

#[cfg(not(feature = "channel-whatsapp-web"))]
super::channel_stub!(
    WhatsappWebChannel,
    WHATSAPP_WEB_DESCRIPTOR,
    "whatsapp-web",
    "WhatsApp Web"
);

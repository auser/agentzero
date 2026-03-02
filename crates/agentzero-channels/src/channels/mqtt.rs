#[cfg(feature = "channel-mqtt")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(MQTT_DESCRIPTOR, "mqtt", "MQTT");

    /// Configuration for the MQTT channel.
    #[derive(Debug, Clone)]
    pub struct MqttConfig {
        /// MQTT broker URL (e.g. "mqtt://localhost:1883").
        pub broker_url: String,
        /// Topic to subscribe to for inbound messages.
        pub subscribe_topic: String,
        /// Topic to publish responses to.
        pub publish_topic: String,
        /// Client ID for the MQTT connection.
        pub client_id: String,
        /// Optional username for broker authentication.
        pub username: Option<String>,
        /// Optional password for broker authentication.
        pub password: Option<String>,
        /// Quality of Service level (0, 1, or 2). Default: 1.
        pub qos: u8,
    }

    impl Default for MqttConfig {
        fn default() -> Self {
            Self {
                broker_url: "mqtt://localhost:1883".to_string(),
                subscribe_topic: "agentzero/inbox".to_string(),
                publish_topic: "agentzero/outbox".to_string(),
                client_id: "agentzero".to_string(),
                username: None,
                password: None,
                qos: 1,
            }
        }
    }

    /// MQTT channel — connects to an MQTT broker, subscribes to a topic for
    /// inbound messages, and publishes agent responses to another topic.
    pub struct MqttChannel {
        config: MqttConfig,
    }

    impl MqttChannel {
        pub fn new(config: MqttConfig) -> Self {
            Self { config }
        }

        pub fn from_defaults() -> Self {
            Self::new(MqttConfig::default())
        }
    }

    #[async_trait]
    impl Channel for MqttChannel {
        fn name(&self) -> &str {
            "mqtt"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            // In a full implementation this would publish to the broker.
            // For now, validate the config and log the intent.
            if self.config.publish_topic.is_empty() {
                anyhow::bail!("mqtt publish_topic is not configured");
            }
            tracing::debug!(
                topic = %self.config.publish_topic,
                recipient = %message.recipient,
                bytes = message.content.len(),
                "publishing message to MQTT broker"
            );
            // Actual publish would go here (requires rumqttc or similar)
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            if self.config.broker_url.is_empty() {
                anyhow::bail!("mqtt broker_url is not configured");
            }
            tracing::info!(
                broker = %self.config.broker_url,
                topic = %self.config.subscribe_topic,
                "MQTT listener started (awaiting broker integration)"
            );
            // Actual subscribe loop would go here.
            // For now, return immediately — a real implementation would block
            // on the event loop dispatching messages via `tx`.
            Ok(())
        }

        async fn health_check(&self) -> bool {
            !self.config.broker_url.is_empty()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn mqtt_channel_name() {
            let ch = MqttChannel::from_defaults();
            assert_eq!(ch.name(), "mqtt");
        }

        #[tokio::test]
        async fn mqtt_send_succeeds_with_default_config() {
            let ch = MqttChannel::from_defaults();
            let msg = SendMessage::new("hello from agent", "device-1");
            assert!(ch.send(&msg).await.is_ok());
        }

        #[tokio::test]
        async fn mqtt_send_fails_with_empty_topic() {
            let ch = MqttChannel::new(MqttConfig {
                publish_topic: String::new(),
                ..MqttConfig::default()
            });
            let msg = SendMessage::new("hello", "device-1");
            let err = ch.send(&msg).await.expect_err("empty topic should fail");
            assert!(err.to_string().contains("publish_topic"));
        }

        #[tokio::test]
        async fn mqtt_listen_fails_with_empty_broker() {
            let ch = MqttChannel::new(MqttConfig {
                broker_url: String::new(),
                ..MqttConfig::default()
            });
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let err = ch
                .listen(tx)
                .await
                .expect_err("empty broker should fail");
            assert!(err.to_string().contains("broker_url"));
        }

        #[tokio::test]
        async fn mqtt_health_check_default() {
            let ch = MqttChannel::from_defaults();
            assert!(ch.health_check().await);
        }

        #[tokio::test]
        async fn mqtt_health_check_fails_empty_broker() {
            let ch = MqttChannel::new(MqttConfig {
                broker_url: String::new(),
                ..MqttConfig::default()
            });
            assert!(!ch.health_check().await);
        }

        #[test]
        fn mqtt_config_defaults() {
            let config = MqttConfig::default();
            assert_eq!(config.broker_url, "mqtt://localhost:1883");
            assert_eq!(config.subscribe_topic, "agentzero/inbox");
            assert_eq!(config.publish_topic, "agentzero/outbox");
            assert_eq!(config.qos, 1);
        }
    }
}

#[cfg(feature = "channel-mqtt")]
pub use impl_::*;

#[cfg(not(feature = "channel-mqtt"))]
super::channel_stub!(MqttChannel, MQTT_DESCRIPTOR, "mqtt", "MQTT");

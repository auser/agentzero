#[cfg(feature = "channel-discord-history")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(DISCORD_HISTORY_DESCRIPTOR, "discord-history", "Discord History");

    /// Shadow listener that logs Discord messages to SQLite for searchable history.
    /// Does not respond to messages — only records them.
    pub struct DiscordHistoryChannel {
        bot_token: String,
    }

    impl DiscordHistoryChannel {
        pub fn new(bot_token: String) -> Self {
            Self { bot_token }
        }
    }

    #[async_trait]
    impl Channel for DiscordHistoryChannel {
        fn name(&self) -> &str {
            "discord-history"
        }

        async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
            // History channel is read-only — does not send messages
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // TODO: Connect to Discord gateway, log messages to SQLite
            tracing::info!("discord-history channel started (logging mode)");
            // For now, sleep forever as a placeholder
            tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
            Ok(())
        }
    }
}

#[cfg(feature = "channel-discord-history")]
pub use impl_::*;

#[cfg(not(feature = "channel-discord-history"))]
super::channel_stub!(
    DiscordHistoryChannel,
    DISCORD_HISTORY_DESCRIPTOR,
    "discord-history",
    "Discord History"
);

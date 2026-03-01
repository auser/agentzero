use super::helpers;
use crate::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use tokio::io::{self, AsyncBufReadExt, BufReader};

super::channel_meta!(CLI_DESCRIPTOR, "cli", "CLI");

pub struct CliChannel;

#[async_trait]
impl Channel for CliChannel {
    fn name(&self) -> &str {
        "cli"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        println!("{}", message.content);
        Ok(())
    }

    async fn listen(
        &self,
        tx: tokio::sync::mpsc::Sender<ChannelMessage>,
    ) -> anyhow::Result<()> {
        let stdin = io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let line = line.trim().to_string();
            if line.is_empty() {
                continue;
            }
            if line == "/quit" || line == "/exit" {
                break;
            }

            let msg = ChannelMessage {
                id: helpers::new_message_id(),
                sender: "user".to_string(),
                reply_target: "user".to_string(),
                content: line,
                channel: "cli".to_string(),
                timestamp: helpers::now_epoch_secs(),
                thread_ts: None,
            };

            if tx.send(msg).await.is_err() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_channel_name() {
        let ch = CliChannel;
        assert_eq!(ch.name(), "cli");
    }

    #[tokio::test]
    async fn cli_channel_send_does_not_panic() {
        let ch = CliChannel;
        let msg = SendMessage::new("test output", "user");
        assert!(ch.send(&msg).await.is_ok());
    }

    #[tokio::test]
    async fn cli_channel_health_check() {
        let ch = CliChannel;
        assert!(ch.health_check().await);
    }
}

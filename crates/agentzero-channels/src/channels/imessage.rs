#[cfg(feature = "channel-imessage")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;
    use tokio::process::Command;

    super::super::channel_meta!(IMESSAGE_DESCRIPTOR, "imessage", "iMessage");

    const MAX_MESSAGE_LENGTH: usize = 20000;
    const POLL_INTERVAL_SECS: u64 = 3;

    /// iMessage channel via macOS AppleScript (osascript).
    pub struct ImessageChannel {
        allowed_users: Vec<String>,
    }

    impl ImessageChannel {
        pub fn new(allowed_users: Vec<String>) -> Self {
            Self { allowed_users }
        }

        async fn run_osascript(script: &str) -> anyhow::Result<String> {
            let output = Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output()
                .await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                anyhow::bail!("osascript failed: {stderr}");
            }
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
    }

    #[async_trait]
    impl Channel for ImessageChannel {
        fn name(&self) -> &str {
            "imessage"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for chunk in chunks {
                // Escape double quotes and backslashes for AppleScript string
                let escaped = chunk
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"");
                let script = format!(
                    r#"tell application "Messages"
    set targetService to 1st account whose service type = iMessage
    set targetBuddy to participant "{}" of targetService
    send "{}" to targetBuddy
end tell"#,
                    message.recipient, escaped
                );
                Self::run_osascript(&script).await?;
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // Poll the Messages SQLite database for new messages.
            // The chat.db is at ~/Library/Messages/chat.db on macOS.
            let db_path = dirs_path();
            let mut last_rowid: i64 = get_max_rowid(&db_path).await;

            loop {
                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;

                let query = format!(
                    "SELECT m.ROWID, m.text, h.id FROM message m \
                     JOIN handle h ON m.handle_id = h.ROWID \
                     WHERE m.is_from_me = 0 AND m.ROWID > {} \
                     ORDER BY m.ROWID ASC LIMIT 50",
                    last_rowid
                );
                let output = match Command::new("sqlite3")
                    .arg("-separator")
                    .arg("|")
                    .arg(&db_path)
                    .arg(&query)
                    .output()
                    .await
                {
                    Ok(o) => o,
                    Err(e) => {
                        tracing::error!(error = %e, "imessage: sqlite3 query failed");
                        continue;
                    }
                };

                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let parts: Vec<&str> = line.splitn(3, '|').collect();
                    if parts.len() < 3 {
                        continue;
                    }
                    let rowid: i64 = parts[0].parse().unwrap_or(0);
                    let text = parts[1];
                    let sender = parts[2];
                    if rowid > last_rowid {
                        last_rowid = rowid;
                    }
                    if text.is_empty() || sender.is_empty() {
                        continue;
                    }
                    if !helpers::is_user_allowed(sender, &self.allowed_users) {
                        continue;
                    }
                    let msg = ChannelMessage {
                        id: helpers::new_message_id(),
                        sender: sender.to_string(),
                        reply_target: sender.to_string(),
                        content: text.to_string(),
                        channel: "imessage".to_string(),
                        timestamp: helpers::now_epoch_secs(),
                        thread_ts: None,
                        privacy_boundary: String::new(),
                        attachments: Vec::new(),
                    };
                    if tx.send(msg).await.is_err() {
                        return Ok(());
                    }
                }
            }
        }

        async fn health_check(&self) -> bool {
            // Check if Messages.app process is running
            Command::new("pgrep")
                .arg("-x")
                .arg("Messages")
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    }

    fn dirs_path() -> String {
        if let Ok(home) = std::env::var("HOME") {
            format!("{home}/Library/Messages/chat.db")
        } else {
            "/Users/Shared/Library/Messages/chat.db".to_string()
        }
    }

    async fn get_max_rowid(db_path: &str) -> i64 {
        Command::new("sqlite3")
            .arg(db_path)
            .arg("SELECT COALESCE(MAX(ROWID),0) FROM message")
            .output()
            .await
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<i64>()
                    .ok()
            })
            .unwrap_or(0)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn imessage_channel_name() {
            let ch = ImessageChannel::new(vec![]);
            assert_eq!(ch.name(), "imessage");
        }

        #[test]
        fn dirs_path_uses_home_env() {
            // Just ensure it returns a non-empty string
            let path = dirs_path();
            assert!(path.contains("Library/Messages/chat.db"));
        }
    }
}

#[cfg(feature = "channel-imessage")]
pub use impl_::*;

#[cfg(not(feature = "channel-imessage"))]
super::channel_stub!(ImessageChannel, IMESSAGE_DESCRIPTOR, "imessage", "iMessage");

#[cfg(feature = "channel-irc")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;
    use tokio::sync::Mutex;

    super::super::channel_meta!(IRC_DESCRIPTOR, "irc", "IRC");

    const MAX_IRC_LINE: usize = 510; // RFC 2812: 512 including CRLF
    const PRIVMSG_OVERHEAD: usize = 50; // "PRIVMSG #channel :" prefix overhead

    pub struct IrcChannel {
        server: String,
        port: u16,
        nick: String,
        channel_name: String,
        password: Option<String>,
        allowed_users: Vec<String>,
        writer: Mutex<Option<tokio::io::WriteHalf<TcpStream>>>,
    }

    impl IrcChannel {
        pub fn new(
            server: String,
            port: u16,
            nick: String,
            channel_name: String,
            password: Option<String>,
            allowed_users: Vec<String>,
        ) -> Self {
            Self {
                server,
                port,
                nick,
                channel_name,
                password,
                allowed_users,
                writer: Mutex::new(None),
            }
        }

        /// Send a raw IRC line (appends CRLF).
        async fn send_raw(
            writer: &mut tokio::io::WriteHalf<TcpStream>,
            line: &str,
        ) -> anyhow::Result<()> {
            writer.write_all(line.as_bytes()).await?;
            writer.write_all(b"\r\n").await?;
            writer.flush().await?;
            Ok(())
        }

        /// Perform IRC registration (PASS, NICK, USER, JOIN).
        async fn register(
            writer: &mut tokio::io::WriteHalf<TcpStream>,
            nick: &str,
            channel_name: &str,
            password: Option<&str>,
        ) -> anyhow::Result<()> {
            if let Some(pass) = password {
                Self::send_raw(writer, &format!("PASS {pass}")).await?;
            }
            Self::send_raw(writer, &format!("NICK {nick}")).await?;
            Self::send_raw(writer, &format!("USER {nick} 0 * :{nick}")).await?;
            Self::send_raw(writer, &format!("JOIN {channel_name}")).await?;
            Ok(())
        }

        /// Parse an IRC message prefix to extract the nick.
        fn parse_nick(prefix: &str) -> &str {
            prefix.split('!').next().unwrap_or(prefix)
        }
    }

    #[async_trait]
    impl Channel for IrcChannel {
        fn name(&self) -> &str {
            "irc"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let mut guard = self.writer.lock().await;
            let writer = guard
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("irc: not connected"))?;

            let target = &message.recipient;
            let max_content = MAX_IRC_LINE.saturating_sub(PRIVMSG_OVERHEAD + target.len());
            let chunks = helpers::split_message(&message.content, max_content);

            for chunk in chunks {
                // IRC PRIVMSG doesn't support newlines; send each line separately
                for line in chunk.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    Self::send_raw(writer, &format!("PRIVMSG {target} :{line}")).await?;
                }
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            let addr = format!("{}:{}", self.server, self.port);
            let stream = TcpStream::connect(&addr).await?;
            let (reader_half, mut writer_half) = tokio::io::split(stream);

            Self::register(
                &mut writer_half,
                &self.nick,
                &self.channel_name,
                self.password.as_deref(),
            )
            .await?;

            // Store writer for send()
            {
                let mut guard = self.writer.lock().await;
                *guard = Some(writer_half);
            }

            let mut reader = BufReader::new(reader_half);
            let mut line_buf = String::new();

            loop {
                line_buf.clear();
                let n = reader.read_line(&mut line_buf).await?;
                if n == 0 {
                    break; // Connection closed
                }

                let line = line_buf.trim_end();

                // Respond to PING to stay connected
                if line.starts_with("PING") {
                    let payload = line.strip_prefix("PING ").unwrap_or(":");
                    let mut guard = self.writer.lock().await;
                    if let Some(writer) = guard.as_mut() {
                        let _ = Self::send_raw(writer, &format!("PONG {payload}")).await;
                    }
                    continue;
                }

                // Parse PRIVMSG: :nick!user@host PRIVMSG #channel :message
                let Some(line) = line.strip_prefix(':') else {
                    continue;
                };

                // Format: "nick!user@host PRIVMSG target :content"
                let mut words = line.splitn(4, ' ');
                let prefix = match words.next() {
                    Some(p) => p,
                    None => continue,
                };
                let command = match words.next() {
                    Some(c) => c,
                    None => continue,
                };
                if command != "PRIVMSG" {
                    continue;
                }
                let target = match words.next() {
                    Some(t) => t,
                    None => continue,
                };
                let raw_content = words.next().unwrap_or("");
                let content = raw_content.strip_prefix(':').unwrap_or(raw_content);

                let nick = Self::parse_nick(prefix);

                if nick == self.nick {
                    continue;
                }

                if !helpers::is_user_allowed(nick, &self.allowed_users) {
                    continue;
                }

                if content.is_empty() {
                    continue;
                }

                // If sent to a channel (#/&), reply there; if DM, reply to sender
                let reply_target = if target.starts_with('#') || target.starts_with('&') {
                    target.to_string()
                } else {
                    nick.to_string()
                };

                let msg = ChannelMessage {
                    id: helpers::new_message_id(),
                    sender: nick.to_string(),
                    reply_target,
                    content: content.to_string(),
                    channel: "irc".to_string(),
                    timestamp: helpers::now_epoch_secs(),
                    thread_ts: None,
                };

                if tx.send(msg).await.is_err() {
                    break;
                }
            }

            Ok(())
        }

        async fn health_check(&self) -> bool {
            let guard = self.writer.lock().await;
            guard.is_some()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn irc_channel_name() {
            let ch = IrcChannel::new(
                "irc.example.com".into(),
                6667,
                "testbot".into(),
                "#test".into(),
                None,
                vec![],
            );
            assert_eq!(ch.name(), "irc");
        }

        #[test]
        fn parse_nick_extracts_from_prefix() {
            assert_eq!(IrcChannel::parse_nick("alice!user@host"), "alice");
            assert_eq!(IrcChannel::parse_nick("bob"), "bob");
        }

        #[test]
        fn irc_health_check_false_when_not_connected() {
            let ch = IrcChannel::new(
                "irc.example.com".into(),
                6667,
                "testbot".into(),
                "#test".into(),
                None,
                vec![],
            );
            let rt = tokio::runtime::Runtime::new().unwrap();
            assert!(!rt.block_on(ch.health_check()));
        }
    }
}

#[cfg(feature = "channel-irc")]
pub use impl_::*;

#[cfg(not(feature = "channel-irc"))]
super::channel_stub!(IrcChannel, IRC_DESCRIPTOR, "irc", "IRC");

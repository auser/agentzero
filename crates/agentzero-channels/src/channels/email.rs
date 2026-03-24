#[cfg(feature = "channel-email")]
#[allow(dead_code)]
mod impl_ {
    use crate::channels::helpers;
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    super::super::channel_meta!(EMAIL_DESCRIPTOR, "email", "Email");

    const POLL_INTERVAL_SECS: u64 = 30;
    const MAX_MESSAGE_LENGTH: usize = 50000;

    pub struct EmailConfig {
        pub smtp_host: String,
        pub smtp_port: u16,
        pub imap_host: String,
        pub imap_port: u16,
        pub username: String,
        pub password: String,
        pub from_address: String,
        pub allowed_senders: Vec<String>,
    }

    pub struct EmailChannel {
        smtp_host: String,
        smtp_port: u16,
        imap_host: String,
        imap_port: u16,
        username: String,
        password: String,
        from_address: String,
        allowed_senders: Vec<String>,
    }

    impl EmailChannel {
        pub fn new(config: EmailConfig) -> Self {
            Self {
                smtp_host: config.smtp_host,
                smtp_port: config.smtp_port,
                imap_host: config.imap_host,
                imap_port: config.imap_port,
                username: config.username,
                password: config.password,
                from_address: config.from_address,
                allowed_senders: config.allowed_senders,
            }
        }

        /// Send an email via raw SMTP.
        async fn smtp_send(
            &self,
            to: &str,
            subject: &str,
            body: &str,
        ) -> anyhow::Result<()> {
            let addr = format!("{}:{}", self.smtp_host, self.smtp_port);
            let stream = TcpStream::connect(&addr).await?;
            let (reader_half, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader_half);
            let mut line = String::new();

            // Read greeting
            Self::read_smtp_line(&mut reader, &mut line).await?;

            // EHLO
            Self::smtp_command(&mut writer, &mut reader, &mut line, "EHLO agentzero").await?;

            // AUTH LOGIN if credentials provided
            if !self.username.is_empty() {
                Self::smtp_command(&mut writer, &mut reader, &mut line, "AUTH LOGIN").await?;
                let user_b64 = base64_encode(self.username.as_bytes());
                Self::smtp_command(&mut writer, &mut reader, &mut line, &user_b64).await?;
                let pass_b64 = base64_encode(self.password.as_bytes());
                Self::smtp_command(&mut writer, &mut reader, &mut line, &pass_b64).await?;
            }

            // MAIL FROM
            Self::smtp_command(
                &mut writer,
                &mut reader,
                &mut line,
                &format!("MAIL FROM:<{}>", self.from_address),
            )
            .await?;

            // RCPT TO
            Self::smtp_command(
                &mut writer,
                &mut reader,
                &mut line,
                &format!("RCPT TO:<{to}>"),
            )
            .await?;

            // DATA
            Self::smtp_command(&mut writer, &mut reader, &mut line, "DATA").await?;

            // Message headers + body
            let message = format!(
                "From: {}\r\nTo: {}\r\nSubject: {}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{}\r\n.",
                self.from_address, to, subject, body
            );
            Self::smtp_command(&mut writer, &mut reader, &mut line, &message).await?;

            // QUIT
            let _ = Self::smtp_command(&mut writer, &mut reader, &mut line, "QUIT").await;

            Ok(())
        }

        async fn smtp_command(
            writer: &mut tokio::io::WriteHalf<TcpStream>,
            reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
            line: &mut String,
            command: &str,
        ) -> anyhow::Result<()> {
            writer.write_all(command.as_bytes()).await?;
            writer.write_all(b"\r\n").await?;
            writer.flush().await?;
            Self::read_smtp_line(reader, line).await
        }

        async fn read_smtp_line(
            reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
            line: &mut String,
        ) -> anyhow::Result<()> {
            line.clear();
            reader.read_line(line).await?;
            // Read continuation lines (multi-line responses: "250-..." then "250 ...")
            while line.len() >= 4 && line.as_bytes().get(3) == Some(&b'-') {
                let mut cont = String::new();
                reader.read_line(&mut cont).await?;
                line.push_str(&cont);
            }
            let code = line.get(..3).unwrap_or("");
            if code.starts_with('4') || code.starts_with('5') {
                anyhow::bail!("SMTP error: {}", line.trim());
            }
            Ok(())
        }

        /// Poll for new messages via basic IMAP.
        async fn imap_poll_unseen(&self) -> anyhow::Result<Vec<(String, String, String)>> {
            let addr = format!("{}:{}", self.imap_host, self.imap_port);
            let stream = TcpStream::connect(&addr).await?;
            let (reader_half, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader_half);
            let mut line = String::new();

            // Read greeting
            Self::imap_read_response(&mut reader, &mut line).await?;

            // LOGIN
            Self::imap_command(
                &mut writer,
                &mut reader,
                &mut line,
                &format!("A001 LOGIN {} {}", self.username, self.password),
            )
            .await?;

            // SELECT INBOX
            Self::imap_command(&mut writer, &mut reader, &mut line, "A002 SELECT INBOX").await?;

            // SEARCH UNSEEN
            Self::imap_command(&mut writer, &mut reader, &mut line, "A003 SEARCH UNSEEN").await?;

            let mut message_ids: Vec<String> = Vec::new();
            for response_line in line.lines() {
                if response_line.starts_with("* SEARCH") {
                    message_ids = response_line
                        .strip_prefix("* SEARCH")
                        .unwrap_or("")
                        .split_whitespace()
                        .map(String::from)
                        .collect();
                }
            }

            let mut results = Vec::new();

            // Fetch each unseen message (limited to newest 5)
            for msg_id in message_ids.iter().rev().take(5) {
                Self::imap_command(
                    &mut writer,
                    &mut reader,
                    &mut line,
                    &format!("A004 FETCH {msg_id} (BODY[HEADER.FIELDS (FROM SUBJECT)] BODY[TEXT])"),
                )
                .await?;

                let from = extract_header(&line, "From");
                let subject = extract_header(&line, "Subject");
                let body = extract_body(&line);

                if !from.is_empty() {
                    results.push((from, subject, body));
                }

                // Mark as seen
                Self::imap_command(
                    &mut writer,
                    &mut reader,
                    &mut line,
                    &format!("A005 STORE {msg_id} +FLAGS (\\Seen)"),
                )
                .await?;
            }

            // LOGOUT
            let _ = Self::imap_command(&mut writer, &mut reader, &mut line, "A006 LOGOUT").await;

            Ok(results)
        }

        async fn imap_command(
            writer: &mut tokio::io::WriteHalf<TcpStream>,
            reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
            line: &mut String,
            command: &str,
        ) -> anyhow::Result<()> {
            writer.write_all(command.as_bytes()).await?;
            writer.write_all(b"\r\n").await?;
            writer.flush().await?;
            Self::imap_read_response(reader, line).await
        }

        async fn imap_read_response(
            reader: &mut BufReader<tokio::io::ReadHalf<TcpStream>>,
            output: &mut String,
        ) -> anyhow::Result<()> {
            output.clear();
            loop {
                let mut line = String::new();
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    break;
                }
                output.push_str(&line);
                // IMAP tagged responses end with "Axx OK/NO/BAD ..."
                let trimmed = line.trim();
                if trimmed.starts_with("* OK")
                    || trimmed.contains(" OK ")
                    || trimmed.contains(" NO ")
                    || trimmed.contains(" BAD ")
                {
                    break;
                }
            }
            Ok(())
        }
    }

    /// Simple base64 encoder (no padding variant handling needed for SMTP AUTH).
    fn base64_encode(input: &[u8]) -> String {
        const CHARS: &[u8] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let triple = (b0 << 16) | (b1 << 8) | b2;
            result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
            result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
            if chunk.len() > 2 {
                result.push(CHARS[(triple & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
        }
        result
    }

    /// Extract a header value from raw IMAP FETCH output.
    fn extract_header(raw: &str, header: &str) -> String {
        let prefix = format!("{header}:");
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix(&prefix) {
                return value.trim().to_string();
            }
        }
        String::new()
    }

    /// Extract the body text from raw IMAP FETCH output.
    fn extract_body(raw: &str) -> String {
        // Look for empty line after headers (body separator in MIME)
        let mut in_body = false;
        let mut body_lines = Vec::new();

        for line in raw.lines() {
            if in_body {
                let trimmed = line.trim();
                // Stop at IMAP tagged response or closing paren
                if trimmed.starts_with("A00") || trimmed == ")" {
                    break;
                }
                body_lines.push(line);
            } else if line.trim().is_empty() {
                in_body = true;
            }
        }

        body_lines.join("\n").trim().to_string()
    }

    #[async_trait]
    impl Channel for EmailChannel {
        fn name(&self) -> &str {
            "email"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            let subject = message
                .subject
                .as_deref()
                .unwrap_or("Message from AgentZero");

            let chunks = helpers::split_message(&message.content, MAX_MESSAGE_LENGTH);
            for (i, chunk) in chunks.iter().enumerate() {
                let subj = if chunks.len() > 1 {
                    format!("{subject} ({}/{})", i + 1, chunks.len())
                } else {
                    subject.to_string()
                };
                self.smtp_send(&message.recipient, &subj, chunk).await?;
            }
            Ok(())
        }

        async fn listen(
            &self,
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            loop {
                match self.imap_poll_unseen().await {
                    Ok(messages) => {
                        for (from, subject, body) in messages {
                            let sender_email = extract_email_address(&from);

                            if !helpers::is_user_allowed(&sender_email, &self.allowed_senders) {
                                tracing::debug!(sender = %sender_email, "email: ignoring from unallowed sender");
                                continue;
                            }

                            let content = if subject.is_empty() {
                                body
                            } else {
                                format!("[{subject}] {body}")
                            };

                            if content.is_empty() {
                                continue;
                            }

                            let msg = ChannelMessage {
                                id: helpers::new_message_id(),
                                sender: sender_email.clone(),
                                reply_target: sender_email,
                                content,
                                channel: "email".to_string(),
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
                    Err(e) => {
                        tracing::error!(error = %e, "email imap poll failed");
                    }
                }

                tokio::time::sleep(Duration::from_secs(POLL_INTERVAL_SECS)).await;
            }
        }

        async fn health_check(&self) -> bool {
            // Try connecting to IMAP server
            let addr = format!("{}:{}", self.imap_host, self.imap_port);
            TcpStream::connect(&addr)
                .await
                .map(|_| true)
                .unwrap_or(false)
        }
    }

    /// Extract bare email address from "Name <email@example.com>" or "email@example.com".
    fn extract_email_address(from: &str) -> String {
        if let Some(start) = from.find('<') {
            if let Some(end) = from.find('>') {
                return from[start + 1..end].trim().to_string();
            }
        }
        from.trim().to_string()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn email_channel_name() {
            let ch = EmailChannel::new(EmailConfig {
                smtp_host: "smtp.example.com".into(),
                smtp_port: 587,
                imap_host: "imap.example.com".into(),
                imap_port: 143,
                username: "user".into(),
                password: "pass".into(),
                from_address: "bot@example.com".into(),
                allowed_senders: vec![],
            });
            assert_eq!(ch.name(), "email");
        }

        #[test]
        fn extract_email_from_display_name() {
            assert_eq!(
                extract_email_address("Alice <alice@example.com>"),
                "alice@example.com"
            );
            assert_eq!(
                extract_email_address("bob@example.com"),
                "bob@example.com"
            );
            assert_eq!(
                extract_email_address("  <admin@example.com>  "),
                "admin@example.com"
            );
        }

        #[test]
        fn extract_header_parses_from() {
            let raw = "From: alice@example.com\r\nSubject: Hello\r\n\r\nBody text";
            assert_eq!(extract_header(raw, "From"), "alice@example.com");
            assert_eq!(extract_header(raw, "Subject"), "Hello");
        }

        #[test]
        fn base64_encode_standard() {
            assert_eq!(base64_encode(b"Hello"), "SGVsbG8=");
            assert_eq!(base64_encode(b"ab"), "YWI=");
            assert_eq!(base64_encode(b"abc"), "YWJj");
        }

        #[test]
        fn extract_body_from_imap_output() {
            let raw = "From: alice@example.com\r\nSubject: Test\r\n\r\nHello world\r\nSecond line\r\n)\r\nA004 OK";
            let body = extract_body(raw);
            assert!(body.contains("Hello world"));
        }
    }
}

#[cfg(feature = "channel-email")]
pub use impl_::*;

#[cfg(not(feature = "channel-email"))]
super::channel_stub!(EmailChannel, EMAIL_DESCRIPTOR, "email", "Email");

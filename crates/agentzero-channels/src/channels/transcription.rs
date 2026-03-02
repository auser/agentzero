#[cfg(feature = "channel-transcription")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(TRANSCRIPTION_DESCRIPTOR, "transcription", "Transcription");

    /// Supported audio formats for transcription.
    const SUPPORTED_EXTENSIONS: &[&str] = &[
        "flac", "mp3", "mp4", "m4a", "ogg", "opus", "wav", "webm",
    ];

    /// Maximum audio file size (25 MB, matching Whisper API limits).
    const MAX_AUDIO_SIZE: usize = 25 * 1024 * 1024;

    /// Configuration for the transcription channel.
    #[derive(Debug, Clone)]
    pub struct TranscriptionConfig {
        /// Whisper-compatible API endpoint.
        pub api_url: String,
        /// API key for the transcription service.
        pub api_key: Option<String>,
        /// Language hint (e.g. "en").
        pub language: Option<String>,
    }

    impl Default for TranscriptionConfig {
        fn default() -> Self {
            Self {
                api_url: "https://api.groq.com/openai/v1/audio/transcriptions".to_string(),
                api_key: None,
                language: None,
            }
        }
    }

    /// Transcription channel — converts audio to text using a Whisper-compatible
    /// API and feeds the text into the agent. Responses are returned as text.
    pub struct TranscriptionChannel {
        config: TranscriptionConfig,
    }

    impl TranscriptionChannel {
        pub fn new(config: TranscriptionConfig) -> Self {
            Self { config }
        }

        pub fn from_defaults() -> Self {
            Self::new(TranscriptionConfig::default())
        }
    }

    /// Validate that a filename has a supported audio extension.
    pub fn is_supported_audio(filename: &str) -> bool {
        let lower = filename.to_ascii_lowercase();
        SUPPORTED_EXTENSIONS
            .iter()
            .any(|ext| lower.ends_with(ext))
    }

    /// Validate audio data size.
    pub fn validate_audio_size(size: usize) -> anyhow::Result<()> {
        if size > MAX_AUDIO_SIZE {
            anyhow::bail!(
                "audio file too large ({:.1} MB, max {} MB)",
                size as f64 / (1024.0 * 1024.0),
                MAX_AUDIO_SIZE / (1024 * 1024)
            );
        }
        if size == 0 {
            anyhow::bail!("audio file is empty");
        }
        Ok(())
    }

    #[async_trait]
    impl Channel for TranscriptionChannel {
        fn name(&self) -> &str {
            "transcription"
        }

        async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
            // Transcription is primarily inbound (audio → text). Send is a
            // no-op text response.
            tracing::debug!(
                recipient = %message.recipient,
                "transcription channel send (text response)"
            );
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            if self.config.api_key.is_none() {
                anyhow::bail!(
                    "transcription channel requires an API key (set GROQ_API_KEY or configure api_key)"
                );
            }
            tracing::info!(
                api_url = %self.config.api_url,
                "transcription listener started (awaiting audio pipeline integration)"
            );
            // Actual audio capture and transcription loop would go here.
            Ok(())
        }

        async fn health_check(&self) -> bool {
            self.config.api_key.is_some()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn transcription_channel_name() {
            let ch = TranscriptionChannel::from_defaults();
            assert_eq!(ch.name(), "transcription");
        }

        #[tokio::test]
        async fn transcription_send_succeeds() {
            let ch = TranscriptionChannel::from_defaults();
            let msg = SendMessage::new("response text", "user");
            assert!(ch.send(&msg).await.is_ok());
        }

        #[tokio::test]
        async fn transcription_listen_fails_without_api_key() {
            let ch = TranscriptionChannel::from_defaults();
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let err = ch
                .listen(tx)
                .await
                .expect_err("missing api_key should fail");
            assert!(err.to_string().contains("API key"));
        }

        #[tokio::test]
        async fn transcription_listen_succeeds_with_api_key() {
            let ch = TranscriptionChannel::new(TranscriptionConfig {
                api_key: Some("test-key".to_string()),
                ..TranscriptionConfig::default()
            });
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            assert!(ch.listen(tx).await.is_ok());
        }

        #[tokio::test]
        async fn transcription_health_check() {
            let without_key = TranscriptionChannel::from_defaults();
            assert!(!without_key.health_check().await);

            let with_key = TranscriptionChannel::new(TranscriptionConfig {
                api_key: Some("key".to_string()),
                ..TranscriptionConfig::default()
            });
            assert!(with_key.health_check().await);
        }

        #[test]
        fn supported_audio_formats() {
            assert!(is_supported_audio("recording.mp3"));
            assert!(is_supported_audio("audio.WAV"));
            assert!(is_supported_audio("voice.opus"));
            assert!(!is_supported_audio("document.pdf"));
            assert!(!is_supported_audio("image.png"));
        }

        #[test]
        fn validate_audio_size_limits() {
            assert!(validate_audio_size(1024).is_ok());
            assert!(validate_audio_size(0).is_err());
            assert!(validate_audio_size(30 * 1024 * 1024).is_err());
        }
    }
}

#[cfg(feature = "channel-transcription")]
pub use impl_::*;

#[cfg(not(feature = "channel-transcription"))]
super::channel_stub!(
    TranscriptionChannel,
    TRANSCRIPTION_DESCRIPTOR,
    "transcription",
    "Transcription"
);

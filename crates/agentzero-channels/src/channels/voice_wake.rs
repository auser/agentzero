#[cfg(feature = "channel-voice-wake")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;

    super::super::channel_meta!(VOICE_WAKE_DESCRIPTOR, "voice-wake", "Voice Wake Word");

    /// Voice-activated channel using energy-based VAD and wake word detection.
    /// Captures audio via cpal, transcribes via Whisper API, matches wake words.
    pub struct VoiceWakeChannel {
        wake_words: Vec<String>,
        energy_threshold: f32,
    }

    impl VoiceWakeChannel {
        pub fn new(wake_words: Vec<String>, energy_threshold: f32) -> Self {
            Self {
                wake_words,
                energy_threshold,
            }
        }
    }

    #[async_trait]
    impl Channel for VoiceWakeChannel {
        fn name(&self) -> &str {
            "voice-wake"
        }

        async fn send(&self, _message: &SendMessage) -> anyhow::Result<()> {
            // Voice channel is input-only
            Ok(())
        }

        async fn listen(
            &self,
            _tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            // TODO: cpal audio capture, VAD state machine, wake word matching
            tracing::info!(wake_words = ?self.wake_words, "voice-wake channel started");
            tokio::time::sleep(std::time::Duration::from_secs(u64::MAX)).await;
            Ok(())
        }
    }
}

#[cfg(feature = "channel-voice-wake")]
pub use impl_::*;

#[cfg(not(feature = "channel-voice-wake"))]
super::channel_stub!(
    VoiceWakeChannel,
    VOICE_WAKE_DESCRIPTOR,
    "voice-wake",
    "Voice Wake Word"
);

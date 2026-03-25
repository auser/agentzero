#[cfg(feature = "channel-voice-wake")]
#[allow(dead_code)]
mod impl_ {
    use crate::{Channel, ChannelMessage, SendMessage};
    use async_trait::async_trait;
    use std::time::Duration;

    super::super::channel_meta!(VOICE_WAKE_DESCRIPTOR, "voice-wake", "Voice Wake Word");

    /// VAD (Voice Activity Detection) state machine.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum VadState {
        /// Waiting for audio energy above threshold.
        Listening,
        /// Wake word energy detected, capturing utterance.
        Capturing,
    }

    /// Voice-activated channel using energy-based VAD and wake word detection.
    ///
    /// Architecture:
    /// 1. Monitor audio input energy levels (simulated without cpal)
    /// 2. When energy exceeds threshold, capture audio buffer
    /// 3. Send captured audio to Whisper API for transcription
    /// 4. Check transcript for wake words
    /// 5. If wake word found, emit ChannelMessage with the full transcript
    pub struct VoiceWakeChannel {
        wake_words: Vec<String>,
        energy_threshold: f32,
        transcription_url: Option<String>,
        capture_timeout: Duration,
    }

    impl VoiceWakeChannel {
        pub fn new(wake_words: Vec<String>, energy_threshold: f32) -> Self {
            Self {
                wake_words,
                energy_threshold,
                transcription_url: None,
                capture_timeout: Duration::from_secs(10),
            }
        }

        pub fn with_transcription_url(mut self, url: String) -> Self {
            self.transcription_url = Some(url);
            self
        }

        pub fn with_capture_timeout(mut self, timeout: Duration) -> Self {
            self.capture_timeout = timeout;
            self
        }

        /// Check if a transcript contains any of the configured wake words.
        fn matches_wake_word(&self, transcript: &str) -> bool {
            let lower = transcript.to_lowercase();
            self.wake_words
                .iter()
                .any(|w| lower.contains(&w.to_lowercase()))
        }

        /// Compute RMS energy of a float audio buffer.
        fn compute_energy(samples: &[f32]) -> f32 {
            if samples.is_empty() {
                return 0.0;
            }
            let sum: f32 = samples.iter().map(|s| s * s).sum();
            (sum / samples.len() as f32).sqrt()
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
            tracing::info!(
                wake_words = ?self.wake_words,
                energy_threshold = self.energy_threshold,
                "voice-wake channel started (requires cpal for audio capture)"
            );

            // Without cpal, we can't capture audio. Log and wait.
            // When cpal is added as a dependency, this method will:
            // 1. Open default audio input device via cpal
            // 2. Read samples in a loop, compute energy via compute_energy()
            // 3. On energy > threshold: transition to Capturing state
            // 4. Buffer captured samples until silence or capture_timeout
            // 5. Encode buffer as WAV, POST to transcription_url
            // 6. Check transcript via matches_wake_word()
            // 7. If matched: send ChannelMessage via tx
            // 8. Return to Listening state
            tracing::warn!(
                "voice-wake: no cpal audio backend available — \
                 channel is dormant until cpal dependency is added"
            );
            tokio::time::sleep(Duration::from_secs(u64::MAX)).await;
            Ok(())
        }

        async fn health_check(&self) -> bool {
            // Healthy if wake words are configured
            !self.wake_words.is_empty()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn matches_wake_word_case_insensitive() {
            let ch = VoiceWakeChannel::new(vec!["hey agent".to_string()], 0.05);
            assert!(ch.matches_wake_word("Hey Agent, what's the weather?"));
            assert!(ch.matches_wake_word("HEY AGENT do something"));
            assert!(!ch.matches_wake_word("hello world"));
        }

        #[test]
        fn matches_wake_word_multiple_words() {
            let ch = VoiceWakeChannel::new(
                vec!["jarvis".to_string(), "computer".to_string()],
                0.05,
            );
            assert!(ch.matches_wake_word("jarvis, set a timer"));
            assert!(ch.matches_wake_word("ok computer, play music"));
            assert!(!ch.matches_wake_word("hello there"));
        }

        #[test]
        fn compute_energy_returns_rms() {
            let samples = vec![0.5, -0.5, 0.5, -0.5];
            let energy = VoiceWakeChannel::compute_energy(&samples);
            assert!((energy - 0.5).abs() < 0.01);
        }

        #[test]
        fn compute_energy_empty_returns_zero() {
            assert_eq!(VoiceWakeChannel::compute_energy(&[]), 0.0);
        }

        #[test]
        fn compute_energy_silence_returns_zero() {
            let samples = vec![0.0; 100];
            assert_eq!(VoiceWakeChannel::compute_energy(&samples), 0.0);
        }

        #[tokio::test]
        async fn health_check_true_with_wake_words() {
            let ch = VoiceWakeChannel::new(vec!["hey".to_string()], 0.05);
            assert!(ch.health_check().await);
        }

        #[tokio::test]
        async fn health_check_false_without_wake_words() {
            let ch = VoiceWakeChannel::new(vec![], 0.05);
            assert!(!ch.health_check().await);
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

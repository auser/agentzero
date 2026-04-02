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
    /// Pipeline:
    /// 1. Monitor audio input energy levels via cpal
    /// 2. When energy exceeds threshold, transition to Capturing
    /// 3. Buffer captured samples until silence or capture_timeout
    /// 4. Encode buffer as WAV via hound, POST to Whisper-compatible API
    /// 5. Check transcript for wake words
    /// 6. If wake word found, emit ChannelMessage with the full transcript
    pub struct VoiceWakeChannel {
        wake_words: Vec<String>,
        energy_threshold: f32,
        transcription_url: Option<String>,
        transcription_api_key: Option<String>,
        capture_timeout: Duration,
        /// Sample rate for audio capture (default: 16000 Hz).
        sample_rate: u32,
    }

    impl VoiceWakeChannel {
        pub fn new(wake_words: Vec<String>, energy_threshold: f32) -> Self {
            Self {
                wake_words,
                energy_threshold,
                transcription_url: None,
                transcription_api_key: None,
                capture_timeout: Duration::from_secs(10),
                sample_rate: 16000,
            }
        }

        pub fn with_transcription_url(mut self, url: String) -> Self {
            self.transcription_url = Some(url);
            self
        }

        pub fn with_transcription_api_key(mut self, key: String) -> Self {
            self.transcription_api_key = Some(key);
            self
        }

        pub fn with_capture_timeout(mut self, timeout: Duration) -> Self {
            self.capture_timeout = timeout;
            self
        }

        pub fn with_sample_rate(mut self, rate: u32) -> Self {
            self.sample_rate = rate;
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

        /// Encode f32 samples as a WAV byte buffer using hound.
        fn encode_wav(samples: &[f32], sample_rate: u32) -> anyhow::Result<Vec<u8>> {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let mut cursor = std::io::Cursor::new(Vec::new());
            {
                let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
                for &sample in samples {
                    let clamped = sample.clamp(-1.0, 1.0);
                    let int_sample = (clamped * i16::MAX as f32) as i16;
                    writer.write_sample(int_sample)?;
                }
                writer.finalize()?;
            }
            Ok(cursor.into_inner())
        }

        /// POST a WAV buffer to the Whisper-compatible transcription API.
        async fn transcribe(&self, wav_data: Vec<u8>) -> anyhow::Result<String> {
            let url = self
                .transcription_url
                .as_deref()
                .unwrap_or("https://api.groq.com/openai/v1/audio/transcriptions");

            let file_part = reqwest::multipart::Part::bytes(wav_data)
                .file_name("capture.wav")
                .mime_str("audio/wav")?;
            let form = reqwest::multipart::Form::new()
                .text("model", "whisper-large-v3")
                .text("response_format", "text")
                .part("file", file_part);

            let mut req = reqwest::Client::new().post(url).multipart(form);
            if let Some(ref key) = self.transcription_api_key {
                req = req.header("Authorization", format!("Bearer {key}"));
            }

            let resp = req.send().await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("transcription API returned {status}: {body}");
            }
            Ok(resp.text().await?.trim().to_string())
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
            tx: tokio::sync::mpsc::Sender<ChannelMessage>,
        ) -> anyhow::Result<()> {
            tracing::info!(
                wake_words = ?self.wake_words,
                energy_threshold = self.energy_threshold,
                sample_rate = self.sample_rate,
                "voice-wake channel starting audio capture"
            );

            // cpal::Stream is !Send, so we run the audio capture in a
            // blocking thread and shuttle samples over an mpsc channel.
            let (audio_tx, mut audio_rx) = tokio::sync::mpsc::channel::<Vec<f32>>(64);
            let sample_rate = self.sample_rate;

            // Spawn the cpal stream on a blocking thread (it stays alive until dropped).
            let _stream_handle = std::thread::spawn(move || -> anyhow::Result<()> {
                use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

                let host = cpal::default_host();
                let device = host
                    .default_input_device()
                    .ok_or_else(|| anyhow::anyhow!("no default audio input device found"))?;

                tracing::info!(
                    device = device.name().unwrap_or_else(|_| "unknown".into()),
                    "using audio input device"
                );

                let config = cpal::StreamConfig {
                    channels: 1,
                    sample_rate: cpal::SampleRate(sample_rate),
                    buffer_size: cpal::BufferSize::Default,
                };

                let stream = device.build_input_stream(
                    &config,
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        let _ = audio_tx.try_send(data.to_vec());
                    },
                    |err| {
                        tracing::error!(error = %err, "audio input stream error");
                    },
                    None,
                )?;

                stream.play()?;
                tracing::info!("audio capture stream started");

                // Keep the thread alive (and the stream playing) forever.
                // The stream is dropped when the thread exits.
                loop {
                    std::thread::park();
                }
            });

            let mut state = VadState::Listening;
            let mut capture_buffer: Vec<f32> = Vec::new();
            let mut capture_start = std::time::Instant::now();
            let silence_threshold_chunks = 10u32; // ~0.6s at 16kHz/1024-sample chunks
            let mut silence_count = 0u32;

            loop {
                match audio_rx.recv().await {
                    Some(samples) => {
                        let energy = Self::compute_energy(&samples);

                        match state {
                            VadState::Listening => {
                                if energy > self.energy_threshold {
                                    tracing::debug!(energy, "voice activity detected, capturing");
                                    state = VadState::Capturing;
                                    capture_buffer.clear();
                                    capture_buffer.extend_from_slice(&samples);
                                    capture_start = std::time::Instant::now();
                                    silence_count = 0;
                                }
                            }
                            VadState::Capturing => {
                                capture_buffer.extend_from_slice(&samples);

                                if energy < self.energy_threshold * 0.5 {
                                    silence_count += 1;
                                } else {
                                    silence_count = 0;
                                }

                                let timed_out = capture_start.elapsed() >= self.capture_timeout;
                                let silence_detected = silence_count >= silence_threshold_chunks;

                                if timed_out || silence_detected {
                                    tracing::debug!(
                                        samples = capture_buffer.len(),
                                        timed_out,
                                        silence_detected,
                                        "capture complete, transcribing"
                                    );

                                    match Self::encode_wav(&capture_buffer, self.sample_rate) {
                                        Ok(wav_data) => match self.transcribe(wav_data).await {
                                            Ok(transcript) if !transcript.is_empty() => {
                                                tracing::info!(transcript = %transcript, "transcription received");
                                                if self.matches_wake_word(&transcript) {
                                                    tracing::info!("wake word matched!");
                                                    let now_ms = std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .unwrap_or_default()
                                                        .as_millis() as u64;
                                                    let msg = ChannelMessage {
                                                        id: format!("voice-{now_ms}"),
                                                        sender: "voice".to_string(),
                                                        reply_target: String::new(),
                                                        content: transcript,
                                                        channel: "voice-wake".to_string(),
                                                        timestamp: now_ms,
                                                        thread_ts: None,
                                                        privacy_boundary: String::new(),
                                                        attachments: vec![],
                                                    };
                                                    if tx.send(msg).await.is_err() {
                                                        tracing::warn!("voice-wake: channel receiver dropped");
                                                        return Ok(());
                                                    }
                                                }
                                            }
                                            Ok(_) => tracing::debug!("empty transcription, ignoring"),
                                            Err(e) => tracing::warn!(error = %e, "transcription failed"),
                                        },
                                        Err(e) => tracing::warn!(error = %e, "WAV encoding failed"),
                                    }

                                    state = VadState::Listening;
                                    capture_buffer.clear();
                                    silence_count = 0;
                                }
                            }
                        }
                    }
                    None => {
                        tracing::info!("audio stream closed");
                        break;
                    }
                }
            }

            Ok(())
        }

        async fn health_check(&self) -> bool {
            // Healthy if wake words are configured and we can find an audio device.
            if self.wake_words.is_empty() {
                return false;
            }
            // Check for audio device availability.
            use cpal::traits::HostTrait;
            cpal::default_host().default_input_device().is_some()
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

        #[test]
        fn encode_wav_produces_valid_output() {
            let samples = vec![0.5, -0.3, 0.1, 0.0, -0.8];
            let wav = VoiceWakeChannel::encode_wav(&samples, 16000).expect("encode");
            assert!(wav.len() > 44, "WAV should have header + data"); // 44-byte WAV header minimum
            assert_eq!(&wav[..4], b"RIFF", "should start with RIFF header");
        }

        #[test]
        fn encode_wav_empty_samples() {
            let wav = VoiceWakeChannel::encode_wav(&[], 16000).expect("encode");
            assert_eq!(&wav[..4], b"RIFF");
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

//! Shared generation infrastructure for local LLM providers.
//!
//! The [`LocalLlm`] trait abstracts over different inference backends (Candle,
//! llama.cpp) at the token level, while [`GenerationLoop`] handles the
//! repetition detection, streaming, and token counting that every local
//! provider needs.

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::warn;

use agentzero_core::types::StreamChunk;

// ---------------------------------------------------------------------------
// LocalLlm trait
// ---------------------------------------------------------------------------

/// Token-level abstraction for a loaded local LLM.
///
/// Implementations hold the model weights, sampling state, and any
/// backend-specific context needed for one generation run. Instances are
/// typically short-lived — created after acquiring a lock on the provider's
/// inner state and dropped when generation completes.
///
/// Adding a new backend (ONNX, MLX, mistral.rs, …) only requires
/// implementing these five methods — the [`GenerationLoop`] handles
/// everything else.
pub trait LocalLlm {
    /// Tokenize a prompt string into token IDs.
    fn tokenize(&self, text: &str) -> Result<Vec<u32>>;

    /// Feed the full prompt and return the first sampled output token.
    ///
    /// The implementation should process all `tokens` through the model in
    /// one batch, then sample and return the first generated token.
    fn feed_prompt(&mut self, tokens: &[u32]) -> Result<u32>;

    /// Feed one previously-sampled token at position `pos` and return the next.
    ///
    /// `pos` starts at `prompt_tokens.len()` (right after the prompt) and
    /// increments by one on each call.
    fn step(&mut self, token: u32, pos: usize) -> Result<u32>;

    /// Decode a single token ID to its string representation.
    fn decode_token(&self, token: u32) -> Result<String>;

    /// Returns `true` if `token` is an end-of-sequence marker.
    fn is_eos(&self, token: u32) -> bool;
}

// ---------------------------------------------------------------------------
// GenerationLoop
// ---------------------------------------------------------------------------

/// Drives token-by-token generation over any [`LocalLlm`].
///
/// Handles repetition detection, optional streaming, and token counting
/// so that each backend only implements the five `LocalLlm` methods.
pub struct GenerationLoop {
    /// Maximum number of tokens to generate before stopping.
    pub max_tokens: u32,
    /// Number of trailing characters used for repetition detection.
    /// When the last `repeat_window` characters appear earlier in the output,
    /// generation stops early.
    pub repeat_window: usize,
}

impl Default for GenerationLoop {
    fn default() -> Self {
        Self {
            max_tokens: 2048,
            repeat_window: 80,
        }
    }
}

impl GenerationLoop {
    /// Run generation to completion, optionally streaming tokens.
    ///
    /// Returns `(output_text, input_token_count, output_token_count)`.
    pub fn run<L: LocalLlm>(
        &self,
        llm: &mut L,
        prompt_tokens: &[u32],
        sender: Option<&mpsc::UnboundedSender<StreamChunk>>,
    ) -> Result<(String, u64, u64)> {
        let input_tokens = prompt_tokens.len() as u64;
        let mut next_token = llm.feed_prompt(prompt_tokens)?;
        let mut output = String::new();
        let mut output_tokens = 0u64;
        let mut pos = prompt_tokens.len();

        for _ in 0..self.max_tokens {
            if llm.is_eos(next_token) {
                break;
            }

            let text = llm.decode_token(next_token)?;

            if let Some(tx) = sender {
                if !text.is_empty() {
                    let _ = tx.send(StreamChunk {
                        delta: text.clone(),
                        done: false,
                        tool_call_delta: None,
                    });
                }
            }

            output.push_str(&text);
            output_tokens += 1;

            // Repetition detection: if the last `repeat_window` chars appear
            // earlier in the output, the model is looping.
            if output.len() > self.repeat_window * 2 {
                let tail = &output[output.len() - self.repeat_window..];
                let before_tail = &output[..output.len() - self.repeat_window];
                if before_tail.contains(tail) {
                    warn!(
                        tokens = output_tokens,
                        "repetition detected, stopping early"
                    );
                    if let Some(rpos) = before_tail.rfind(tail) {
                        output.truncate(rpos + self.repeat_window);
                    }
                    break;
                }
            }

            next_token = llm.step(next_token, pos)?;
            pos += 1;
        }

        if let Some(tx) = sender {
            let _ = tx.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });
        }

        Ok((output, input_tokens, output_tokens))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic mock LLM that yields a fixed sequence of tokens.
    struct MockLlm {
        /// Tokens to yield (after the prompt is consumed).
        tokens: Vec<u32>,
        /// Current position in `tokens`.
        cursor: usize,
        /// The end-of-sequence sentinel.
        eos: u32,
    }

    impl LocalLlm for MockLlm {
        fn tokenize(&self, text: &str) -> Result<Vec<u32>> {
            // One token per whitespace-separated word, starting at 100.
            Ok(text
                .split_whitespace()
                .enumerate()
                .map(|(i, _)| 100 + i as u32)
                .collect())
        }

        fn feed_prompt(&mut self, _tokens: &[u32]) -> Result<u32> {
            self.cursor = 0;
            let t = self.tokens.get(self.cursor).copied().unwrap_or(self.eos);
            self.cursor += 1;
            Ok(t)
        }

        fn step(&mut self, _token: u32, _pos: usize) -> Result<u32> {
            let t = self.tokens.get(self.cursor).copied().unwrap_or(self.eos);
            self.cursor += 1;
            Ok(t)
        }

        fn decode_token(&self, token: u32) -> Result<String> {
            if token == self.eos {
                Ok(String::new())
            } else {
                Ok(format!("t{token}"))
            }
        }

        fn is_eos(&self, token: u32) -> bool {
            token == self.eos
        }
    }

    #[test]
    fn generation_loop_basic() {
        let mut llm = MockLlm {
            tokens: vec![1, 2, 3],
            cursor: 0,
            eos: 99,
        };
        let gen = GenerationLoop {
            max_tokens: 100,
            repeat_window: 80,
        };
        let (output, input, output_count) = gen.run(&mut llm, &[10, 20], None).expect("run");
        assert_eq!(input, 2);
        assert_eq!(output_count, 3);
        assert_eq!(output, "t1t2t3");
    }

    #[test]
    fn generation_loop_stops_at_eos() {
        let mut llm = MockLlm {
            tokens: vec![1, 99, 3], // 99 is EOS
            cursor: 0,
            eos: 99,
        };
        let gen = GenerationLoop {
            max_tokens: 100,
            repeat_window: 80,
        };
        let (output, _, output_count) = gen.run(&mut llm, &[10], None).expect("run");
        assert_eq!(output_count, 1);
        assert_eq!(output, "t1");
    }

    #[test]
    fn generation_loop_respects_max_tokens() {
        let mut llm = MockLlm {
            tokens: (1..=100).collect(),
            cursor: 0,
            eos: 999,
        };
        let gen = GenerationLoop {
            max_tokens: 5,
            repeat_window: 80,
        };
        let (_, _, output_count) = gen.run(&mut llm, &[10], None).expect("run");
        assert_eq!(output_count, 5);
    }

    #[test]
    fn generation_loop_streams_tokens() {
        let mut llm = MockLlm {
            tokens: vec![1, 2, 3],
            cursor: 0,
            eos: 99,
        };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let gen = GenerationLoop {
            max_tokens: 100,
            repeat_window: 80,
        };
        gen.run(&mut llm, &[10], Some(&tx)).expect("run");
        drop(tx);

        let mut chunks = vec![];
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        // 3 content chunks + 1 done chunk
        assert_eq!(chunks.len(), 4);
        assert!(!chunks[0].done);
        assert_eq!(chunks[0].delta, "t1");
        assert!(!chunks[1].done);
        assert_eq!(chunks[1].delta, "t2");
        assert!(!chunks[2].done);
        assert_eq!(chunks[2].delta, "t3");
        assert!(chunks[3].done);
        assert!(chunks[3].delta.is_empty());
    }

    #[test]
    fn generation_loop_detects_repetition() {
        // Build a sequence that repeats: tokens 1..=5 produce "t1t2t3t4t5"
        // (15 chars). We set repeat_window=15, so after two full copies the
        // loop should detect the repeat and stop.
        let repeating: Vec<u32> = (1..=5).cycle().take(100).collect();
        let mut llm = MockLlm {
            tokens: repeating,
            cursor: 0,
            eos: 999,
        };
        let gen = GenerationLoop {
            max_tokens: 200,
            repeat_window: 15,
        };
        let (_, _, output_count) = gen.run(&mut llm, &[10], None).expect("run");
        // Should have stopped well before 200 tokens due to repetition.
        assert!(
            output_count < 50,
            "expected early stop from repetition, got {output_count} tokens"
        );
    }
}

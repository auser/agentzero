//! Local LLM provider using Candle (Hugging Face's pure Rust ML framework).
//!
//! Runs inference entirely in-process via Candle with GGUF model loading.
//! Supports Metal (Apple Silicon), CUDA, and CPU backends.
//!
//! Tool use is supported via the same ChatML `<tool_call>` format as the
//! builtin provider — see [`crate::local_tools`] for shared parsing logic.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use async_trait::async_trait;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_llama::ModelWeights;
use tokenizers::Tokenizer;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use agentzero_core::types::{
    ChatResult, ConversationMessage, ReasoningConfig, StopReason, StreamChunk, ToolDefinition,
};
use agentzero_core::Provider;

use crate::local_tools;
use crate::model_manager;

/// Configuration for the Candle provider, populated from `LocalModelConfig`.
#[derive(Debug, Clone)]
pub struct CandleConfig {
    pub model: String,
    pub filename: String,
    pub n_ctx: u32,
    pub temperature: f64,
    pub top_p: f64,
    pub max_output_tokens: u32,
    pub seed: u64,
    pub repeat_penalty: f32,
    pub device: String,
}

impl Default for CandleConfig {
    fn default() -> Self {
        Self {
            model: model_manager::DEFAULT_HF_REPO.to_string(),
            filename: model_manager::DEFAULT_GGUF_FILE.to_string(),
            n_ctx: 8192,
            temperature: 0.7,
            top_p: 0.9,
            max_output_tokens: 2048,
            seed: 42,
            repeat_penalty: 1.1,
            device: "auto".to_string(),
        }
    }
}

/// A provider that runs inference locally via Candle.
pub struct CandleProvider {
    config: CandleConfig,
    /// Lazily initialized model + tokenizer.
    inner: Mutex<Option<LoadedModel>>,
}

struct LoadedModel {
    weights: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
}

// Safety: LoadedModel is only accessed behind a Mutex and within spawn_blocking.
// Candle tensors are not Send, so we ensure all access stays on one thread.
unsafe impl Send for CandleProvider {}
unsafe impl Sync for CandleProvider {}

impl CandleProvider {
    /// Create a new Candle provider with the given configuration.
    pub fn new(config: CandleConfig) -> Self {
        Self {
            config,
            inner: Mutex::new(None),
        }
    }

    /// Select the appropriate Candle device based on config.
    ///
    /// Currently only CPU is supported. Metal and CUDA support will be added
    /// when candle-metal-kernels stabilises on crates.io (currently alpha).
    fn select_device(preference: &str) -> Result<Device> {
        match preference {
            "metal" | "cuda" => {
                warn!(
                    device = preference,
                    "GPU acceleration not yet available for Candle provider, falling back to CPU"
                );
                Ok(Device::Cpu)
            }
            _ => {
                info!("using CPU for Candle inference");
                Ok(Device::Cpu)
            }
        }
    }

    /// Ensure the model is downloaded and loaded.
    fn ensure_loaded(&self) -> Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if guard.is_some() {
            return Ok(());
        }

        let model_path = self.resolve_model_path()?;

        eprintln!(
            "\x1b[1;36m⟐ Loading Candle model:\x1b[0m {}",
            model_path.display()
        );
        info!(model = %model_path.display(), "loading Candle GGUF model");

        let device = Self::select_device(&self.config.device)?;

        // Load GGUF model
        let mut file = std::fs::File::open(&model_path)
            .with_context(|| format!("failed to open model file: {}", model_path.display()))?;

        let content = candle_core::quantized::gguf_file::Content::read(&mut file)
            .context("failed to read GGUF file")?;

        let weights = ModelWeights::from_gguf(content, &mut file, &device)
            .context("failed to load model weights from GGUF")?;

        // Load tokenizer from HuggingFace Hub
        let tokenizer = self.load_tokenizer()?;

        eprintln!("\x1b[1;32m✓ Candle model loaded\x1b[0m ({device:?})");
        info!("Candle model loaded successfully");

        *guard = Some(LoadedModel {
            weights,
            tokenizer,
            device,
        });
        Ok(())
    }

    /// Resolve the model path — download from HF Hub if needed.
    fn resolve_model_path(&self) -> Result<PathBuf> {
        // If it's a direct path to a .gguf file, use it
        if self.config.model.ends_with(".gguf") && std::path::Path::new(&self.config.model).exists()
        {
            return Ok(PathBuf::from(&self.config.model));
        }

        // Otherwise download from HF Hub
        model_manager::ensure_model(&self.config.model, &self.config.filename)
    }

    /// Load the tokenizer for the model from HuggingFace Hub.
    fn load_tokenizer(&self) -> Result<Tokenizer> {
        let api = hf_hub::api::sync::ApiBuilder::new()
            .with_progress(false)
            .build()
            .context("failed to create HuggingFace API client")?;

        // For GGUF repos, the tokenizer is usually in the base model repo.
        // Try the configured repo first, then strip "-GGUF" suffix.
        let repo_name = self.config.model.replace("-GGUF", "");
        let repo = api.model(repo_name.clone());

        let tokenizer_path = repo
            .get("tokenizer.json")
            .with_context(|| format!("failed to download tokenizer.json from {repo_name}"))?;

        Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("failed to load tokenizer: {e}"))
    }

    /// Run inference and return generated text + token counts.
    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<(String, u64, u64)> {
        self.ensure_loaded()?;

        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let loaded = guard
            .as_mut()
            .context("model not loaded after ensure_loaded")?;

        let encoding = loaded
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;
        let tokens = encoding.get_ids();
        let input_tokens = tokens.len() as u64;

        // Guard: prompt must fit in context window
        let max_input = self.config.n_ctx.saturating_sub(256) as usize;
        if tokens.len() > max_input {
            anyhow::bail!(
                "prompt too large for context window: {} tokens exceeds limit of {} \
                 (n_ctx={}). Try reducing tool count or prompt length.",
                tokens.len(),
                max_input,
                self.config.n_ctx,
            );
        }

        let eos_token = self.resolve_eos_token(&loaded.tokenizer);

        let mut logits_processor = LogitsProcessor::from_sampling(
            self.config.seed,
            candle_transformers::generation::Sampling::TopKThenTopP {
                k: 40,
                p: self.config.top_p,
                temperature: self.config.temperature,
            },
        );

        // Feed prompt tokens
        let input_tensor = Tensor::new(tokens, &loaded.device)?.unsqueeze(0)?;
        let logits = loaded.weights.forward(&input_tensor, 0)?;
        let logits = logits.squeeze(0)?;
        let logits = logits.get(logits.dim(0)? - 1)?;
        let mut next_token = logits_processor.sample(&logits)?;

        let mut output = String::new();
        let mut output_tokens = 0u64;
        let mut pos = tokens.len();

        const REPEAT_WINDOW: usize = 80;

        for _ in 0..max_tokens {
            // Check EOS
            if Some(next_token) == eos_token {
                break;
            }

            // Decode token
            let text = loaded
                .tokenizer
                .decode(&[next_token], true)
                .map_err(|e| anyhow::anyhow!("token decode failed: {e}"))?;
            output.push_str(&text);
            output_tokens += 1;

            // Repetition detection
            if output.len() > REPEAT_WINDOW * 2 {
                let tail = &output[output.len() - REPEAT_WINDOW..];
                let before_tail = &output[..output.len() - REPEAT_WINDOW];
                if before_tail.contains(tail) {
                    warn!(
                        tokens = output_tokens,
                        "repetition detected, stopping early"
                    );
                    if let Some(pos) = before_tail.rfind(tail) {
                        output.truncate(pos + REPEAT_WINDOW);
                    }
                    break;
                }
            }

            // Next token
            let input = Tensor::new(&[next_token], &loaded.device)?.unsqueeze(0)?;
            let logits = loaded.weights.forward(&input, pos)?;
            let logits = logits.squeeze(0)?;
            let logits = logits.get(logits.dim(0)? - 1)?;
            next_token = logits_processor.sample(&logits)?;
            pos += 1;
        }

        Ok((output, input_tokens, output_tokens))
    }

    /// Run inference with streaming — sends tokens via mpsc channel as they're generated.
    fn generate_streaming(
        &self,
        prompt: &str,
        max_tokens: u32,
        sender: &mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<(String, u64, u64)> {
        self.ensure_loaded()?;

        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let loaded = guard
            .as_mut()
            .context("model not loaded after ensure_loaded")?;

        let encoding = loaded
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;
        let tokens = encoding.get_ids();
        let input_tokens = tokens.len() as u64;

        let max_input = self.config.n_ctx.saturating_sub(256) as usize;
        if tokens.len() > max_input {
            anyhow::bail!(
                "prompt too large for context window: {} tokens exceeds limit of {} \
                 (n_ctx={}). Try reducing tool count or prompt length.",
                tokens.len(),
                max_input,
                self.config.n_ctx,
            );
        }

        let eos_token = self.resolve_eos_token(&loaded.tokenizer);

        let mut logits_processor = LogitsProcessor::from_sampling(
            self.config.seed,
            candle_transformers::generation::Sampling::TopKThenTopP {
                k: 40,
                p: self.config.top_p,
                temperature: self.config.temperature,
            },
        );

        // Feed prompt
        let input_tensor = Tensor::new(tokens, &loaded.device)?.unsqueeze(0)?;
        let logits = loaded.weights.forward(&input_tensor, 0)?;
        let logits = logits.squeeze(0)?;
        let logits = logits.get(logits.dim(0)? - 1)?;
        let mut next_token = logits_processor.sample(&logits)?;

        let mut output = String::new();
        let mut output_tokens = 0u64;
        let mut pos = tokens.len();

        const REPEAT_WINDOW: usize = 80;

        for _ in 0..max_tokens {
            if Some(next_token) == eos_token {
                break;
            }

            let text = loaded
                .tokenizer
                .decode(&[next_token], true)
                .map_err(|e| anyhow::anyhow!("token decode failed: {e}"))?;

            if !text.is_empty() {
                // Stream the token
                let _ = sender.send(StreamChunk {
                    delta: text.clone(),
                    done: false,
                    tool_call_delta: None,
                });
            }

            output.push_str(&text);
            output_tokens += 1;

            // Repetition detection
            if output.len() > REPEAT_WINDOW * 2 {
                let tail = &output[output.len() - REPEAT_WINDOW..];
                let before_tail = &output[..output.len() - REPEAT_WINDOW];
                if before_tail.contains(tail) {
                    warn!(
                        tokens = output_tokens,
                        "repetition detected, stopping early"
                    );
                    if let Some(rpos) = before_tail.rfind(tail) {
                        output.truncate(rpos + REPEAT_WINDOW);
                    }
                    break;
                }
            }

            let input = Tensor::new(&[next_token], &loaded.device)?.unsqueeze(0)?;
            let logits = loaded.weights.forward(&input, pos)?;
            let logits = logits.squeeze(0)?;
            let logits = logits.get(logits.dim(0)? - 1)?;
            next_token = logits_processor.sample(&logits)?;
            pos += 1;
        }

        // Send final done chunk
        let _ = sender.send(StreamChunk {
            delta: String::new(),
            done: true,
            tool_call_delta: None,
        });

        Ok((output, input_tokens, output_tokens))
    }

    /// Resolve the EOS token ID for the loaded model.
    fn resolve_eos_token(&self, tokenizer: &Tokenizer) -> Option<u32> {
        // Try common EOS tokens
        for candidate in &["<|im_end|>", "<|endoftext|>", "</s>", "<eos>"] {
            if let Some(id) = tokenizer.token_to_id(candidate) {
                return Some(id);
            }
        }
        None
    }
}

#[async_trait]
impl Provider for CandleProvider {
    fn supports_streaming(&self) -> bool {
        true
    }

    fn estimate_tokens(&self, text: &str) -> Option<usize> {
        let guard = self.inner.lock().ok()?;
        let loaded = guard.as_ref()?;
        let encoding = loaded.tokenizer.encode(text, false).ok()?;
        Some(encoding.get_ids().len())
    }

    async fn complete(&self, prompt: &str) -> Result<ChatResult> {
        let formatted = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");

        let (output_text, input_tokens, output_tokens) = tokio::task::block_in_place(|| {
            self.generate(&formatted, self.config.max_output_tokens)
        })?;

        Ok(ChatResult {
            output_text,
            tool_calls: vec![],
            stop_reason: Some(StopReason::EndTurn),
            input_tokens,
            output_tokens,
        })
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<ChatResult> {
        let formatted = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");

        let (output_text, input_tokens, output_tokens) = tokio::task::block_in_place(|| {
            self.generate_streaming(&formatted, self.config.max_output_tokens, &sender)
        })?;

        Ok(ChatResult {
            output_text,
            tool_calls: vec![],
            stop_reason: Some(StopReason::EndTurn),
            input_tokens,
            output_tokens,
        })
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
    ) -> Result<ChatResult> {
        let prompt = local_tools::format_chatml_prompt(messages, tools);

        let (raw_output, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.generate(&prompt, self.config.max_output_tokens))?;

        let (text, tool_calls) = local_tools::parse_tool_calls(&raw_output);

        let stop_reason = if tool_calls.is_empty() {
            StopReason::EndTurn
        } else {
            StopReason::ToolUse
        };

        debug!(
            tool_count = tool_calls.len(),
            text_len = text.len(),
            "parsed candle response"
        );

        Ok(ChatResult {
            output_text: text,
            tool_calls,
            stop_reason: Some(stop_reason),
            input_tokens,
            output_tokens,
        })
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
        sender: mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<ChatResult> {
        let prompt = local_tools::format_chatml_prompt(messages, tools);

        let (raw_output, input_tokens, output_tokens) = tokio::task::block_in_place(|| {
            self.generate_streaming(&prompt, self.config.max_output_tokens, &sender)
        })?;

        // Parse tool calls from accumulated output
        let (text, tool_calls) = local_tools::parse_tool_calls(&raw_output);

        let stop_reason = if tool_calls.is_empty() {
            StopReason::EndTurn
        } else {
            StopReason::ToolUse
        };

        debug!(
            tool_count = tool_calls.len(),
            text_len = text.len(),
            "parsed candle streaming response"
        );

        Ok(ChatResult {
            output_text: text,
            tool_calls,
            stop_reason: Some(stop_reason),
            input_tokens,
            output_tokens,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let config = CandleConfig::default();
        assert!(config.n_ctx >= 2048);
        assert!(config.temperature >= 0.0 && config.temperature <= 2.0);
        assert!(config.top_p > 0.0 && config.top_p <= 1.0);
        assert!(config.max_output_tokens > 0);
        assert!(!config.model.is_empty());
        assert!(config.filename.ends_with(".gguf"));
    }

    #[test]
    fn select_device_cpu() {
        let device = CandleProvider::select_device("cpu").expect("cpu should always work");
        assert!(matches!(device, Device::Cpu));
    }

    #[test]
    fn select_device_auto_falls_back_to_cpu() {
        // On CI/test machines without GPU, auto should fall back to CPU
        let device = CandleProvider::select_device("auto").expect("auto should work");
        // We can't assert which device, but it should not error
        let _ = device;
    }
}

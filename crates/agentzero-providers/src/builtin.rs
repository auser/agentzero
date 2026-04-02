//! Built-in local LLM provider using llama.cpp.
//!
//! Runs inference entirely in-process — no external server needed.
//! Uses `llama-cpp-2` for GGUF model loading and text generation.
//!
//! Tool use is supported via Qwen's `<tool_call>` / `<tools>` prompt format:
//! tool definitions are injected into the system prompt, and `<tool_call>` XML
//! blocks in the model output are parsed into `ToolUseRequest` objects.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use async_trait::async_trait;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::sampling::LlamaSampler;
use tracing::{debug, info, warn};

use agentzero_core::types::{
    ChatResult, ConversationMessage, ReasoningConfig, StopReason, StreamChunk, ToolDefinition,
};
use agentzero_core::Provider;

use crate::model_manager;

/// Process-global llama.cpp backend singleton.
///
/// `LlamaBackend::init()` can only be called once per process (enforced by an
/// `AtomicBool` in the llama-cpp-2 crate). Storing it in a `OnceLock` ensures
/// exactly-once initialization and prevents `llama_backend_free()` from being
/// called while any provider is still running.
static SHARED_BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

fn shared_backend() -> &'static LlamaBackend {
    SHARED_BACKEND.get_or_init(|| LlamaBackend::init().expect("failed to init llama.cpp backend"))
}

/// A provider that runs inference locally via llama.cpp.
pub struct BuiltinProvider {
    model_path: PathBuf,
    model_name: String,
    /// Lazily initialized backend + model.
    /// Behind a Mutex because llama.cpp contexts aren't Send.
    inner: Mutex<Option<LoadedModel>>,
    n_ctx: u32,
    n_gpu_layers: u32,
}

struct LoadedModel {
    model: LlamaModel,
}

impl BuiltinProvider {
    /// Create a new builtin provider.
    ///
    /// `model` can be:
    /// - `"default"` or empty — uses the default bundled model
    /// - A path to a `.gguf` file
    /// - A `repo/filename` HuggingFace reference
    pub fn new(model: String) -> Self {
        Self {
            model_path: PathBuf::new(),
            model_name: if model.is_empty() {
                model_manager::DEFAULT_BUILTIN_MODEL.to_string()
            } else {
                model.clone()
            },
            inner: Mutex::new(None),
            n_ctx: 8192,       // tool definitions can easily consume 2-3k tokens
            n_gpu_layers: 999, // offload all layers to GPU when available
        }
    }

    /// Ensure the model is downloaded and loaded.
    fn ensure_loaded(&self) -> Result<()> {
        let mut guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        if guard.is_some() {
            return Ok(());
        }

        // Resolve model path
        let model_path = if !self.model_path.as_os_str().is_empty() {
            self.model_path.clone()
        } else if self.model_name == model_manager::DEFAULT_BUILTIN_MODEL
            || self.model_name == "default"
        {
            model_manager::ensure_default_model()?
        } else if self.model_name.ends_with(".gguf")
            && std::path::Path::new(&self.model_name).exists()
        {
            PathBuf::from(&self.model_name)
        } else {
            // Try as HF repo/file: "org/repo/file.gguf"
            let parts: Vec<&str> = self.model_name.splitn(3, '/').collect();
            if parts.len() == 3 && parts[2].ends_with(".gguf") {
                let repo = format!("{}/{}", parts[0], parts[1]);
                model_manager::ensure_model(&repo, parts[2])?
            } else {
                model_manager::ensure_default_model()?
            }
        };

        eprintln!(
            "\x1b[1;36m⟐ Loading builtin model:\x1b[0m {}",
            model_path.display()
        );
        info!(model = %model_path.display(), "loading builtin model");

        let backend = shared_backend();

        let model_params = LlamaModelParams::default().with_n_gpu_layers(self.n_gpu_layers);

        let model = LlamaModel::load_from_file(backend, &model_path, &model_params)
            .map_err(|e| anyhow::anyhow!("failed to load model: {e}"))?;

        eprintln!("\x1b[1;32m✓ Model loaded\x1b[0m");
        info!("builtin model loaded successfully");

        *guard = Some(LoadedModel { model });
        Ok(())
    }

    /// Run inference and return generated text.
    fn generate(&self, prompt: &str, max_tokens: u32) -> Result<(String, u64, u64)> {
        self.ensure_loaded()?;

        let guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let loaded = guard
            .as_ref()
            .context("model not loaded after ensure_loaded")?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(self.n_ctx))
            .with_n_batch(self.n_ctx);

        let mut ctx = loaded
            .model
            .new_context(shared_backend(), ctx_params)
            .map_err(|e| anyhow::anyhow!("failed to create context: {e}"))?;

        // Tokenize input
        let tokens = loaded
            .model
            .str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;

        let input_tokens = tokens.len() as u64;

        // Guard: prompt must fit in the context window with room for output.
        let max_input = self.n_ctx.saturating_sub(256) as usize; // reserve 256 for generation
        if tokens.len() > max_input {
            anyhow::bail!(
                "prompt too large for context window: {} tokens exceeds limit of {} \
                 (n_ctx={}). Try reducing tool count or prompt length.",
                tokens.len(),
                max_input,
                self.n_ctx,
            );
        }

        // Create batch with all input tokens
        let mut batch = LlamaBatch::new(self.n_ctx as usize, 1);
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch
                .add(token, i as i32, &[0], is_last)
                .map_err(|e| anyhow::anyhow!("batch add failed: {e}"))?;
        }

        // Process prompt
        ctx.decode(&mut batch)
            .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;

        // Create sampler chain: temperature + top-p + dist
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::temp(0.7),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::dist(42),
        ]);

        // Generate tokens with repetition detection.
        // Small models often get stuck repeating the same phrase — we detect
        // this by checking if the last N characters appear twice in a row and
        // stop early to avoid wasting compute.
        let mut output = String::new();
        let mut output_tokens = 0u64;
        let mut n_cur = tokens.len() as i32;
        const REPEAT_WINDOW: usize = 80; // check last 80 chars for repetition

        for _ in 0..max_tokens {
            let token = sampler.sample(&ctx, -1);

            // Check for EOS
            if loaded.model.is_eog_token(token) {
                break;
            }

            let bytes = loaded
                .model
                .token_to_piece_bytes(token, 64, true, None)
                .map_err(|e| anyhow::anyhow!("token to bytes failed: {e}"))?;
            let piece = String::from_utf8_lossy(&bytes);
            output.push_str(&piece);
            output_tokens += 1;

            // Repetition detection: if the last REPEAT_WINDOW chars appear
            // earlier in the output, the model is looping.
            if output.len() > REPEAT_WINDOW * 2 {
                let tail = &output[output.len() - REPEAT_WINDOW..];
                let before_tail = &output[..output.len() - REPEAT_WINDOW];
                if before_tail.contains(tail) {
                    warn!(
                        tokens = output_tokens,
                        "repetition detected in builtin model output, stopping early"
                    );
                    // Trim the repeated content
                    if let Some(pos) = before_tail.rfind(tail) {
                        output.truncate(pos + REPEAT_WINDOW);
                    }
                    break;
                }
            }

            // Prepare next batch
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| anyhow::anyhow!("batch add failed: {e}"))?;
            n_cur += 1;

            ctx.decode(&mut batch)
                .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;
        }

        Ok((output, input_tokens, output_tokens))
    }

    /// Format conversation messages into a ChatML prompt string (no tools).
    #[cfg(test)]
    fn format_messages(&self, messages: &[ConversationMessage]) -> String {
        self.format_messages_with_tools(messages, &[])
    }

    /// Format conversation messages with tool definitions injected into the
    /// system prompt using Qwen's tool-calling format.
    fn format_messages_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
    ) -> String {
        crate::local_tools::format_chatml_prompt(messages, tools)
    }
}

#[async_trait]
impl Provider for BuiltinProvider {
    async fn complete(&self, prompt: &str) -> Result<ChatResult> {
        let formatted = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");

        let (output_text, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.generate(&formatted, 2048))?;

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
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<ChatResult> {
        let result = self.complete(prompt).await?;
        let _ = sender.send(StreamChunk {
            delta: result.output_text.clone(),
            done: true,
            tool_call_delta: None,
        });
        Ok(result)
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        _reasoning: &ReasoningConfig,
    ) -> Result<ChatResult> {
        let prompt = self.format_messages_with_tools(messages, tools);

        let (raw_output, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.generate(&prompt, 2048))?;

        let (text, tool_calls) = crate::local_tools::parse_tool_calls(&raw_output);

        let stop_reason = if tool_calls.is_empty() {
            StopReason::EndTurn
        } else {
            StopReason::ToolUse
        };

        debug!(
            tool_count = tool_calls.len(),
            text_len = text.len(),
            "parsed builtin response"
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
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<ChatResult> {
        let result = self.complete_with_tools(messages, tools, reasoning).await?;
        let _ = sender.send(StreamChunk {
            delta: result.output_text.clone(),
            done: true,
            tool_call_delta: None,
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_with_default_model() {
        let provider = BuiltinProvider::new(String::new());
        assert_eq!(provider.model_name, model_manager::DEFAULT_BUILTIN_MODEL);
    }

    #[test]
    fn new_with_custom_model() {
        let provider = BuiltinProvider::new("my-model".to_string());
        assert_eq!(provider.model_name, "my-model");
    }

    #[test]
    fn format_messages_produces_chatml() {
        let provider = BuiltinProvider::new(String::new());
        let messages = vec![
            ConversationMessage::System {
                content: "You are helpful.".to_string(),
            },
            ConversationMessage::User {
                content: "Hello".to_string(),
                parts: vec![],
            },
        ];
        let formatted = provider.format_messages(&messages);
        assert!(formatted.contains("<|im_start|>system"));
        assert!(formatted.contains("You are helpful."));
        assert!(formatted.contains("<|im_start|>user"));
        assert!(formatted.contains("Hello"));
        assert!(formatted.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn format_messages_with_tools_injects_tool_block() {
        let provider = BuiltinProvider::new(String::new());
        let tools = vec![ToolDefinition {
            name: "web_search".to_string(),
            description: "Search the web".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string"}
                },
                "required": ["query"]
            }),
        }];
        let messages = vec![ConversationMessage::User {
            content: "Search for rust".to_string(),
            parts: vec![],
        }];
        let formatted = provider.format_messages_with_tools(&messages, &tools);

        assert!(
            formatted.contains("# Available Tools"),
            "should contain tools header"
        );
        assert!(formatted.contains("web_search"), "should contain tool name");
        assert!(
            formatted.contains("<tool_call>"),
            "should contain tool_call example"
        );
        assert!(
            formatted.contains("</tool_call>"),
            "should contain closing tag"
        );
    }

    #[test]
    fn format_messages_with_tools_appends_to_existing_system() {
        let provider = BuiltinProvider::new(String::new());
        let tools = vec![ToolDefinition {
            name: "read_file".to_string(),
            description: "Read a file".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let messages = vec![
            ConversationMessage::System {
                content: "You are a researcher.".to_string(),
            },
            ConversationMessage::User {
                content: "Read test.txt".to_string(),
                parts: vec![],
            },
        ];
        let formatted = provider.format_messages_with_tools(&messages, &tools);

        // The system message should contain both the original content and the tools block
        assert!(formatted.contains("You are a researcher."));
        assert!(formatted.contains("# Available Tools"));
        // There should only be ONE system block, not two
        assert_eq!(
            formatted.matches("<|im_start|>system").count(),
            1,
            "should have exactly one system block"
        );
    }

    #[test]
    fn format_messages_with_tools_no_system_injects_synthetic() {
        let provider = BuiltinProvider::new(String::new());
        let tools = vec![ToolDefinition {
            name: "shell".to_string(),
            description: "Run a command".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
        }];
        let messages = vec![ConversationMessage::User {
            content: "List files".to_string(),
            parts: vec![],
        }];
        let formatted = provider.format_messages_with_tools(&messages, &tools);

        assert!(
            formatted.contains("You are a helpful assistant."),
            "should inject synthetic system prompt"
        );
        assert!(formatted.contains("# Available Tools"));
    }

    // Tool call parsing and ChatML formatting tests are in local_tools::tests.
    // These tests verify the builtin provider delegates correctly.
}

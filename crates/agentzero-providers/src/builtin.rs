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
use llama_cpp_2::token::LlamaToken;
use tracing::{debug, info};

use agentzero_core::types::{
    ChatResult, ConversationMessage, ReasoningConfig, StopReason, StreamChunk, ToolDefinition,
};
use agentzero_core::Provider;

use crate::local_llm::{GenerationLoop, LocalLlm};
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

// ---------------------------------------------------------------------------
// LlamaCppLlm — LocalLlm implementation for llama.cpp
// ---------------------------------------------------------------------------

/// Short-lived wrapper created for a single generation run.
///
/// Holds the llama.cpp context, batch, and sampler state needed to drive
/// token-by-token generation via [`LocalLlm`].
struct LlamaCppLlm<'model> {
    model: &'model LlamaModel,
    ctx: llama_cpp_2::context::LlamaContext<'model>,
    batch: LlamaBatch<'model>,
    sampler: LlamaSampler,
}

impl<'model> LlamaCppLlm<'model> {
    fn new(
        model: &'model LlamaModel,
        ctx: llama_cpp_2::context::LlamaContext<'model>,
        n_ctx: usize,
    ) -> Self {
        Self {
            model,
            ctx,
            batch: LlamaBatch::new(n_ctx, 1),
            sampler: LlamaSampler::chain_simple([
                LlamaSampler::temp(0.7),
                LlamaSampler::top_p(0.9, 1),
                LlamaSampler::dist(42),
            ]),
        }
    }

    /// Apply a JSON schema grammar constraint to the sampler chain.
    ///
    /// Converts the JSON schema to a GBNF grammar using llama.cpp's built-in
    /// converter, then prepends a grammar sampler so every generated token
    /// conforms to the schema. This guarantees valid JSON output from local
    /// models — no malformed tool calls.
    fn apply_json_schema_grammar(&mut self, json_schema: &str) -> Result<()> {
        let gbnf = llama_cpp_2::json_schema_to_grammar(json_schema)
            .map_err(|e| anyhow::anyhow!("failed to convert JSON schema to grammar: {e}"))?;
        let grammar_sampler = LlamaSampler::grammar(self.model, &gbnf, "root")
            .map_err(|e| anyhow::anyhow!("failed to create grammar sampler: {e}"))?;
        // Rebuild the sampler chain with grammar first (most restrictive → least).
        self.sampler = LlamaSampler::chain_simple([
            grammar_sampler,
            LlamaSampler::temp(0.7),
            LlamaSampler::top_p(0.9, 1),
            LlamaSampler::dist(42),
        ]);
        tracing::debug!(
            "applied JSON schema grammar constraint ({} bytes GBNF)",
            gbnf.len()
        );
        Ok(())
    }
}

impl LocalLlm for LlamaCppLlm<'_> {
    fn tokenize(&self, text: &str) -> Result<Vec<u32>> {
        let tokens = self
            .model
            .str_to_token(text, llama_cpp_2::model::AddBos::Always)
            .map_err(|e| anyhow::anyhow!("tokenization failed: {e}"))?;
        Ok(tokens.iter().map(|t| t.0 as u32).collect())
    }

    fn feed_prompt(&mut self, tokens: &[u32]) -> Result<u32> {
        self.batch.clear();
        for (i, &token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            self.batch
                .add(LlamaToken(token as i32), i as i32, &[0], is_last)
                .map_err(|e| anyhow::anyhow!("batch add failed: {e}"))?;
        }
        self.ctx
            .decode(&mut self.batch)
            .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;
        let token = self.sampler.sample(&self.ctx, -1);
        Ok(token.0 as u32)
    }

    fn step(&mut self, token: u32, pos: usize) -> Result<u32> {
        self.batch.clear();
        self.batch
            .add(LlamaToken(token as i32), pos as i32, &[0], true)
            .map_err(|e| anyhow::anyhow!("batch add failed: {e}"))?;
        self.ctx
            .decode(&mut self.batch)
            .map_err(|e| anyhow::anyhow!("decode failed: {e}"))?;
        let next = self.sampler.sample(&self.ctx, -1);
        Ok(next.0 as u32)
    }

    fn decode_token(&self, token: u32) -> Result<String> {
        let bytes = self
            .model
            .token_to_piece_bytes(LlamaToken(token as i32), 64, true, None)
            .map_err(|e| anyhow::anyhow!("token to bytes failed: {e}"))?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    fn is_eos(&self, token: u32) -> bool {
        self.model.is_eog_token(LlamaToken(token as i32))
    }
}

// ---------------------------------------------------------------------------
// BuiltinProvider
// ---------------------------------------------------------------------------

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
        } else if self.model_name.ends_with(".azb")
            && std::path::Path::new(&self.model_name).exists()
        {
            model_manager::load_from_bundle(std::path::Path::new(&self.model_name))?
        } else if self.model_name == model_manager::DEFAULT_BUILTIN_MODEL
            || self.model_name == "default"
        {
            model_manager::ensure_default_model()?
        } else if self.model_name.ends_with(".gguf")
            && std::path::Path::new(&self.model_name).exists()
        {
            PathBuf::from(&self.model_name)
        } else if let Some(entry) = model_manager::resolve_model(&self.model_name) {
            // Known model ID from the GGUF registry (e.g. "qwen2.5-coder-7b")
            model_manager::ensure_model(&entry.hf_repo, &entry.gguf_file)?
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

    /// Run inference with the shared [`GenerationLoop`], optionally streaming.
    ///
    /// Locks the inner model, creates a fresh llama.cpp context + a
    /// short-lived [`LlamaCppLlm`], tokenizes the prompt, validates against
    /// the context window, and runs the loop.
    fn run_inference(
        &self,
        prompt: &str,
        max_tokens: u32,
        sender: Option<&tokio::sync::mpsc::UnboundedSender<StreamChunk>>,
    ) -> Result<(String, u64, u64)> {
        self.run_inference_with_grammar(prompt, max_tokens, sender, None)
    }

    fn run_inference_with_grammar(
        &self,
        prompt: &str,
        max_tokens: u32,
        sender: Option<&tokio::sync::mpsc::UnboundedSender<StreamChunk>>,
        json_schema: Option<&str>,
    ) -> Result<(String, u64, u64)> {
        self.ensure_loaded()?;

        let guard = self.inner.lock().map_err(|e| anyhow::anyhow!("{e}"))?;
        let loaded = guard
            .as_ref()
            .context("model not loaded after ensure_loaded")?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(self.n_ctx))
            .with_n_batch(self.n_ctx);

        let ctx = loaded
            .model
            .new_context(shared_backend(), ctx_params)
            .map_err(|e| anyhow::anyhow!("failed to create context: {e}"))?;

        let mut llm = LlamaCppLlm::new(&loaded.model, ctx, self.n_ctx as usize);

        // Apply grammar constraint if a JSON schema was provided.
        if let Some(schema) = json_schema {
            if let Err(e) = llm.apply_json_schema_grammar(schema) {
                tracing::warn!(error = %e, "grammar constraint failed, falling back to unconstrained generation");
            }
        }

        let tokens = llm.tokenize(prompt)?;

        // Guard: prompt must fit in the context window with room for output.
        let max_input = self.n_ctx.saturating_sub(256) as usize;
        if tokens.len() > max_input {
            anyhow::bail!(
                "prompt too large for context window: {} tokens exceeds limit of {} \
                 (n_ctx={}). Try reducing tool count or prompt length.",
                tokens.len(),
                max_input,
                self.n_ctx,
            );
        }

        let gen = GenerationLoop {
            max_tokens,
            repeat_window: 80,
        };
        gen.run(&mut llm, &tokens, sender)
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
    fn supports_streaming(&self) -> bool {
        true
    }

    async fn complete(&self, prompt: &str) -> Result<ChatResult> {
        let formatted = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");

        let (output_text, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.run_inference(&formatted, 2048, None))?;

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
        let formatted = format!("<|im_start|>user\n{prompt}<|im_end|>\n<|im_start|>assistant\n");

        let (output_text, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.run_inference(&formatted, 2048, Some(&sender)))?;

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
        let prompt = self.format_messages_with_tools(messages, tools);

        let (raw_output, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.run_inference(&prompt, 2048, None))?;

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
        _reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> Result<ChatResult> {
        let prompt = self.format_messages_with_tools(messages, tools);

        let (raw_output, input_tokens, output_tokens) =
            tokio::task::block_in_place(|| self.run_inference(&prompt, 2048, Some(&sender)))?;

        let (text, tool_calls) = crate::local_tools::parse_tool_calls(&raw_output);

        let stop_reason = if tool_calls.is_empty() {
            StopReason::EndTurn
        } else {
            StopReason::ToolUse
        };

        debug!(
            tool_count = tool_calls.len(),
            text_len = text.len(),
            "parsed builtin streaming response"
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

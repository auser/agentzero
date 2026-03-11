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
    ToolUseRequest,
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

        let ctx_params =
            LlamaContextParams::default().with_n_ctx(std::num::NonZeroU32::new(self.n_ctx));

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
        debug!(input_tokens, n_ctx = self.n_ctx, "tokenized prompt");

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

        // Generate tokens
        let mut output = String::new();
        let mut output_tokens = 0u64;
        let mut n_cur = tokens.len() as i32;

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
        let mut prompt = String::new();
        let mut has_system = false;

        for msg in messages {
            match msg {
                ConversationMessage::System { content } => {
                    prompt.push_str("<|im_start|>system\n");
                    prompt.push_str(content);
                    if !tools.is_empty() {
                        prompt.push_str(&format_tools_system_block(tools));
                    }
                    prompt.push_str("<|im_end|>\n");
                    has_system = true;
                }
                ConversationMessage::User { content, .. } => {
                    // If no system message was seen yet and we have tools,
                    // inject a synthetic system message first.
                    if !has_system && !tools.is_empty() {
                        prompt.push_str("<|im_start|>system\n");
                        prompt.push_str("You are a helpful assistant.");
                        prompt.push_str(&format_tools_system_block(tools));
                        prompt.push_str("<|im_end|>\n");
                        has_system = true;
                    }
                    prompt.push_str("<|im_start|>user\n");
                    prompt.push_str(content);
                    prompt.push_str("<|im_end|>\n");
                }
                ConversationMessage::Assistant {
                    content,
                    tool_calls,
                } => {
                    prompt.push_str("<|im_start|>assistant\n");
                    if let Some(text) = content {
                        prompt.push_str(text);
                    }
                    // Replay previous tool calls in the expected format
                    for tc in tool_calls {
                        prompt.push_str("\n<tool_call>\n");
                        let call_json = serde_json::json!({
                            "name": tc.name,
                            "arguments": tc.input,
                        });
                        prompt.push_str(&serde_json::to_string(&call_json).unwrap_or_default());
                        prompt.push_str("\n</tool_call>");
                    }
                    prompt.push_str("<|im_end|>\n");
                }
                ConversationMessage::ToolResult(result) => {
                    prompt.push_str("<|im_start|>tool\n");
                    prompt.push_str(&result.content);
                    prompt.push_str("<|im_end|>\n");
                }
            }
        }

        // Start assistant turn
        prompt.push_str("<|im_start|>assistant\n");
        prompt
    }
}

/// Build the tool-definition block appended to the system prompt.
///
/// Uses Qwen's expected format:
/// ```text
/// \n\n# Tools
/// You may call one or more functions to assist with the user query.
/// ...
/// <tools>
/// {"type": "function", "function": {"name": "...", ...}}
/// </tools>
/// ```
fn format_tools_system_block(tools: &[ToolDefinition]) -> String {
    let mut block = String::from(
        "\n\n# Tools\n\
         You may call one or more functions to assist with the user query.\n\n\
         You are provided with function signatures within <tools></tools> XML tags:\n<tools>\n",
    );

    for tool in tools {
        let tool_json = serde_json::json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
            }
        });
        if let Ok(s) = serde_json::to_string(&tool_json) {
            block.push_str(&s);
            block.push('\n');
        }
    }

    block.push_str(
        "</tools>\n\n\
         For each function call, return a json object with function name and arguments \
         within <tool_call></tool_call> XML tags:\n\
         <tool_call>\n\
         {\"name\": <function-name>, \"arguments\": <args-json-object>}\n\
         </tool_call>",
    );

    block
}

/// Parse `<tool_call>...</tool_call>` blocks from model output.
///
/// Returns `(text_output, tool_calls)` where `text_output` is the model
/// response with tool_call blocks removed and `tool_calls` is the parsed list.
fn parse_tool_calls(raw: &str) -> (String, Vec<ToolUseRequest>) {
    let mut tool_calls = Vec::new();
    let mut text = String::new();
    let mut remaining = raw;
    let mut call_index = 0usize;

    loop {
        match remaining.find("<tool_call>") {
            None => {
                text.push_str(remaining);
                break;
            }
            Some(start) => {
                // Text before the tag
                text.push_str(&remaining[..start]);

                let after_open = &remaining[start + "<tool_call>".len()..];
                match after_open.find("</tool_call>") {
                    None => {
                        // Unterminated — treat entire remainder as text
                        text.push_str(&remaining[start..]);
                        break;
                    }
                    Some(end) => {
                        let json_str = after_open[..end].trim();
                        if let Some(tc) = parse_single_tool_call(json_str, call_index) {
                            call_index += 1;
                            tool_calls.push(tc);
                        } else {
                            warn!(json = json_str, "failed to parse tool_call JSON");
                            // Keep it in the text output so nothing is silently lost
                            text.push_str(
                                &remaining[start
                                    ..start + "<tool_call>".len() + end + "</tool_call>".len()],
                            );
                        }
                        remaining = &after_open[end + "</tool_call>".len()..];
                    }
                }
            }
        }
    }

    let text = text.trim().to_string();
    (text, tool_calls)
}

/// Parse a single tool call JSON object.
/// Accepts `{"name": "...", "arguments": {...}}`.
fn parse_single_tool_call(json_str: &str, index: usize) -> Option<ToolUseRequest> {
    let v: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let name = v.get("name")?.as_str()?;
    let arguments = v
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    Some(ToolUseRequest {
        id: format!("builtin_tc_{index}"),
        name: name.to_string(),
        input: arguments,
    })
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

        let (text, tool_calls) = parse_tool_calls(&raw_output);

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
    use agentzero_core::types::ToolResultMessage;

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
            formatted.contains("<tools>"),
            "should contain <tools> block"
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
        assert!(formatted.contains("<tools>"));
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
        assert!(formatted.contains("<tools>"));
    }

    #[test]
    fn format_messages_replays_tool_calls_in_history() {
        let provider = BuiltinProvider::new(String::new());
        let messages = vec![
            ConversationMessage::User {
                content: "Search for AI".to_string(),
                parts: vec![],
            },
            ConversationMessage::Assistant {
                content: Some("I'll search for that.".to_string()),
                tool_calls: vec![ToolUseRequest {
                    id: "tc_0".to_string(),
                    name: "web_search".to_string(),
                    input: serde_json::json!({"query": "AI"}),
                }],
            },
            ConversationMessage::ToolResult(ToolResultMessage {
                tool_use_id: "tc_0".to_string(),
                content: "Found 10 results about AI.".to_string(),
                is_error: false,
            }),
        ];
        let formatted = provider.format_messages(&messages);

        assert!(formatted.contains("<tool_call>"));
        assert!(formatted.contains("web_search"));
        assert!(formatted.contains("<|im_start|>tool"));
        assert!(formatted.contains("Found 10 results about AI."));
    }

    // ── parse_tool_calls tests ───────────────────────────────────────────

    #[test]
    fn parse_tool_calls_extracts_single_call() {
        let raw = "I'll search for that.\n\
                    <tool_call>\n\
                    {\"name\": \"web_search\", \"arguments\": {\"query\": \"rust programming\"}}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, "I'll search for that.");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[0].input["query"], "rust programming");
        assert_eq!(calls[0].id, "builtin_tc_0");
    }

    #[test]
    fn parse_tool_calls_extracts_multiple_calls() {
        let raw = "Let me do two things.\n\
                    <tool_call>\n\
                    {\"name\": \"web_search\", \"arguments\": {\"query\": \"rust\"}}\n\
                    </tool_call>\n\
                    <tool_call>\n\
                    {\"name\": \"write_file\", \"arguments\": {\"path\": \"out.md\", \"content\": \"hello\"}}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, "Let me do two things.");
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[0].id, "builtin_tc_0");
        assert_eq!(calls[1].name, "write_file");
        assert_eq!(calls[1].id, "builtin_tc_1");
    }

    #[test]
    fn parse_tool_calls_no_calls_returns_text() {
        let raw = "Just a normal response with no tool calls.";
        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(text, raw);
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_malformed_json() {
        let raw = "Trying something.\n\
                    <tool_call>\n\
                    {not valid json}\n\
                    </tool_call>";

        let (text, calls) = parse_tool_calls(raw);
        // Malformed JSON should be kept in text output
        assert!(text.contains("{not valid json}"));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_unterminated_tag() {
        let raw = "Some text\n<tool_call>\n{\"name\": \"x\"}";
        let (text, calls) = parse_tool_calls(raw);
        assert!(text.contains("<tool_call>"));
        assert!(calls.is_empty());
    }

    #[test]
    fn parse_tool_calls_handles_missing_arguments() {
        let raw = "<tool_call>\n\
                    {\"name\": \"simple_tool\"}\n\
                    </tool_call>";

        let (_, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "simple_tool");
        assert!(calls[0].input.is_object());
    }

    #[test]
    fn parse_tool_calls_preserves_text_between_calls() {
        let raw = "First I'll search.\n\
                    <tool_call>\n\
                    {\"name\": \"search\", \"arguments\": {\"q\": \"a\"}}\n\
                    </tool_call>\n\
                    Then I'll write.\n\
                    <tool_call>\n\
                    {\"name\": \"write\", \"arguments\": {\"f\": \"b\"}}\n\
                    </tool_call>\n\
                    Done.";

        let (text, calls) = parse_tool_calls(raw);
        assert_eq!(calls.len(), 2);
        assert!(text.contains("First I'll search."));
        assert!(text.contains("Then I'll write."));
        assert!(text.contains("Done."));
    }
}

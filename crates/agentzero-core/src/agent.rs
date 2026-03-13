use crate::common::privacy_helpers::{is_network_tool, resolve_boundary};
use crate::loop_detection::{LoopDetectionConfig, ToolLoopDetector};
use crate::security::redaction::redact_text;
use crate::types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, ConversationMessage,
    HookEvent, HookFailureMode, HookRiskTier, HookSink, LoopAction, MemoryEntry, MemoryStore,
    MetricsSink, Provider, ResearchTrigger, StopReason, StreamSink, Tool, ToolContext,
    ToolDefinition, ToolResultMessage, ToolSelector, ToolSummary, ToolUseRequest, UserMessage,
};
use crate::validation::validate_json;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{timeout, Duration};
use tracing::{info, info_span, instrument, warn, Instrument};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

fn cap_prompt(input: &str, max_chars: usize) -> (String, bool) {
    let len = input.chars().count();
    if len <= max_chars {
        return (input.to_string(), false);
    }
    let truncated = input
        .chars()
        .rev()
        .take(max_chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    (truncated, true)
}

fn build_provider_prompt(
    current_prompt: &str,
    recent_memory: &[MemoryEntry],
    max_prompt_chars: usize,
) -> (String, bool) {
    if recent_memory.is_empty() {
        return cap_prompt(current_prompt, max_prompt_chars);
    }

    let mut prompt = String::from("Recent memory:\n");
    for entry in recent_memory.iter().rev() {
        prompt.push_str("- ");
        prompt.push_str(&entry.role);
        prompt.push_str(": ");
        prompt.push_str(&entry.content);
        prompt.push('\n');
    }
    prompt.push_str("\nCurrent input:\n");
    prompt.push_str(current_prompt);

    cap_prompt(&prompt, max_prompt_chars)
}

/// Extract tool calls from a prompt. Each line starting with `tool:` is a call.
fn parse_tool_calls(prompt: &str) -> Vec<(&str, &str)> {
    prompt
        .lines()
        .filter_map(|line| {
            line.strip_prefix("tool:").map(|rest| {
                let rest = rest.trim();
                let mut parts = rest.splitn(2, ' ');
                let name = parts.next().unwrap_or_default();
                let input = parts.next().unwrap_or_default();
                (name, input)
            })
        })
        .collect()
}

/// Attempt to extract a tool call from LLM text output.
///
/// Many local models (llama.cpp, ollama, etc.) don't return structured `tool_calls`
/// in the OpenAI response format. Instead, they emit the tool invocation as a JSON
/// code block or raw JSON in the `content` field. This function detects common
/// patterns and converts them into a proper `ToolUseRequest` so the agent loop
/// can dispatch the tool.
///
/// Recognized formats:
/// - ```json\n{"name": "tool", "arguments": {...}}\n```
/// - {"name": "tool", "arguments": {...}}
/// - {"name": "tool", "parameters": {...}}
fn extract_tool_call_from_text(
    text: &str,
    known_tools: &[ToolDefinition],
) -> Option<ToolUseRequest> {
    // Try to find JSON in a code block first, then raw JSON.
    let json_str = extract_json_block(text).or_else(|| extract_bare_json(text))?;
    let obj: serde_json::Value = serde_json::from_str(json_str).ok()?;
    let name = obj.get("name")?.as_str()?;

    // Only extract if the name matches a known tool — avoids false positives.
    if !known_tools.iter().any(|t| t.name == name) {
        return None;
    }

    let args = obj
        .get("arguments")
        .or_else(|| obj.get("parameters"))
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    Some(ToolUseRequest {
        id: format!("text_extracted_{}", name),
        name: name.to_string(),
        input: args,
    })
}

/// Extract JSON from a fenced code block (```json ... ``` or ``` ... ```).
fn extract_json_block(text: &str) -> Option<&str> {
    let start = text.find("```")?;
    let after_fence = &text[start + 3..];
    // Skip optional language tag on the same line.
    let content_start = after_fence.find('\n')? + 1;
    let content = &after_fence[content_start..];
    let end = content.find("```")?;
    let block = content[..end].trim();
    if block.starts_with('{') {
        Some(block)
    } else {
        None
    }
}

/// Extract the first top-level `{...}` JSON object from text.
fn extract_bare_json(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let candidate = &text[start..];
    // Find the matching closing brace by counting depth.
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    for (i, ch) in candidate.char_indices() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(&candidate[..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// If the JSON schema declares exactly one required string property, return its name.
fn single_required_string_field(schema: &serde_json::Value) -> Option<String> {
    let required = schema.get("required")?.as_array()?;
    if required.len() != 1 {
        return None;
    }
    let field_name = required[0].as_str()?;
    let properties = schema.get("properties")?.as_object()?;
    let field_schema = properties.get(field_name)?;
    if field_schema.get("type")?.as_str()? == "string" {
        Some(field_name.to_string())
    } else {
        None
    }
}

/// Convert structured tool input (`Value`) to the `&str` format `Tool::execute()` expects.
///
/// - Validates the input against the tool's JSON schema (if present).
/// - If the value is a bare JSON string, unwrap it.
/// - If the tool schema has a single required string field, extract that field.
/// - Otherwise, serialize to JSON string (for tools that parse JSON via `from_str`).
///
/// Returns `Err` with a human-readable message if schema validation fails.
fn prepare_tool_input(tool: &dyn Tool, raw_input: &serde_json::Value) -> Result<String, String> {
    // Bare strings are the legacy text-input path — pass through without schema validation.
    if let Some(s) = raw_input.as_str() {
        return Ok(s.to_string());
    }

    // Validate structured input against the tool's JSON schema.
    if let Some(schema) = tool.input_schema() {
        if let Err(errors) = validate_json(raw_input, &schema) {
            return Err(format!(
                "Invalid input for tool '{}': {}",
                tool.name(),
                errors.join("; ")
            ));
        }

        if let Some(field) = single_required_string_field(&schema) {
            if let Some(val) = raw_input.get(&field).and_then(|v| v.as_str()) {
                return Ok(val.to_string());
            }
        }
    }

    Ok(serde_json::to_string(raw_input).unwrap_or_default())
}

/// Truncate conversation messages to fit within a character budget.
/// Preserves the prefix (system prompt + first user message) and drops from
/// the middle, keeping the most recent messages that fit.
///
/// Tool-use aware: when keeping `ToolResult` messages from the end, the
/// preceding `Assistant` message (which contains the matching `tool_use`
/// blocks) is always kept as well to avoid orphaned tool_result errors
/// from the Anthropic API.
fn truncate_messages(messages: &mut Vec<ConversationMessage>, max_chars: usize) {
    let total_chars: usize = messages.iter().map(|m| m.char_count()).sum();
    if total_chars <= max_chars || messages.len() <= 2 {
        return;
    }

    // Find the prefix to always preserve: everything up to and including
    // the first User message. This ensures the system prompt and the
    // initial user request are never dropped (system messages are filtered
    // out by providers, so keeping only messages[0] when it is a System
    // would leave the first API message as a ToolResult — a protocol error).
    let mut prefix_end = 1; // at minimum keep messages[0]
    for (i, msg) in messages.iter().enumerate() {
        if matches!(msg, ConversationMessage::User { .. }) {
            prefix_end = i + 1;
            break;
        }
    }

    let prefix_cost: usize = messages[..prefix_end].iter().map(|m| m.char_count()).sum();
    let mut budget = max_chars.saturating_sub(prefix_cost);

    // Walk backward from the end, accumulating messages that fit.
    // Track whether we're inside a tool_result run so we can also keep the
    // preceding assistant message that contains the tool_use blocks.
    let mut keep_from_end = 0;
    let mut in_tool_result_run = false;
    for msg in messages[prefix_end..].iter().rev() {
        let cost = msg.char_count();
        let is_tool_result = matches!(msg, ConversationMessage::ToolResult(_));
        let is_assistant_with_tools = matches!(
            msg,
            ConversationMessage::Assistant { tool_calls, .. } if !tool_calls.is_empty()
        );

        if is_tool_result {
            in_tool_result_run = true;
        }

        // If we're in a tool_result run and hit the assistant message that
        // produced these tool calls, we MUST include it regardless of budget
        // to keep the tool_use/tool_result pairing valid.
        if in_tool_result_run && is_assistant_with_tools {
            budget = budget.saturating_sub(cost);
            keep_from_end += 1;
            in_tool_result_run = false;
            continue;
        }

        if !is_tool_result {
            in_tool_result_run = false;
        }

        if cost > budget {
            break;
        }
        budget -= cost;
        keep_from_end += 1;
    }

    if keep_from_end == 0 {
        // Nothing from the tail fits. Keep only the prefix.
        messages.truncate(prefix_end);
        return;
    }

    let split_point = messages.len() - keep_from_end;
    if split_point <= prefix_end {
        return;
    }

    messages.drain(prefix_end..split_point);

    // Post-truncation cleanup: remove any leading ToolResult messages in the
    // kept tail that have no preceding Assistant with the matching tool_use.
    // These orphaned tool_results cause Anthropic API errors because their
    // tool_use_ids have no corresponding tool_use block.
    while messages.len() > prefix_end {
        if matches!(&messages[prefix_end], ConversationMessage::ToolResult(_)) {
            messages.remove(prefix_end);
        } else {
            break;
        }
    }
}

/// Convert recent memory entries to chronological `ConversationMessage` list.
/// `memory.recent()` returns newest-first; this reverses to chronological order.
fn memory_to_messages(entries: &[MemoryEntry]) -> Vec<ConversationMessage> {
    entries
        .iter()
        .rev()
        .map(|entry| {
            if entry.role == "assistant" {
                ConversationMessage::Assistant {
                    content: Some(entry.content.clone()),
                    tool_calls: vec![],
                }
            } else {
                ConversationMessage::user(entry.content.clone())
            }
        })
        .collect()
}

pub struct Agent {
    config: AgentConfig,
    provider: Box<dyn Provider>,
    memory: Box<dyn MemoryStore>,
    tools: Vec<Box<dyn Tool>>,
    audit: Option<Box<dyn AuditSink>>,
    hooks: Option<Box<dyn HookSink>>,
    metrics: Option<Box<dyn MetricsSink>>,
    loop_detection_config: Option<LoopDetectionConfig>,
    tool_selector: Option<Box<dyn ToolSelector>>,
}

impl Agent {
    pub fn new(
        config: AgentConfig,
        provider: Box<dyn Provider>,
        memory: Box<dyn MemoryStore>,
        tools: Vec<Box<dyn Tool>>,
    ) -> Self {
        Self {
            config,
            provider,
            memory,
            tools,
            audit: None,
            hooks: None,
            metrics: None,
            loop_detection_config: None,
            tool_selector: None,
        }
    }

    /// Enable tiered loop detection (similarity + cost runaway) for this agent.
    pub fn with_loop_detection(mut self, config: LoopDetectionConfig) -> Self {
        self.loop_detection_config = Some(config);
        self
    }

    pub fn with_audit(mut self, audit: Box<dyn AuditSink>) -> Self {
        self.audit = Some(audit);
        self
    }

    pub fn with_hooks(mut self, hooks: Box<dyn HookSink>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn with_metrics(mut self, metrics: Box<dyn MetricsSink>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub fn with_tool_selector(mut self, selector: Box<dyn ToolSelector>) -> Self {
        self.tool_selector = Some(selector);
        self
    }

    /// Add a tool to this agent after construction.
    pub fn add_tool(&mut self, tool: Box<dyn Tool>) {
        self.tools.push(tool);
    }

    /// Build tool definitions for all registered tools that have an input schema.
    fn build_tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
            .filter_map(|tool| ToolDefinition::from_tool(&**tool))
            .collect()
    }

    /// Check if any registered tools have schemas (can participate in structured tool use).
    fn has_tool_definitions(&self) -> bool {
        self.tools.iter().any(|t| t.input_schema().is_some())
    }

    async fn audit(&self, stage: &str, detail: serde_json::Value) {
        if let Some(sink) = &self.audit {
            let _ = sink
                .record(AuditEvent {
                    stage: stage.to_string(),
                    detail,
                })
                .await;
        }
    }

    fn next_request_id() -> String {
        let ts_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("req-{ts_ms}-{seq}")
    }

    fn hook_risk_tier(stage: &str) -> HookRiskTier {
        if matches!(
            stage,
            "before_tool_call" | "after_tool_call" | "before_plugin_call" | "after_plugin_call"
        ) {
            HookRiskTier::High
        } else if matches!(
            stage,
            "before_provider_call"
                | "after_provider_call"
                | "before_memory_write"
                | "after_memory_write"
                | "before_run"
                | "after_run"
        ) {
            HookRiskTier::Medium
        } else {
            HookRiskTier::Low
        }
    }

    fn hook_failure_mode_for_stage(&self, stage: &str) -> HookFailureMode {
        if self.config.hooks.fail_closed {
            return HookFailureMode::Block;
        }

        match Self::hook_risk_tier(stage) {
            HookRiskTier::Low => self.config.hooks.low_tier_mode,
            HookRiskTier::Medium => self.config.hooks.medium_tier_mode,
            HookRiskTier::High => self.config.hooks.high_tier_mode,
        }
    }

    async fn hook(&self, stage: &str, detail: serde_json::Value) -> Result<(), AgentError> {
        if !self.config.hooks.enabled {
            return Ok(());
        }
        let Some(sink) = &self.hooks else {
            return Ok(());
        };

        let event = HookEvent {
            stage: stage.to_string(),
            detail,
        };
        let hook_call = sink.record(event);
        let mode = self.hook_failure_mode_for_stage(stage);
        let tier = Self::hook_risk_tier(stage);
        match timeout(
            Duration::from_millis(self.config.hooks.timeout_ms),
            hook_call,
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                let redacted = redact_text(&err.to_string());
                match mode {
                    HookFailureMode::Block => Err(AgentError::Hook {
                        stage: stage.to_string(),
                        source: err,
                    }),
                    HookFailureMode::Warn => {
                        warn!(
                            stage = stage,
                            tier = ?tier,
                            mode = "warn",
                            "hook error (continuing): {redacted}"
                        );
                        self.audit(
                            "hook_error_warn",
                            json!({"stage": stage, "tier": format!("{tier:?}").to_ascii_lowercase(), "error": redacted}),
                        )
                        .await;
                        Ok(())
                    }
                    HookFailureMode::Ignore => {
                        self.audit(
                            "hook_error_ignored",
                            json!({"stage": stage, "tier": format!("{tier:?}").to_ascii_lowercase(), "error": redacted}),
                        )
                        .await;
                        Ok(())
                    }
                }
            }
            Err(_) => match mode {
                HookFailureMode::Block => Err(AgentError::Hook {
                    stage: stage.to_string(),
                    source: anyhow::anyhow!(
                        "hook execution timed out after {} ms",
                        self.config.hooks.timeout_ms
                    ),
                }),
                HookFailureMode::Warn => {
                    warn!(
                        stage = stage,
                        tier = ?tier,
                        mode = "warn",
                        timeout_ms = self.config.hooks.timeout_ms,
                        "hook timeout (continuing)"
                    );
                    self.audit(
                            "hook_timeout_warn",
                            json!({"stage": stage, "tier": format!("{tier:?}").to_ascii_lowercase(), "timeout_ms": self.config.hooks.timeout_ms}),
                        )
                        .await;
                    Ok(())
                }
                HookFailureMode::Ignore => {
                    self.audit(
                            "hook_timeout_ignored",
                            json!({"stage": stage, "tier": format!("{tier:?}").to_ascii_lowercase(), "timeout_ms": self.config.hooks.timeout_ms}),
                        )
                        .await;
                    Ok(())
                }
            },
        }
    }

    fn increment_counter(&self, name: &'static str) {
        if let Some(metrics) = &self.metrics {
            metrics.increment_counter(name, 1);
        }
    }

    fn observe_histogram(&self, name: &'static str, value: f64) {
        if let Some(metrics) = &self.metrics {
            metrics.observe_histogram(name, value);
        }
    }

    #[instrument(skip(self, tool, tool_input, ctx), fields(tool = tool_name, request_id, iteration))]
    async fn execute_tool(
        &self,
        tool: &dyn Tool,
        tool_name: &str,
        tool_input: &str,
        ctx: &ToolContext,
        request_id: &str,
        iteration: usize,
    ) -> Result<crate::types::ToolResult, AgentError> {
        // Privacy boundary enforcement: resolve tool-specific boundary against
        // the agent's boundary, then check if the tool is allowed.
        let tool_specific = self
            .config
            .tool_boundaries
            .get(tool_name)
            .map(|s| s.as_str())
            .unwrap_or("");
        let resolved = resolve_boundary(tool_specific, &self.config.privacy_boundary);
        if resolved == "local_only" && is_network_tool(tool_name) {
            return Err(AgentError::Tool {
                tool: tool_name.to_string(),
                source: anyhow::anyhow!(
                    "tool '{}' requires network access but privacy boundary is 'local_only'",
                    tool_name
                ),
            });
        }

        let is_plugin_call = tool_name.starts_with("plugin:");
        self.hook(
            "before_tool_call",
            json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name}),
        )
        .await?;
        if is_plugin_call {
            self.hook(
                "before_plugin_call",
                json!({"request_id": request_id, "iteration": iteration, "plugin_tool": tool_name}),
            )
            .await?;
        }
        self.audit(
            "tool_execute_start",
            json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name}),
        )
        .await;
        let tool_span = info_span!(
            "tool_call",
            tool = %tool_name,
            request_id = %request_id,
            iteration = iteration,
        );
        let _tool_guard = tool_span.enter();
        let tool_started = Instant::now();
        let tool_timeout_ms = self.config.tool_timeout_ms;
        let result = if tool_timeout_ms > 0 {
            match timeout(
                Duration::from_millis(tool_timeout_ms),
                tool.execute(tool_input, ctx),
            )
            .await
            {
                Ok(Ok(result)) => result,
                Ok(Err(source)) => {
                    self.observe_histogram(
                        "tool_latency_ms",
                        tool_started.elapsed().as_secs_f64() * 1000.0,
                    );
                    self.increment_counter("tool_errors_total");
                    return Err(AgentError::Tool {
                        tool: tool_name.to_string(),
                        source,
                    });
                }
                Err(_elapsed) => {
                    self.observe_histogram(
                        "tool_latency_ms",
                        tool_started.elapsed().as_secs_f64() * 1000.0,
                    );
                    self.increment_counter("tool_errors_total");
                    self.increment_counter("tool_timeouts_total");
                    warn!(
                        tool = %tool_name,
                        timeout_ms = tool_timeout_ms,
                        "tool execution timed out"
                    );
                    return Err(AgentError::Tool {
                        tool: tool_name.to_string(),
                        source: anyhow::anyhow!(
                            "tool '{}' timed out after {}ms",
                            tool_name,
                            tool_timeout_ms
                        ),
                    });
                }
            }
        } else {
            match tool.execute(tool_input, ctx).await {
                Ok(result) => result,
                Err(source) => {
                    self.observe_histogram(
                        "tool_latency_ms",
                        tool_started.elapsed().as_secs_f64() * 1000.0,
                    );
                    self.increment_counter("tool_errors_total");
                    return Err(AgentError::Tool {
                        tool: tool_name.to_string(),
                        source,
                    });
                }
            }
        };
        self.observe_histogram(
            "tool_latency_ms",
            tool_started.elapsed().as_secs_f64() * 1000.0,
        );
        self.audit(
            "tool_execute_success",
            json!({
                "request_id": request_id,
                "iteration": iteration,
                "tool_name": tool_name,
                "tool_output_len": result.output.len(),
                "duration_ms": tool_started.elapsed().as_millis(),
            }),
        )
        .await;
        info!(
            request_id = %request_id,
            stage = "tool",
            tool_name = %tool_name,
            duration_ms = %tool_started.elapsed().as_millis(),
            "tool execution finished"
        );
        self.hook(
            "after_tool_call",
            json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name, "status": "ok"}),
        )
        .await?;
        if is_plugin_call {
            self.hook(
                "after_plugin_call",
                json!({"request_id": request_id, "iteration": iteration, "plugin_tool": tool_name, "status": "ok"}),
            )
            .await?;
        }
        Ok(result)
    }

    async fn call_provider_with_context(
        &self,
        prompt: &str,
        request_id: &str,
        stream_sink: Option<StreamSink>,
        source_channel: Option<&str>,
    ) -> Result<String, AgentError> {
        let recent_memory = self
            .memory
            .recent_for_boundary(
                self.config.memory_window_size,
                &self.config.privacy_boundary,
                source_channel,
            )
            .await
            .map_err(|source| AgentError::Memory { source })?;
        self.audit(
            "memory_recent_loaded",
            json!({"request_id": request_id, "items": recent_memory.len()}),
        )
        .await;
        let (provider_prompt, prompt_truncated) =
            build_provider_prompt(prompt, &recent_memory, self.config.max_prompt_chars);
        if prompt_truncated {
            self.audit(
                "provider_prompt_truncated",
                json!({
                    "request_id": request_id,
                    "max_prompt_chars": self.config.max_prompt_chars,
                }),
            )
            .await;
        }
        self.hook(
            "before_provider_call",
            json!({
                "request_id": request_id,
                "prompt_len": provider_prompt.len(),
                "memory_items": recent_memory.len(),
                "prompt_truncated": prompt_truncated
            }),
        )
        .await?;
        self.audit(
            "provider_call_start",
            json!({
                "request_id": request_id,
                "prompt_len": provider_prompt.len(),
                "memory_items": recent_memory.len(),
                "prompt_truncated": prompt_truncated
            }),
        )
        .await;
        let provider_started = Instant::now();
        let provider_result = if let Some(sink) = stream_sink {
            self.provider
                .complete_streaming(&provider_prompt, sink)
                .await
        } else {
            self.provider
                .complete_with_reasoning(&provider_prompt, &self.config.reasoning)
                .await
        };
        let completion = match provider_result {
            Ok(result) => result,
            Err(source) => {
                self.observe_histogram(
                    "provider_latency_ms",
                    provider_started.elapsed().as_secs_f64() * 1000.0,
                );
                self.increment_counter("provider_errors_total");
                return Err(AgentError::Provider { source });
            }
        };
        self.observe_histogram(
            "provider_latency_ms",
            provider_started.elapsed().as_secs_f64() * 1000.0,
        );
        self.audit(
            "provider_call_success",
            json!({
                "request_id": request_id,
                "response_len": completion.output_text.len(),
                "duration_ms": provider_started.elapsed().as_millis(),
            }),
        )
        .await;
        info!(
            request_id = %request_id,
            stage = "provider",
            duration_ms = %provider_started.elapsed().as_millis(),
            "provider call finished"
        );
        self.hook(
            "after_provider_call",
            json!({"request_id": request_id, "response_len": completion.output_text.len(), "status": "ok"}),
        )
        .await?;
        Ok(completion.output_text)
    }

    async fn write_to_memory(
        &self,
        role: &str,
        content: &str,
        request_id: &str,
        source_channel: Option<&str>,
        conversation_id: &str,
    ) -> Result<(), AgentError> {
        self.hook(
            "before_memory_write",
            json!({"request_id": request_id, "role": role}),
        )
        .await?;
        self.memory
            .append(MemoryEntry {
                role: role.to_string(),
                content: content.to_string(),
                privacy_boundary: self.config.privacy_boundary.clone(),
                source_channel: source_channel.map(String::from),
                conversation_id: conversation_id.to_string(),
                created_at: None,
                expires_at: None,
                org_id: String::new(),
            })
            .await
            .map_err(|source| AgentError::Memory { source })?;
        self.hook(
            "after_memory_write",
            json!({"request_id": request_id, "role": role}),
        )
        .await?;
        self.audit(
            &format!("memory_append_{role}"),
            json!({"request_id": request_id}),
        )
        .await;
        Ok(())
    }

    fn should_research(&self, user_text: &str) -> bool {
        if !self.config.research.enabled {
            return false;
        }
        match self.config.research.trigger {
            ResearchTrigger::Never => false,
            ResearchTrigger::Always => true,
            ResearchTrigger::Keywords => {
                let lower = user_text.to_lowercase();
                self.config
                    .research
                    .keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            }
            ResearchTrigger::Length => user_text.len() >= self.config.research.min_message_length,
            ResearchTrigger::Question => user_text.trim_end().ends_with('?'),
        }
    }

    async fn run_research_phase(
        &self,
        user_text: &str,
        ctx: &ToolContext,
        request_id: &str,
    ) -> Result<String, AgentError> {
        self.audit(
            "research_phase_start",
            json!({
                "request_id": request_id,
                "max_iterations": self.config.research.max_iterations,
            }),
        )
        .await;
        self.increment_counter("research_phase_started");

        let research_prompt = format!(
            "You are in RESEARCH mode. The user asked: \"{user_text}\"\n\
             Gather relevant information using available tools. \
             Respond with tool: calls to collect data. \
             When done gathering, summarize your findings without a tool: prefix."
        );
        let mut prompt = self
            .call_provider_with_context(
                &research_prompt,
                request_id,
                None,
                ctx.source_channel.as_deref(),
            )
            .await?;
        let mut findings: Vec<String> = Vec::new();

        for iteration in 0..self.config.research.max_iterations {
            if !prompt.starts_with("tool:") {
                findings.push(prompt.clone());
                break;
            }

            let calls = parse_tool_calls(&prompt);
            if calls.is_empty() {
                break;
            }

            let (name, input) = calls[0];
            if let Some(tool) = self.tools.iter().find(|t| t.name() == name) {
                let result = self
                    .execute_tool(&**tool, name, input, ctx, request_id, iteration)
                    .await?;
                findings.push(format!("{name}: {}", result.output));

                if self.config.research.show_progress {
                    info!(iteration, tool = name, "research phase: tool executed");
                    self.audit(
                        "research_phase_iteration",
                        json!({
                            "request_id": request_id,
                            "iteration": iteration,
                            "tool_name": name,
                        }),
                    )
                    .await;
                }

                let next_prompt = format!(
                    "Research iteration {iteration}: tool `{name}` returned: {}\n\
                     Continue researching or summarize findings.",
                    result.output
                );
                prompt = self
                    .call_provider_with_context(
                        &next_prompt,
                        request_id,
                        None,
                        ctx.source_channel.as_deref(),
                    )
                    .await?;
            } else {
                break;
            }
        }

        self.audit(
            "research_phase_complete",
            json!({
                "request_id": request_id,
                "findings_count": findings.len(),
            }),
        )
        .await;
        self.increment_counter("research_phase_completed");

        Ok(findings.join("\n"))
    }

    /// Structured tool use loop: LLM decides which tools to call, agent executes
    /// them, feeds results back, and the LLM sees the results to decide next steps.
    #[instrument(
        skip(self, user_text, research_context, ctx, stream_sink),
        fields(request_id)
    )]
    async fn respond_with_tools(
        &self,
        request_id: &str,
        user_text: &str,
        research_context: &str,
        ctx: &ToolContext,
        stream_sink: Option<StreamSink>,
    ) -> Result<AssistantMessage, AgentError> {
        self.audit(
            "structured_tool_use_start",
            json!({
                "request_id": request_id,
                "max_tool_iterations": self.config.max_tool_iterations,
            }),
        )
        .await;

        let all_tool_definitions = self.build_tool_definitions();

        // Apply tool selection if a selector is configured.
        let tool_definitions = if let Some(ref selector) = self.tool_selector {
            let summaries: Vec<ToolSummary> = all_tool_definitions
                .iter()
                .map(|td| ToolSummary {
                    name: td.name.clone(),
                    description: td.description.clone(),
                })
                .collect();
            match selector.select(user_text, &summaries).await {
                Ok(selected_names) => {
                    let selected: Vec<ToolDefinition> = all_tool_definitions
                        .iter()
                        .filter(|td| selected_names.contains(&td.name))
                        .cloned()
                        .collect();
                    info!(
                        total = all_tool_definitions.len(),
                        selected = selected.len(),
                        mode = %self.config.tool_selection,
                        "tool selection applied"
                    );
                    selected
                }
                Err(e) => {
                    warn!(error = %e, "tool selection failed, falling back to all tools");
                    all_tool_definitions
                }
            }
        } else {
            all_tool_definitions
        };

        // Load recent memory and convert to conversation messages.
        let recent_memory = if let Some(ref cid) = ctx.conversation_id {
            self.memory
                .recent_for_conversation(cid, self.config.memory_window_size)
                .await
                .map_err(|source| AgentError::Memory { source })?
        } else {
            self.memory
                .recent_for_boundary(
                    self.config.memory_window_size,
                    &self.config.privacy_boundary,
                    ctx.source_channel.as_deref(),
                )
                .await
                .map_err(|source| AgentError::Memory { source })?
        };
        self.audit(
            "memory_recent_loaded",
            json!({"request_id": request_id, "items": recent_memory.len()}),
        )
        .await;

        let mut messages: Vec<ConversationMessage> = Vec::new();

        // Prepend system prompt if configured.
        if let Some(ref sp) = self.config.system_prompt {
            messages.push(ConversationMessage::System {
                content: sp.clone(),
            });
        }

        messages.extend(memory_to_messages(&recent_memory));

        if !research_context.is_empty() {
            messages.push(ConversationMessage::user(format!(
                "Research findings:\n{research_context}",
            )));
        }

        messages.push(ConversationMessage::user(user_text.to_string()));

        let mut tool_history: Vec<(String, String, String)> = Vec::new();
        let mut failure_streak: usize = 0;
        let mut loop_detector = self
            .loop_detection_config
            .as_ref()
            .map(|cfg| ToolLoopDetector::new(cfg.clone()));
        let mut restricted_tools: Vec<String> = Vec::new();

        for iteration in 0..self.config.max_tool_iterations {
            // Check cancellation between tool iterations.
            if ctx.is_cancelled() {
                warn!(request_id = %request_id, "agent execution cancelled");
                self.audit(
                    "execution_cancelled",
                    json!({"request_id": request_id, "iteration": iteration}),
                )
                .await;
                return Ok(AssistantMessage {
                    text: "[Execution cancelled]".to_string(),
                });
            }

            truncate_messages(&mut messages, self.config.max_prompt_chars);

            self.hook(
                "before_provider_call",
                json!({
                    "request_id": request_id,
                    "iteration": iteration,
                    "message_count": messages.len(),
                    "tool_count": tool_definitions.len(),
                }),
            )
            .await?;
            self.audit(
                "provider_call_start",
                json!({
                    "request_id": request_id,
                    "iteration": iteration,
                    "message_count": messages.len(),
                }),
            )
            .await;

            // Filter tool definitions if loop detection has restricted some tools.
            let effective_tools: Vec<ToolDefinition> = if restricted_tools.is_empty() {
                tool_definitions.clone()
            } else {
                tool_definitions
                    .iter()
                    .filter(|td| !restricted_tools.contains(&td.name))
                    .cloned()
                    .collect()
            };

            let provider_span = info_span!(
                "provider_call",
                request_id = %request_id,
                iteration = iteration,
                tool_count = effective_tools.len(),
            );
            let _provider_guard = provider_span.enter();
            let provider_started = Instant::now();
            let provider_result = if let Some(ref sink) = stream_sink {
                self.provider
                    .complete_streaming_with_tools(
                        &messages,
                        &effective_tools,
                        &self.config.reasoning,
                        sink.clone(),
                    )
                    .await
            } else {
                self.provider
                    .complete_with_tools(&messages, &effective_tools, &self.config.reasoning)
                    .await
            };
            let chat_result = match provider_result {
                Ok(result) => result,
                Err(source) => {
                    self.observe_histogram(
                        "provider_latency_ms",
                        provider_started.elapsed().as_secs_f64() * 1000.0,
                    );
                    self.increment_counter("provider_errors_total");
                    return Err(AgentError::Provider { source });
                }
            };
            self.observe_histogram(
                "provider_latency_ms",
                provider_started.elapsed().as_secs_f64() * 1000.0,
            );

            // Accumulate token usage from this provider call into the budget.
            let iter_tokens = chat_result.input_tokens + chat_result.output_tokens;
            if iter_tokens > 0 {
                ctx.add_tokens(iter_tokens);
            }

            // Calculate and accumulate cost from this provider call.
            if let Some(ref calc) = self.config.cost_calculator {
                let cost = calc(chat_result.input_tokens, chat_result.output_tokens);
                if cost > 0 {
                    ctx.add_cost(cost);
                }
            }

            // Check budget limits.
            if let Some(reason) = ctx.budget_exceeded() {
                warn!(
                    request_id = %request_id,
                    iteration = iteration,
                    reason = %reason,
                    "budget exceeded — force-completing run"
                );
                return Err(AgentError::BudgetExceeded { reason });
            }

            info!(
                request_id = %request_id,
                iteration = iteration,
                stop_reason = ?chat_result.stop_reason,
                tool_calls = chat_result.tool_calls.len(),
                tokens_this_call = iter_tokens,
                total_tokens = ctx.current_tokens(),
                cost_microdollars = ctx.current_cost(),
                "structured provider call finished"
            );
            self.hook(
                "after_provider_call",
                json!({
                    "request_id": request_id,
                    "iteration": iteration,
                    "response_len": chat_result.output_text.len(),
                    "tool_calls": chat_result.tool_calls.len(),
                    "status": "ok",
                }),
            )
            .await?;

            // No tool calls or EndTurn → check for text-based tool calls from
            // local models before returning the final response.
            let mut chat_result = chat_result;
            if chat_result.tool_calls.is_empty()
                || chat_result.stop_reason == Some(StopReason::EndTurn)
            {
                // Fallback: local models (llama.cpp, ollama, etc.) often emit tool
                // calls as JSON text instead of structured tool_calls. Try to extract
                // and dispatch them so the agent loop continues.
                if chat_result.tool_calls.is_empty() && !chat_result.output_text.is_empty() {
                    if let Some(extracted) =
                        extract_tool_call_from_text(&chat_result.output_text, &effective_tools)
                    {
                        info!(
                            request_id = %request_id,
                            tool = %extracted.name,
                            "extracted tool call from text output (local model fallback)"
                        );
                        chat_result.tool_calls = vec![extracted];
                        chat_result.stop_reason = Some(StopReason::ToolUse);
                        // Fall through to tool dispatch below instead of returning.
                    }
                }

                // If still no tool calls after extraction attempt, return text.
                if chat_result.tool_calls.is_empty() {
                    let response_text = chat_result.output_text;
                    self.write_to_memory(
                        "assistant",
                        &response_text,
                        request_id,
                        ctx.source_channel.as_deref(),
                        ctx.conversation_id.as_deref().unwrap_or(""),
                    )
                    .await?;
                    self.audit("respond_success", json!({"request_id": request_id}))
                        .await;
                    self.hook(
                        "before_response_emit",
                        json!({"request_id": request_id, "response_len": response_text.len()}),
                    )
                    .await?;
                    let response = AssistantMessage {
                        text: response_text,
                    };
                    self.hook(
                        "after_response_emit",
                        json!({"request_id": request_id, "response_len": response.text.len()}),
                    )
                    .await?;
                    return Ok(response);
                }
            }

            // Record assistant message with tool calls in conversation history.
            messages.push(ConversationMessage::Assistant {
                content: if chat_result.output_text.is_empty() {
                    None
                } else {
                    Some(chat_result.output_text.clone())
                },
                tool_calls: chat_result.tool_calls.clone(),
            });

            // Execute tool calls.
            let tool_calls = &chat_result.tool_calls;
            let has_gated = tool_calls
                .iter()
                .any(|tc| self.config.gated_tools.contains(&tc.name));
            let use_parallel = self.config.parallel_tools && tool_calls.len() > 1 && !has_gated;

            let mut tool_results: Vec<ToolResultMessage> = Vec::new();

            if use_parallel {
                let futs: Vec<_> = tool_calls
                    .iter()
                    .map(|tc| {
                        let tool = self.tools.iter().find(|t| t.name() == tc.name);
                        async move {
                            match tool {
                                Some(tool) => {
                                    let input_str = match prepare_tool_input(&**tool, &tc.input) {
                                        Ok(s) => s,
                                        Err(validation_err) => {
                                            return (
                                                tc.name.clone(),
                                                String::new(),
                                                ToolResultMessage {
                                                    tool_use_id: tc.id.clone(),
                                                    content: validation_err,
                                                    is_error: true,
                                                },
                                            );
                                        }
                                    };
                                    match tool.execute(&input_str, ctx).await {
                                        Ok(result) => (
                                            tc.name.clone(),
                                            input_str,
                                            ToolResultMessage {
                                                tool_use_id: tc.id.clone(),
                                                content: result.output,
                                                is_error: false,
                                            },
                                        ),
                                        Err(e) => (
                                            tc.name.clone(),
                                            input_str,
                                            ToolResultMessage {
                                                tool_use_id: tc.id.clone(),
                                                content: format!("Error: {e}"),
                                                is_error: true,
                                            },
                                        ),
                                    }
                                }
                                None => (
                                    tc.name.clone(),
                                    String::new(),
                                    ToolResultMessage {
                                        tool_use_id: tc.id.clone(),
                                        content: format!("Tool '{}' not found", tc.name),
                                        is_error: true,
                                    },
                                ),
                            }
                        }
                    })
                    .collect();
                let results = futures_util::future::join_all(futs).await;
                for (name, input_str, result_msg) in results {
                    if result_msg.is_error {
                        failure_streak += 1;
                    } else {
                        failure_streak = 0;
                        tool_history.push((name, input_str, result_msg.content.clone()));
                    }
                    tool_results.push(result_msg);
                }
            } else {
                // Sequential execution.
                for tc in tool_calls {
                    let result_msg = match self.tools.iter().find(|t| t.name() == tc.name) {
                        Some(tool) => {
                            let input_str = match prepare_tool_input(&**tool, &tc.input) {
                                Ok(s) => s,
                                Err(validation_err) => {
                                    failure_streak += 1;
                                    tool_results.push(ToolResultMessage {
                                        tool_use_id: tc.id.clone(),
                                        content: validation_err,
                                        is_error: true,
                                    });
                                    continue;
                                }
                            };
                            self.audit(
                                "tool_requested",
                                json!({
                                    "request_id": request_id,
                                    "iteration": iteration,
                                    "tool_name": tc.name,
                                    "tool_input_len": input_str.len(),
                                }),
                            )
                            .await;

                            match self
                                .execute_tool(
                                    &**tool, &tc.name, &input_str, ctx, request_id, iteration,
                                )
                                .await
                            {
                                Ok(result) => {
                                    failure_streak = 0;
                                    tool_history.push((
                                        tc.name.clone(),
                                        input_str,
                                        result.output.clone(),
                                    ));
                                    ToolResultMessage {
                                        tool_use_id: tc.id.clone(),
                                        content: result.output,
                                        is_error: false,
                                    }
                                }
                                Err(e) => {
                                    failure_streak += 1;
                                    ToolResultMessage {
                                        tool_use_id: tc.id.clone(),
                                        content: format!("Error: {e}"),
                                        is_error: true,
                                    }
                                }
                            }
                        }
                        None => {
                            self.audit(
                                "tool_not_found",
                                json!({
                                    "request_id": request_id,
                                    "iteration": iteration,
                                    "tool_name": tc.name,
                                }),
                            )
                            .await;
                            failure_streak += 1;
                            ToolResultMessage {
                                tool_use_id: tc.id.clone(),
                                content: format!(
                                    "Tool '{}' not found. Available tools: {}",
                                    tc.name,
                                    tool_definitions
                                        .iter()
                                        .map(|d| d.name.as_str())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ),
                                is_error: true,
                            }
                        }
                    };
                    tool_results.push(result_msg);
                }
            }

            // Append tool results to conversation.
            for result in &tool_results {
                messages.push(ConversationMessage::ToolResult(result.clone()));
            }

            // Tiered loop detection (similarity + cost runaway).
            if let Some(ref mut detector) = loop_detector {
                // Check each tool call from this iteration.
                let tool_calls_this_iter = &chat_result.tool_calls;
                let mut worst_action = LoopAction::Continue;
                for tc in tool_calls_this_iter {
                    let action = detector.check(
                        &tc.name,
                        &tc.input,
                        ctx.current_tokens(),
                        ctx.current_cost(),
                    );
                    if crate::loop_detection::severity(&action)
                        > crate::loop_detection::severity(&worst_action)
                    {
                        worst_action = action;
                    }
                }

                match worst_action {
                    LoopAction::Continue => {}
                    LoopAction::InjectMessage(ref msg) => {
                        warn!(
                            request_id = %request_id,
                            "tiered loop detection: injecting message"
                        );
                        self.audit(
                            "loop_detection_inject",
                            json!({
                                "request_id": request_id,
                                "iteration": iteration,
                                "message": msg,
                            }),
                        )
                        .await;
                        messages.push(ConversationMessage::user(format!("SYSTEM NOTICE: {msg}")));
                    }
                    LoopAction::RestrictTools(ref tools) => {
                        warn!(
                            request_id = %request_id,
                            tools = ?tools,
                            "tiered loop detection: restricting tools"
                        );
                        self.audit(
                            "loop_detection_restrict",
                            json!({
                                "request_id": request_id,
                                "iteration": iteration,
                                "restricted_tools": tools,
                            }),
                        )
                        .await;
                        restricted_tools.extend(tools.iter().cloned());
                        messages.push(ConversationMessage::user(format!(
                            "SYSTEM NOTICE: The following tools have been temporarily \
                             restricted due to repetitive usage: {}. Try a different approach.",
                            tools.join(", ")
                        )));
                    }
                    LoopAction::ForceComplete(ref reason) => {
                        warn!(
                            request_id = %request_id,
                            reason = %reason,
                            "tiered loop detection: force completing"
                        );
                        self.audit(
                            "loop_detection_force_complete",
                            json!({
                                "request_id": request_id,
                                "iteration": iteration,
                                "reason": reason,
                            }),
                        )
                        .await;
                        // Force a final response without tools.
                        messages.push(ConversationMessage::user(format!(
                            "SYSTEM NOTICE: {reason}. Provide your best answer now \
                             without calling any more tools."
                        )));
                        truncate_messages(&mut messages, self.config.max_prompt_chars);
                        let final_result = if let Some(ref sink) = stream_sink {
                            self.provider
                                .complete_streaming_with_tools(
                                    &messages,
                                    &[],
                                    &self.config.reasoning,
                                    sink.clone(),
                                )
                                .await
                                .map_err(|source| AgentError::Provider { source })?
                        } else {
                            self.provider
                                .complete_with_tools(&messages, &[], &self.config.reasoning)
                                .await
                                .map_err(|source| AgentError::Provider { source })?
                        };
                        let response_text = final_result.output_text;
                        self.write_to_memory(
                            "assistant",
                            &response_text,
                            request_id,
                            ctx.source_channel.as_deref(),
                            ctx.conversation_id.as_deref().unwrap_or(""),
                        )
                        .await?;
                        return Ok(AssistantMessage {
                            text: response_text,
                        });
                    }
                }
            }

            // Loop detection: no-progress.
            if self.config.loop_detection_no_progress_threshold > 0 {
                let threshold = self.config.loop_detection_no_progress_threshold;
                if tool_history.len() >= threshold {
                    let recent = &tool_history[tool_history.len() - threshold..];
                    let all_same = recent.iter().all(|entry| {
                        entry.0 == recent[0].0 && entry.1 == recent[0].1 && entry.2 == recent[0].2
                    });
                    if all_same {
                        warn!(
                            tool_name = recent[0].0,
                            threshold, "structured loop detection: no-progress threshold reached"
                        );
                        self.audit(
                            "loop_detection_no_progress",
                            json!({
                                "request_id": request_id,
                                "tool_name": recent[0].0,
                                "threshold": threshold,
                            }),
                        )
                        .await;
                        messages.push(ConversationMessage::user(format!(
                                "SYSTEM NOTICE: Loop detected — the tool `{}` was called \
                                 {} times with identical arguments and output. You are stuck \
                                 in a loop. Stop calling this tool and try a different approach, \
                                 or provide your best answer with the information you already have.",
                                recent[0].0, threshold
                        )));
                    }
                }
            }

            // Loop detection: ping-pong.
            if self.config.loop_detection_ping_pong_cycles > 0 {
                let cycles = self.config.loop_detection_ping_pong_cycles;
                let needed = cycles * 2;
                if tool_history.len() >= needed {
                    let recent = &tool_history[tool_history.len() - needed..];
                    let is_ping_pong = (0..needed).all(|i| recent[i].0 == recent[i % 2].0)
                        && recent[0].0 != recent[1].0;
                    if is_ping_pong {
                        warn!(
                            tool_a = recent[0].0,
                            tool_b = recent[1].0,
                            cycles,
                            "structured loop detection: ping-pong pattern detected"
                        );
                        self.audit(
                            "loop_detection_ping_pong",
                            json!({
                                "request_id": request_id,
                                "tool_a": recent[0].0,
                                "tool_b": recent[1].0,
                                "cycles": cycles,
                            }),
                        )
                        .await;
                        messages.push(ConversationMessage::user(format!(
                            "SYSTEM NOTICE: Loop detected — tools `{}` and `{}` have been \
                                 alternating for {} cycles in a ping-pong pattern. Stop this \
                                 alternation and try a different approach, or provide your best \
                                 answer with the information you already have.",
                            recent[0].0, recent[1].0, cycles
                        )));
                    }
                }
            }

            // Failure streak detection.
            if self.config.loop_detection_failure_streak > 0
                && failure_streak >= self.config.loop_detection_failure_streak
            {
                warn!(
                    failure_streak,
                    "structured loop detection: consecutive failure streak"
                );
                self.audit(
                    "loop_detection_failure_streak",
                    json!({
                        "request_id": request_id,
                        "streak": failure_streak,
                    }),
                )
                .await;
                messages.push(ConversationMessage::user(format!(
                    "SYSTEM NOTICE: {} consecutive tool calls have failed. \
                         Stop calling tools and provide your best answer with \
                         the information you already have.",
                    failure_streak
                )));
            }
        }

        // Max iterations reached — one final call without tools.
        self.audit(
            "structured_tool_use_max_iterations",
            json!({
                "request_id": request_id,
                "max_tool_iterations": self.config.max_tool_iterations,
            }),
        )
        .await;

        messages.push(ConversationMessage::user(
            "You have reached the maximum number of tool call iterations. \
                      Please provide your best answer now without calling any more tools."
                .to_string(),
        ));
        truncate_messages(&mut messages, self.config.max_prompt_chars);

        let final_result = if let Some(ref sink) = stream_sink {
            self.provider
                .complete_streaming_with_tools(&messages, &[], &self.config.reasoning, sink.clone())
                .await
                .map_err(|source| AgentError::Provider { source })?
        } else {
            self.provider
                .complete_with_tools(&messages, &[], &self.config.reasoning)
                .await
                .map_err(|source| AgentError::Provider { source })?
        };

        let response_text = final_result.output_text;
        self.write_to_memory(
            "assistant",
            &response_text,
            request_id,
            ctx.source_channel.as_deref(),
            ctx.conversation_id.as_deref().unwrap_or(""),
        )
        .await?;
        self.audit("respond_success", json!({"request_id": request_id}))
            .await;
        self.hook(
            "before_response_emit",
            json!({"request_id": request_id, "response_len": response_text.len()}),
        )
        .await?;
        let response = AssistantMessage {
            text: response_text,
        };
        self.hook(
            "after_response_emit",
            json!({"request_id": request_id, "response_len": response.text.len()}),
        )
        .await?;
        Ok(response)
    }

    pub async fn respond(
        &self,
        user: UserMessage,
        ctx: &ToolContext,
    ) -> Result<AssistantMessage, AgentError> {
        let request_id = Self::next_request_id();
        let span = info_span!(
            "agent_run",
            request_id = %request_id,
            depth = ctx.depth,
            conversation_id = ctx.conversation_id.as_deref().unwrap_or(""),
        );
        self.respond_traced(&request_id, user, ctx, None)
            .instrument(span)
            .await
    }

    /// Streaming variant of `respond()`. Sends incremental `StreamChunk`s through
    /// `sink` as tokens arrive from the provider. Returns the final accumulated
    /// `AssistantMessage` once the stream completes.
    pub async fn respond_streaming(
        &self,
        user: UserMessage,
        ctx: &ToolContext,
        sink: StreamSink,
    ) -> Result<AssistantMessage, AgentError> {
        let request_id = Self::next_request_id();
        let span = info_span!(
            "agent_run",
            request_id = %request_id,
            depth = ctx.depth,
            conversation_id = ctx.conversation_id.as_deref().unwrap_or(""),
            streaming = true,
        );
        self.respond_traced(&request_id, user, ctx, Some(sink))
            .instrument(span)
            .await
    }

    /// Shared traced implementation for `respond()` and `respond_streaming()`.
    async fn respond_traced(
        &self,
        request_id: &str,
        user: UserMessage,
        ctx: &ToolContext,
        stream_sink: Option<StreamSink>,
    ) -> Result<AssistantMessage, AgentError> {
        self.increment_counter("requests_total");
        let run_started = Instant::now();
        self.hook("before_run", json!({"request_id": request_id}))
            .await?;
        let timed = timeout(
            Duration::from_millis(self.config.request_timeout_ms),
            self.respond_inner(request_id, user, ctx, stream_sink),
        )
        .await;
        let result = match timed {
            Ok(result) => result,
            Err(_) => Err(AgentError::Timeout {
                timeout_ms: self.config.request_timeout_ms,
            }),
        };

        let after_detail = match &result {
            Ok(response) => json!({
                "request_id": request_id,
                "status": "ok",
                "response_len": response.text.len(),
                "duration_ms": run_started.elapsed().as_millis(),
            }),
            Err(err) => json!({
                "request_id": request_id,
                "status": "error",
                "error": redact_text(&err.to_string()),
                "duration_ms": run_started.elapsed().as_millis(),
            }),
        };
        let total_cost = ctx.current_cost();
        let cost_usd = total_cost as f64 / 1_000_000.0;
        info!(
            request_id = %request_id,
            duration_ms = %run_started.elapsed().as_millis(),
            total_tokens = ctx.current_tokens(),
            cost_microdollars = total_cost,
            cost_usd = format!("{:.4}", cost_usd),
            "agent run completed"
        );
        self.hook("after_run", after_detail).await?;
        result
    }

    async fn respond_inner(
        &self,
        request_id: &str,
        user: UserMessage,
        ctx: &ToolContext,
        stream_sink: Option<StreamSink>,
    ) -> Result<AssistantMessage, AgentError> {
        self.audit(
            "respond_start",
            json!({
                "request_id": request_id,
                "user_message_len": user.text.len(),
                "max_tool_iterations": self.config.max_tool_iterations,
                "request_timeout_ms": self.config.request_timeout_ms,
            }),
        )
        .await;
        self.write_to_memory(
            "user",
            &user.text,
            request_id,
            ctx.source_channel.as_deref(),
            ctx.conversation_id.as_deref().unwrap_or(""),
        )
        .await?;

        let research_context = if self.should_research(&user.text) {
            self.run_research_phase(&user.text, ctx, request_id).await?
        } else {
            String::new()
        };

        // Use structured tool dispatch when the model supports it and tools have schemas.
        if self.config.model_supports_tool_use && self.has_tool_definitions() {
            return self
                .respond_with_tools(request_id, &user.text, &research_context, ctx, stream_sink)
                .await;
        }

        let mut prompt = user.text;
        let mut tool_history: Vec<(String, String, String)> = Vec::new();

        for iteration in 0..self.config.max_tool_iterations {
            if !prompt.starts_with("tool:") {
                break;
            }

            let calls = parse_tool_calls(&prompt);
            if calls.is_empty() {
                break;
            }

            // C3: Parallel tool execution when enabled and multiple calls present.
            // Falls back to sequential if any tool in the batch is gated
            // (requires approval), preserving the interactive approval flow.
            let has_gated = calls
                .iter()
                .any(|(name, _)| self.config.gated_tools.contains(*name));
            if calls.len() > 1 && self.config.parallel_tools && !has_gated {
                let mut resolved: Vec<(&str, &str, &dyn Tool)> = Vec::new();
                for &(name, input) in &calls {
                    let tool = self.tools.iter().find(|t| t.name() == name);
                    match tool {
                        Some(t) => resolved.push((name, input, &**t)),
                        None => {
                            self.audit(
                                "tool_not_found",
                                json!({"request_id": request_id, "iteration": iteration, "tool_name": name}),
                            )
                            .await;
                        }
                    }
                }
                if resolved.is_empty() {
                    break;
                }

                let futs: Vec<_> = resolved
                    .iter()
                    .map(|&(name, input, tool)| async move {
                        let r = tool.execute(input, ctx).await;
                        (name, input, r)
                    })
                    .collect();
                let results = futures_util::future::join_all(futs).await;

                let mut output_parts: Vec<String> = Vec::new();
                for (name, input, result) in results {
                    match result {
                        Ok(r) => {
                            tool_history.push((
                                name.to_string(),
                                input.to_string(),
                                r.output.clone(),
                            ));
                            output_parts.push(format!("Tool output from {name}: {}", r.output));
                        }
                        Err(source) => {
                            return Err(AgentError::Tool {
                                tool: name.to_string(),
                                source,
                            });
                        }
                    }
                }
                prompt = output_parts.join("\n");
                continue;
            }

            // Sequential: process the first call only.
            let (tool_name, tool_input) = calls[0];
            self.audit(
                "tool_requested",
                json!({
                    "request_id": request_id,
                    "iteration": iteration,
                    "tool_name": tool_name,
                    "tool_input_len": tool_input.len(),
                }),
            )
            .await;

            if let Some(tool) = self.tools.iter().find(|t| t.name() == tool_name) {
                let result = self
                    .execute_tool(&**tool, tool_name, tool_input, ctx, request_id, iteration)
                    .await?;

                tool_history.push((
                    tool_name.to_string(),
                    tool_input.to_string(),
                    result.output.clone(),
                ));

                // C1: No-progress detection — same tool+args+output N times.
                if self.config.loop_detection_no_progress_threshold > 0 {
                    let threshold = self.config.loop_detection_no_progress_threshold;
                    if tool_history.len() >= threshold {
                        let recent = &tool_history[tool_history.len() - threshold..];
                        let all_same = recent.iter().all(|entry| {
                            entry.0 == recent[0].0
                                && entry.1 == recent[0].1
                                && entry.2 == recent[0].2
                        });
                        if all_same {
                            warn!(
                                tool_name,
                                threshold, "loop detection: no-progress threshold reached"
                            );
                            self.audit(
                                "loop_detection_no_progress",
                                json!({
                                    "request_id": request_id,
                                    "tool_name": tool_name,
                                    "threshold": threshold,
                                }),
                            )
                            .await;
                            prompt = format!(
                                "SYSTEM NOTICE: Loop detected — the tool `{tool_name}` was called \
                                 {threshold} times with identical arguments and output. You are stuck \
                                 in a loop. Stop calling this tool and try a different approach, or \
                                 provide your best answer with the information you already have."
                            );
                            break;
                        }
                    }
                }

                // C1: Ping-pong detection — A→B→A→B alternation.
                if self.config.loop_detection_ping_pong_cycles > 0 {
                    let cycles = self.config.loop_detection_ping_pong_cycles;
                    let needed = cycles * 2;
                    if tool_history.len() >= needed {
                        let recent = &tool_history[tool_history.len() - needed..];
                        let is_ping_pong = (0..needed).all(|i| recent[i].0 == recent[i % 2].0)
                            && recent[0].0 != recent[1].0;
                        if is_ping_pong {
                            warn!(
                                tool_a = recent[0].0,
                                tool_b = recent[1].0,
                                cycles,
                                "loop detection: ping-pong pattern detected"
                            );
                            self.audit(
                                "loop_detection_ping_pong",
                                json!({
                                    "request_id": request_id,
                                    "tool_a": recent[0].0,
                                    "tool_b": recent[1].0,
                                    "cycles": cycles,
                                }),
                            )
                            .await;
                            prompt = format!(
                                "SYSTEM NOTICE: Loop detected — tools `{}` and `{}` have been \
                                 alternating for {} cycles in a ping-pong pattern. Stop this \
                                 alternation and try a different approach, or provide your best \
                                 answer with the information you already have.",
                                recent[0].0, recent[1].0, cycles
                            );
                            break;
                        }
                    }
                }

                if result.output.starts_with("tool:") {
                    prompt = result.output.clone();
                } else {
                    prompt = format!("Tool output from {tool_name}: {}", result.output);
                }
                continue;
            }

            self.audit(
                "tool_not_found",
                json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name}),
            )
            .await;
            break;
        }

        if !research_context.is_empty() {
            prompt = format!("Research findings:\n{research_context}\n\nUser request:\n{prompt}");
        }

        let response_text = self
            .call_provider_with_context(
                &prompt,
                request_id,
                stream_sink,
                ctx.source_channel.as_deref(),
            )
            .await?;
        self.write_to_memory(
            "assistant",
            &response_text,
            request_id,
            ctx.source_channel.as_deref(),
            ctx.conversation_id.as_deref().unwrap_or(""),
        )
        .await?;
        self.audit("respond_success", json!({"request_id": request_id}))
            .await;

        self.hook(
            "before_response_emit",
            json!({"request_id": request_id, "response_len": response_text.len()}),
        )
        .await?;
        let response = AssistantMessage {
            text: response_text,
        };
        self.hook(
            "after_response_emit",
            json!({"request_id": request_id, "response_len": response.text.len()}),
        )
        .await?;

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        ChatResult, HookEvent, HookFailureMode, HookPolicy, ReasoningConfig, ResearchPolicy,
        ResearchTrigger, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::time::{sleep, Duration};

    #[derive(Default)]
    struct TestMemory {
        entries: Arc<Mutex<Vec<MemoryEntry>>>,
    }

    #[async_trait]
    impl MemoryStore for TestMemory {
        async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
            self.entries
                .lock()
                .expect("memory lock poisoned")
                .push(entry);
            Ok(())
        }

        async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
            let entries = self.entries.lock().expect("memory lock poisoned");
            Ok(entries.iter().rev().take(limit).cloned().collect())
        }
    }

    struct TestProvider {
        received_prompts: Arc<Mutex<Vec<String>>>,
        response_text: String,
    }

    #[async_trait]
    impl Provider for TestProvider {
        async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
            self.received_prompts
                .lock()
                .expect("provider lock poisoned")
                .push(prompt.to_string());
            Ok(ChatResult {
                output_text: self.response_text.clone(),
                ..Default::default()
            })
        }
    }

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }

        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: format!("echoed:{input}"),
            })
        }
    }

    struct PluginEchoTool;

    #[async_trait]
    impl Tool for PluginEchoTool {
        fn name(&self) -> &'static str {
            "plugin:echo"
        }

        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: format!("plugin-echoed:{input}"),
            })
        }
    }

    struct FailingTool;

    #[async_trait]
    impl Tool for FailingTool {
        fn name(&self) -> &'static str {
            "boom"
        }

        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Err(anyhow::anyhow!("tool exploded"))
        }
    }

    struct SlowTool;

    #[async_trait]
    impl Tool for SlowTool {
        fn name(&self) -> &'static str {
            "slow"
        }

        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            sleep(Duration::from_millis(500)).await;
            Ok(ToolResult {
                output: "finally done".to_string(),
            })
        }
    }

    struct FailingProvider;

    #[async_trait]
    impl Provider for FailingProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Err(anyhow::anyhow!("provider boom"))
        }
    }

    struct SlowProvider;

    #[async_trait]
    impl Provider for SlowProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            sleep(Duration::from_millis(500)).await;
            Ok(ChatResult {
                output_text: "late".to_string(),
                ..Default::default()
            })
        }
    }

    struct ScriptedProvider {
        responses: Vec<String>,
        call_count: AtomicUsize,
        received_prompts: Arc<Mutex<Vec<String>>>,
    }

    impl ScriptedProvider {
        fn new(responses: Vec<&str>) -> Self {
            Self {
                responses: responses.into_iter().map(|s| s.to_string()).collect(),
                call_count: AtomicUsize::new(0),
                received_prompts: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Provider for ScriptedProvider {
        async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
            self.received_prompts
                .lock()
                .expect("provider lock poisoned")
                .push(prompt.to_string());
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let response = if idx < self.responses.len() {
                self.responses[idx].clone()
            } else {
                self.responses.last().cloned().unwrap_or_default()
            };
            Ok(ChatResult {
                output_text: response,
                ..Default::default()
            })
        }
    }

    struct UpperTool;

    #[async_trait]
    impl Tool for UpperTool {
        fn name(&self) -> &'static str {
            "upper"
        }

        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: input.to_uppercase(),
            })
        }
    }

    /// Tool that always returns a `tool:` directive pointing back to itself,
    /// creating an infinite self-referencing loop (caught by no-progress detection).
    struct LoopTool;

    #[async_trait]
    impl Tool for LoopTool {
        fn name(&self) -> &'static str {
            "loop_tool"
        }

        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "tool:loop_tool x".to_string(),
            })
        }
    }

    /// Tool that chains to PongTool (for ping-pong detection tests).
    struct PingTool;

    #[async_trait]
    impl Tool for PingTool {
        fn name(&self) -> &'static str {
            "ping"
        }

        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "tool:pong x".to_string(),
            })
        }
    }

    /// Tool that chains to PingTool (for ping-pong detection tests).
    struct PongTool;

    #[async_trait]
    impl Tool for PongTool {
        fn name(&self) -> &'static str {
            "pong"
        }

        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "tool:ping x".to_string(),
            })
        }
    }

    struct RecordingHookSink {
        events: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl HookSink for RecordingHookSink {
        async fn record(&self, event: HookEvent) -> anyhow::Result<()> {
            self.events
                .lock()
                .expect("hook lock poisoned")
                .push(event.stage);
            Ok(())
        }
    }

    struct FailingHookSink;

    #[async_trait]
    impl HookSink for FailingHookSink {
        async fn record(&self, _event: HookEvent) -> anyhow::Result<()> {
            Err(anyhow::anyhow!("hook sink failure"))
        }
    }

    struct RecordingAuditSink {
        events: Arc<Mutex<Vec<AuditEvent>>>,
    }

    #[async_trait]
    impl AuditSink for RecordingAuditSink {
        async fn record(&self, event: AuditEvent) -> anyhow::Result<()> {
            self.events.lock().expect("audit lock poisoned").push(event);
            Ok(())
        }
    }

    struct RecordingMetricsSink {
        counters: Arc<Mutex<HashMap<&'static str, u64>>>,
        histograms: Arc<Mutex<HashMap<&'static str, usize>>>,
    }

    impl MetricsSink for RecordingMetricsSink {
        fn increment_counter(&self, name: &'static str, value: u64) {
            let mut counters = self.counters.lock().expect("metrics lock poisoned");
            *counters.entry(name).or_insert(0) += value;
        }

        fn observe_histogram(&self, name: &'static str, _value: f64) {
            let mut histograms = self.histograms.lock().expect("metrics lock poisoned");
            *histograms.entry(name).or_insert(0) += 1;
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext::new(".".to_string())
    }

    fn counter(counters: &Arc<Mutex<HashMap<&'static str, u64>>>, name: &'static str) -> u64 {
        counters
            .lock()
            .expect("metrics lock poisoned")
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    fn histogram_count(
        histograms: &Arc<Mutex<HashMap<&'static str, usize>>>,
        name: &'static str,
    ) -> usize {
        histograms
            .lock()
            .expect("metrics lock poisoned")
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    #[tokio::test]
    async fn respond_appends_user_then_assistant_memory_entries() {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory {
            entries: entries.clone(),
        };
        let provider = TestProvider {
            received_prompts: prompts,
            response_text: "assistant-output".to_string(),
        };
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hello world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("agent respond should succeed");

        assert_eq!(response.text, "assistant-output");
        let stored = entries.lock().expect("memory lock poisoned");
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].role, "user");
        assert_eq!(stored[0].content, "hello world");
        assert_eq!(stored[1].role, "assistant");
        assert_eq!(stored[1].content, "assistant-output");
    }

    #[tokio::test]
    async fn respond_invokes_tool_for_tool_prefixed_prompt() {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory {
            entries: entries.clone(),
        };
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "assistant-after-tool".to_string(),
        };
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("agent respond should succeed");

        assert_eq!(response.text, "assistant-after-tool");
        let prompts = prompts.lock().expect("provider lock poisoned");
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Recent memory:"));
        assert!(prompts[0].contains("Current input:\nTool output from echo: echoed:ping"));
    }

    #[tokio::test]
    async fn respond_with_unknown_tool_falls_back_to_provider_prompt() {
        let entries = Arc::new(Mutex::new(Vec::new()));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory {
            entries: entries.clone(),
        };
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "assistant-without-tool".to_string(),
        };
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:unknown payload".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("agent respond should succeed");

        assert_eq!(response.text, "assistant-without-tool");
        let prompts = prompts.lock().expect("provider lock poisoned");
        assert_eq!(prompts.len(), 1);
        assert!(prompts[0].contains("Recent memory:"));
        assert!(prompts[0].contains("Current input:\ntool:unknown payload"));
    }

    #[tokio::test]
    async fn respond_includes_bounded_recent_memory_in_provider_prompt() {
        let entries = Arc::new(Mutex::new(vec![
            MemoryEntry {
                role: "assistant".to_string(),
                content: "very-old".to_string(),
                ..Default::default()
            },
            MemoryEntry {
                role: "user".to_string(),
                content: "recent-before-request".to_string(),
                ..Default::default()
            },
        ]));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory {
            entries: entries.clone(),
        };
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                memory_window_size: 2,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![],
        );

        agent
            .respond(
                UserMessage {
                    text: "latest-user".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let prompts = prompts.lock().expect("provider lock poisoned");
        let provider_prompt = prompts.first().expect("provider prompt should exist");
        assert!(provider_prompt.contains("- user: recent-before-request"));
        assert!(provider_prompt.contains("- user: latest-user"));
        assert!(!provider_prompt.contains("very-old"));
    }

    #[tokio::test]
    async fn respond_caps_provider_prompt_to_configured_max_chars() {
        let entries = Arc::new(Mutex::new(vec![MemoryEntry {
            role: "assistant".to_string(),
            content: "historic context ".repeat(16),
            ..Default::default()
        }]));
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory {
            entries: entries.clone(),
        };
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                max_prompt_chars: 64,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![],
        );

        agent
            .respond(
                UserMessage {
                    text: "final-tail-marker".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let prompts = prompts.lock().expect("provider lock poisoned");
        let provider_prompt = prompts.first().expect("provider prompt should exist");
        assert!(provider_prompt.chars().count() <= 64);
        assert!(provider_prompt.contains("final-tail-marker"));
    }

    #[tokio::test]
    async fn respond_returns_provider_typed_error() {
        let memory = TestMemory::default();
        let counters = Arc::new(Mutex::new(HashMap::new()));
        let histograms = Arc::new(Mutex::new(HashMap::new()));
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(FailingProvider),
            Box::new(memory),
            vec![],
        )
        .with_metrics(Box::new(RecordingMetricsSink {
            counters: counters.clone(),
            histograms: histograms.clone(),
        }));

        let result = agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await;

        match result {
            Err(AgentError::Provider { source }) => {
                assert!(source.to_string().contains("provider boom"));
            }
            other => panic!("expected provider error, got {other:?}"),
        }
        assert_eq!(counter(&counters, "requests_total"), 1);
        assert_eq!(counter(&counters, "provider_errors_total"), 1);
        assert_eq!(counter(&counters, "tool_errors_total"), 0);
        assert_eq!(histogram_count(&histograms, "provider_latency_ms"), 1);
    }

    #[tokio::test]
    async fn respond_increments_tool_error_counter_on_tool_failure() {
        let memory = TestMemory::default();
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts,
            response_text: "unused".to_string(),
        };
        let counters = Arc::new(Mutex::new(HashMap::new()));
        let histograms = Arc::new(Mutex::new(HashMap::new()));
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(FailingTool)],
        )
        .with_metrics(Box::new(RecordingMetricsSink {
            counters: counters.clone(),
            histograms: histograms.clone(),
        }));

        let result = agent
            .respond(
                UserMessage {
                    text: "tool:boom ping".to_string(),
                },
                &test_ctx(),
            )
            .await;

        match result {
            Err(AgentError::Tool { tool, source }) => {
                assert_eq!(tool, "boom");
                assert!(source.to_string().contains("tool exploded"));
            }
            other => panic!("expected tool error, got {other:?}"),
        }
        assert_eq!(counter(&counters, "requests_total"), 1);
        assert_eq!(counter(&counters, "provider_errors_total"), 0);
        assert_eq!(counter(&counters, "tool_errors_total"), 1);
        assert_eq!(histogram_count(&histograms, "tool_latency_ms"), 1);
    }

    #[tokio::test]
    async fn respond_increments_requests_counter_on_success() {
        let memory = TestMemory::default();
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts,
            response_text: "ok".to_string(),
        };
        let counters = Arc::new(Mutex::new(HashMap::new()));
        let histograms = Arc::new(Mutex::new(HashMap::new()));
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![],
        )
        .with_metrics(Box::new(RecordingMetricsSink {
            counters: counters.clone(),
            histograms: histograms.clone(),
        }));

        let response = agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("response should succeed");

        assert_eq!(response.text, "ok");
        assert_eq!(counter(&counters, "requests_total"), 1);
        assert_eq!(counter(&counters, "provider_errors_total"), 0);
        assert_eq!(counter(&counters, "tool_errors_total"), 0);
        assert_eq!(histogram_count(&histograms, "provider_latency_ms"), 1);
    }

    #[tokio::test]
    async fn respond_returns_timeout_typed_error() {
        let memory = TestMemory::default();
        let agent = Agent::new(
            AgentConfig {
                max_tool_iterations: 1,
                request_timeout_ms: 10,
                ..AgentConfig::default()
            },
            Box::new(SlowProvider),
            Box::new(memory),
            vec![],
        );

        let result = agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await;

        match result {
            Err(AgentError::Timeout { timeout_ms }) => assert_eq!(timeout_ms, 10),
            other => panic!("expected timeout error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn respond_emits_before_after_hook_events_when_enabled() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory::default();
        let provider = TestProvider {
            received_prompts: Arc::new(Mutex::new(Vec::new())),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                hooks: HookPolicy {
                    enabled: true,
                    timeout_ms: 50,
                    fail_closed: true,
                    ..HookPolicy::default()
                },
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool), Box::new(PluginEchoTool)],
        )
        .with_hooks(Box::new(RecordingHookSink {
            events: events.clone(),
        }));

        agent
            .respond(
                UserMessage {
                    text: "tool:plugin:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let stages = events.lock().expect("hook lock poisoned");
        assert!(stages.contains(&"before_run".to_string()));
        assert!(stages.contains(&"after_run".to_string()));
        assert!(stages.contains(&"before_tool_call".to_string()));
        assert!(stages.contains(&"after_tool_call".to_string()));
        assert!(stages.contains(&"before_plugin_call".to_string()));
        assert!(stages.contains(&"after_plugin_call".to_string()));
        assert!(stages.contains(&"before_provider_call".to_string()));
        assert!(stages.contains(&"after_provider_call".to_string()));
        assert!(stages.contains(&"before_memory_write".to_string()));
        assert!(stages.contains(&"after_memory_write".to_string()));
        assert!(stages.contains(&"before_response_emit".to_string()));
        assert!(stages.contains(&"after_response_emit".to_string()));
    }

    #[tokio::test]
    async fn respond_audit_events_include_request_id_and_durations() {
        let audit_events = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory::default();
        let provider = TestProvider {
            received_prompts: Arc::new(Mutex::new(Vec::new())),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        )
        .with_audit(Box::new(RecordingAuditSink {
            events: audit_events.clone(),
        }));

        agent
            .respond(
                UserMessage {
                    text: "tool:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let events = audit_events.lock().expect("audit lock poisoned");
        let request_id = events
            .iter()
            .find_map(|e| {
                e.detail
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .expect("request_id should exist on audit events");

        let provider_event = events
            .iter()
            .find(|e| e.stage == "provider_call_success")
            .expect("provider success event should exist");
        assert_eq!(
            provider_event
                .detail
                .get("request_id")
                .and_then(Value::as_str),
            Some(request_id.as_str())
        );
        assert!(provider_event
            .detail
            .get("duration_ms")
            .and_then(Value::as_u64)
            .is_some());

        let tool_event = events
            .iter()
            .find(|e| e.stage == "tool_execute_success")
            .expect("tool success event should exist");
        assert_eq!(
            tool_event.detail.get("request_id").and_then(Value::as_str),
            Some(request_id.as_str())
        );
        assert!(tool_event
            .detail
            .get("duration_ms")
            .and_then(Value::as_u64)
            .is_some());
    }

    #[tokio::test]
    async fn hook_errors_respect_block_mode_for_high_tier_negative_path() {
        let memory = TestMemory::default();
        let provider = TestProvider {
            received_prompts: Arc::new(Mutex::new(Vec::new())),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                hooks: HookPolicy {
                    enabled: true,
                    timeout_ms: 50,
                    fail_closed: false,
                    default_mode: HookFailureMode::Warn,
                    low_tier_mode: HookFailureMode::Ignore,
                    medium_tier_mode: HookFailureMode::Warn,
                    high_tier_mode: HookFailureMode::Block,
                },
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        )
        .with_hooks(Box::new(FailingHookSink));

        let result = agent
            .respond(
                UserMessage {
                    text: "tool:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await;

        match result {
            Err(AgentError::Hook { stage, .. }) => assert_eq!(stage, "before_tool_call"),
            other => panic!("expected hook block error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn hook_errors_respect_warn_mode_for_high_tier_success_path() {
        let audit_events = Arc::new(Mutex::new(Vec::new()));
        let memory = TestMemory::default();
        let provider = TestProvider {
            received_prompts: Arc::new(Mutex::new(Vec::new())),
            response_text: "ok".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                hooks: HookPolicy {
                    enabled: true,
                    timeout_ms: 50,
                    fail_closed: false,
                    default_mode: HookFailureMode::Warn,
                    low_tier_mode: HookFailureMode::Ignore,
                    medium_tier_mode: HookFailureMode::Warn,
                    high_tier_mode: HookFailureMode::Warn,
                },
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        )
        .with_hooks(Box::new(FailingHookSink))
        .with_audit(Box::new(RecordingAuditSink {
            events: audit_events.clone(),
        }));

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("warn mode should continue");
        assert!(!response.text.is_empty());

        let events = audit_events.lock().expect("audit lock poisoned");
        assert!(events.iter().any(|event| event.stage == "hook_error_warn"));
    }

    // ── C1: Self-correction prompt injection tests ──────────────────────────

    #[tokio::test]
    async fn self_correction_injected_on_no_progress() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "I'll try a different approach".to_string(),
        };
        // LoopTool always returns "tool:loop_tool x", creating a self-referencing
        // chain that triggers no-progress detection after 3 identical iterations.
        let agent = Agent::new(
            AgentConfig {
                loop_detection_no_progress_threshold: 3,
                max_tool_iterations: 20,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(LoopTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:loop_tool x".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("self-correction should succeed");

        assert_eq!(response.text, "I'll try a different approach");

        // The provider should have received the self-correction prompt.
        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("Loop detected"),
            "provider prompt should contain self-correction notice, got: {last_prompt}"
        );
        assert!(last_prompt.contains("loop_tool"));
    }

    #[tokio::test]
    async fn self_correction_injected_on_ping_pong() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "I stopped the loop".to_string(),
        };
        // PingTool→PongTool→PingTool→PongTool creates a 2-cycle ping-pong.
        let agent = Agent::new(
            AgentConfig {
                loop_detection_ping_pong_cycles: 2,
                loop_detection_no_progress_threshold: 0, // disable no-progress
                max_tool_iterations: 20,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(PingTool), Box::new(PongTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:ping x".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("self-correction should succeed");

        assert_eq!(response.text, "I stopped the loop");

        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("ping-pong"),
            "provider prompt should contain ping-pong notice, got: {last_prompt}"
        );
    }

    #[tokio::test]
    async fn no_progress_disabled_when_threshold_zero() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "final answer".to_string(),
        };
        // With detection disabled, LoopTool chains until max_tool_iterations
        // exhausts without triggering self-correction.
        let agent = Agent::new(
            AgentConfig {
                loop_detection_no_progress_threshold: 0,
                loop_detection_ping_pong_cycles: 0,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(LoopTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:loop_tool x".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should complete without error");

        assert_eq!(response.text, "final answer");

        // Provider should NOT receive a self-correction notice.
        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            !last_prompt.contains("Loop detected"),
            "should not contain self-correction when detection is disabled"
        );
    }

    // ── C3: Parallel tool execution tests ───────────────────────────────────

    #[tokio::test]
    async fn parallel_tools_executes_multiple_calls() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "parallel done".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool), Box::new(UpperTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo hello\ntool:upper world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("parallel tools should succeed");

        assert_eq!(response.text, "parallel done");

        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("echoed:hello"),
            "should contain echo output, got: {last_prompt}"
        );
        assert!(
            last_prompt.contains("WORLD"),
            "should contain upper output, got: {last_prompt}"
        );
    }

    #[tokio::test]
    async fn parallel_tools_maintains_stable_ordering() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "ordered".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool), Box::new(UpperTool)],
        );

        // echo comes first in the prompt; its output should appear first.
        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo aaa\ntool:upper bbb".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("parallel tools should succeed");

        assert_eq!(response.text, "ordered");

        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        let echo_pos = last_prompt
            .find("echoed:aaa")
            .expect("echo output should exist");
        let upper_pos = last_prompt.find("BBB").expect("upper output should exist");
        assert!(
            echo_pos < upper_pos,
            "echo output should come before upper output in the prompt"
        );
    }

    #[tokio::test]
    async fn parallel_disabled_runs_first_call_only() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "sequential".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                parallel_tools: false,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool), Box::new(UpperTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo hello\ntool:upper world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("sequential tools should succeed");

        assert_eq!(response.text, "sequential");

        // Only the first tool (echo) should have executed.
        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("echoed:hello"),
            "should contain first tool output, got: {last_prompt}"
        );
        assert!(
            !last_prompt.contains("WORLD"),
            "should NOT contain second tool output when parallel is disabled"
        );
    }

    #[tokio::test]
    async fn single_call_parallel_matches_sequential() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "single".to_string(),
        };
        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo ping".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("single tool with parallel enabled should succeed");

        assert_eq!(response.text, "single");

        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("echoed:ping"),
            "single tool call should work same as sequential, got: {last_prompt}"
        );
    }

    #[tokio::test]
    async fn parallel_falls_back_to_sequential_for_gated_tools() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "gated sequential".to_string(),
        };
        let mut gated = std::collections::HashSet::new();
        gated.insert("upper".to_string());
        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                gated_tools: gated,
                max_tool_iterations: 5,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool), Box::new(UpperTool)],
        );

        // Both tools present, but upper is gated → sequential fallback.
        // Sequential processes only the first call per iteration.
        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo hello\ntool:upper world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("gated fallback should succeed");

        assert_eq!(response.text, "gated sequential");

        let received = prompts.lock().expect("provider lock poisoned");
        let last_prompt = received.last().expect("provider should have been called");
        assert!(
            last_prompt.contains("echoed:hello"),
            "first tool should execute, got: {last_prompt}"
        );
        assert!(
            !last_prompt.contains("WORLD"),
            "gated tool should NOT execute in parallel, got: {last_prompt}"
        );
    }

    // ── C2: Research Phase tests ──────────────────────────────────────

    fn research_config(trigger: ResearchTrigger) -> ResearchPolicy {
        ResearchPolicy {
            enabled: true,
            trigger,
            keywords: vec!["search".to_string(), "find".to_string()],
            min_message_length: 10,
            max_iterations: 5,
            show_progress: true,
        }
    }

    fn config_with_research(trigger: ResearchTrigger) -> AgentConfig {
        AgentConfig {
            research: research_config(trigger),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn research_trigger_never() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "final answer".to_string(),
        };
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Never),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "search for something".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "final answer");
        let received = prompts.lock().expect("lock");
        assert_eq!(
            received.len(),
            1,
            "should have exactly 1 provider call (no research)"
        );
    }

    #[tokio::test]
    async fn research_trigger_always() {
        let provider = ScriptedProvider::new(vec![
            "tool:echo gathering data",
            "Found relevant information",
            "Final answer with research context",
        ]);
        let prompts = provider.received_prompts.clone();
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Always),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hello world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Final answer with research context");
        let received = prompts.lock().expect("lock");
        let last = received.last().expect("should have provider calls");
        assert!(
            last.contains("Research findings:"),
            "final prompt should contain research findings, got: {last}"
        );
    }

    #[tokio::test]
    async fn research_trigger_keywords_match() {
        let provider = ScriptedProvider::new(vec!["Summary of search", "Answer based on research"]);
        let prompts = provider.received_prompts.clone();
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Keywords),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "please search for the config".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Answer based on research");
        let received = prompts.lock().expect("lock");
        assert!(received.len() >= 2, "should have at least 2 provider calls");
        assert!(
            received[0].contains("RESEARCH mode"),
            "first call should be research prompt"
        );
    }

    #[tokio::test]
    async fn research_trigger_keywords_no_match() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "direct answer".to_string(),
        };
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Keywords),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hello world".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "direct answer");
        let received = prompts.lock().expect("lock");
        assert_eq!(received.len(), 1, "no research phase should fire");
    }

    #[tokio::test]
    async fn research_trigger_length_short_skips() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "short".to_string(),
        };
        let config = AgentConfig {
            research: ResearchPolicy {
                min_message_length: 20,
                ..research_config(ResearchTrigger::Length)
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );
        agent
            .respond(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let received = prompts.lock().expect("lock");
        assert_eq!(received.len(), 1, "short message should skip research");
    }

    #[tokio::test]
    async fn research_trigger_length_long_triggers() {
        let provider = ScriptedProvider::new(vec!["research summary", "answer with research"]);
        let prompts = provider.received_prompts.clone();
        let config = AgentConfig {
            research: ResearchPolicy {
                min_message_length: 20,
                ..research_config(ResearchTrigger::Length)
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );
        agent
            .respond(
                UserMessage {
                    text: "this is a longer message that exceeds the threshold".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let received = prompts.lock().expect("lock");
        assert!(received.len() >= 2, "long message should trigger research");
    }

    #[tokio::test]
    async fn research_trigger_question_with_mark() {
        let provider = ScriptedProvider::new(vec!["research summary", "answer with research"]);
        let prompts = provider.received_prompts.clone();
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Question),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );
        agent
            .respond(
                UserMessage {
                    text: "what is the meaning of life?".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let received = prompts.lock().expect("lock");
        assert!(received.len() >= 2, "question should trigger research");
    }

    #[tokio::test]
    async fn research_trigger_question_without_mark() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "no research".to_string(),
        };
        let agent = Agent::new(
            config_with_research(ResearchTrigger::Question),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );
        agent
            .respond(
                UserMessage {
                    text: "do this thing".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");
        let received = prompts.lock().expect("lock");
        assert_eq!(received.len(), 1, "non-question should skip research");
    }

    #[tokio::test]
    async fn research_respects_max_iterations() {
        let provider = ScriptedProvider::new(vec![
            "tool:echo step1",
            "tool:echo step2",
            "tool:echo step3",
            "tool:echo step4",
            "tool:echo step5",
            "tool:echo step6",
            "tool:echo step7",
            "answer after research",
        ]);
        let prompts = provider.received_prompts.clone();
        let config = AgentConfig {
            research: ResearchPolicy {
                max_iterations: 3,
                ..research_config(ResearchTrigger::Always)
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "test".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert!(!response.text.is_empty(), "should get a response");
        let received = prompts.lock().expect("lock");
        let research_calls = received
            .iter()
            .filter(|p| p.contains("RESEARCH mode") || p.contains("Research iteration"))
            .count();
        // 1 initial research prompt + 3 iteration prompts = 4
        assert_eq!(
            research_calls, 4,
            "should have 4 research provider calls (1 initial + 3 iterations)"
        );
    }

    #[tokio::test]
    async fn research_disabled_skips_phase() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "direct answer".to_string(),
        };
        let config = AgentConfig {
            research: ResearchPolicy {
                enabled: false,
                trigger: ResearchTrigger::Always,
                ..Default::default()
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "search for something".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "direct answer");
        let received = prompts.lock().expect("lock");
        assert_eq!(
            received.len(),
            1,
            "disabled research should not fire even with Always trigger"
        );
    }

    // --- Reasoning tests ---

    struct ReasoningCapturingProvider {
        captured_reasoning: Arc<Mutex<Vec<ReasoningConfig>>>,
        response_text: String,
    }

    #[async_trait]
    impl Provider for ReasoningCapturingProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            self.captured_reasoning
                .lock()
                .expect("lock")
                .push(ReasoningConfig::default());
            Ok(ChatResult {
                output_text: self.response_text.clone(),
                ..Default::default()
            })
        }

        async fn complete_with_reasoning(
            &self,
            _prompt: &str,
            reasoning: &ReasoningConfig,
        ) -> anyhow::Result<ChatResult> {
            self.captured_reasoning
                .lock()
                .expect("lock")
                .push(reasoning.clone());
            Ok(ChatResult {
                output_text: self.response_text.clone(),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn reasoning_config_passed_to_provider() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = ReasoningCapturingProvider {
            captured_reasoning: captured.clone(),
            response_text: "ok".to_string(),
        };
        let config = AgentConfig {
            reasoning: ReasoningConfig {
                enabled: Some(true),
                level: Some("high".to_string()),
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );

        agent
            .respond(
                UserMessage {
                    text: "test".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        let configs = captured.lock().expect("lock");
        assert_eq!(configs.len(), 1, "provider should be called once");
        assert_eq!(configs[0].enabled, Some(true));
        assert_eq!(configs[0].level.as_deref(), Some("high"));
    }

    #[tokio::test]
    async fn reasoning_disabled_passes_config_through() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = ReasoningCapturingProvider {
            captured_reasoning: captured.clone(),
            response_text: "ok".to_string(),
        };
        let config = AgentConfig {
            reasoning: ReasoningConfig {
                enabled: Some(false),
                level: None,
            },
            ..Default::default()
        };
        let agent = Agent::new(
            config,
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );

        agent
            .respond(
                UserMessage {
                    text: "test".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        let configs = captured.lock().expect("lock");
        assert_eq!(configs.len(), 1);
        assert_eq!(
            configs[0].enabled,
            Some(false),
            "disabled reasoning should still be passed to provider"
        );
        assert_eq!(configs[0].level, None);
    }

    // --- Structured tool use test infrastructure ---

    struct StructuredProvider {
        responses: Vec<ChatResult>,
        call_count: AtomicUsize,
        received_messages: Arc<Mutex<Vec<Vec<ConversationMessage>>>>,
    }

    impl StructuredProvider {
        fn new(responses: Vec<ChatResult>) -> Self {
            Self {
                responses,
                call_count: AtomicUsize::new(0),
                received_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Provider for StructuredProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(self.responses.get(idx).cloned().unwrap_or_default())
        }

        async fn complete_with_tools(
            &self,
            messages: &[ConversationMessage],
            _tools: &[ToolDefinition],
            _reasoning: &ReasoningConfig,
        ) -> anyhow::Result<ChatResult> {
            self.received_messages
                .lock()
                .expect("lock")
                .push(messages.to_vec());
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(self.responses.get(idx).cloned().unwrap_or_default())
        }
    }

    struct StructuredEchoTool;

    #[async_trait]
    impl Tool for StructuredEchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "Echoes input back"
        }
        fn input_schema(&self) -> Option<serde_json::Value> {
            Some(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to echo" }
                },
                "required": ["text"]
            }))
        }
        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: format!("echoed:{input}"),
            })
        }
    }

    struct StructuredFailingTool;

    #[async_trait]
    impl Tool for StructuredFailingTool {
        fn name(&self) -> &'static str {
            "boom"
        }
        fn description(&self) -> &'static str {
            "Always fails"
        }
        fn input_schema(&self) -> Option<serde_json::Value> {
            Some(json!({
                "type": "object",
                "properties": {},
            }))
        }
        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Err(anyhow::anyhow!("tool exploded"))
        }
    }

    struct StructuredUpperTool;

    #[async_trait]
    impl Tool for StructuredUpperTool {
        fn name(&self) -> &'static str {
            "upper"
        }
        fn description(&self) -> &'static str {
            "Uppercases input"
        }
        fn input_schema(&self) -> Option<serde_json::Value> {
            Some(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string", "description": "Text to uppercase" }
                },
                "required": ["text"]
            }))
        }
        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: input.to_uppercase(),
            })
        }
    }

    use crate::types::ToolUseRequest;

    // --- Structured tool use tests ---

    #[tokio::test]
    async fn structured_basic_tool_call_then_end_turn() {
        let provider = StructuredProvider::new(vec![
            ChatResult {
                output_text: "Let me echo that.".to_string(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "hello"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "The echo returned: hello".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);
        let received = provider.received_messages.clone();

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "echo hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "The echo returned: hello");

        // Verify the second call included tool results.
        let msgs = received.lock().expect("lock");
        assert_eq!(msgs.len(), 2, "provider should be called twice");
        // Second call should have: [User, Assistant(tool_calls), ToolResult]
        assert!(
            msgs[1].len() >= 3,
            "second call should have user + assistant + tool result"
        );
    }

    #[tokio::test]
    async fn structured_no_tool_calls_returns_immediately() {
        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "Hello! No tools needed.".to_string(),
            tool_calls: vec![],
            stop_reason: Some(StopReason::EndTurn),
            ..Default::default()
        }]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Hello! No tools needed.");
    }

    #[tokio::test]
    async fn structured_tool_not_found_sends_error_result() {
        let provider = StructuredProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "nonexistent".to_string(),
                    input: json!({}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "I see the tool was not found.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);
        let received = provider.received_messages.clone();

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "use nonexistent".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed, not abort");

        assert_eq!(response.text, "I see the tool was not found.");

        // Verify the error ToolResult was sent to provider.
        let msgs = received.lock().expect("lock");
        let last_call = &msgs[1];
        let has_error_result = last_call.iter().any(|m| {
            matches!(m, ConversationMessage::ToolResult(r) if r.is_error && r.content.contains("not found"))
        });
        assert!(has_error_result, "should include error ToolResult");
    }

    #[tokio::test]
    async fn structured_tool_error_does_not_abort() {
        let provider = StructuredProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "boom".to_string(),
                    input: json!({}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "I handled the error gracefully.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredFailingTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "boom".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed, error becomes ToolResultMessage");

        assert_eq!(response.text, "I handled the error gracefully.");
    }

    #[tokio::test]
    async fn structured_multi_iteration_tool_calls() {
        let provider = StructuredProvider::new(vec![
            // First: call echo
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "first"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            // Second: call echo again
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_2".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "second"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            // Third: final answer
            ChatResult {
                output_text: "Done with two tool calls.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);
        let received = provider.received_messages.clone();

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "echo twice".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Done with two tool calls.");
        let msgs = received.lock().expect("lock");
        assert_eq!(msgs.len(), 3, "three provider calls");
    }

    #[tokio::test]
    async fn structured_max_iterations_forces_final_answer() {
        // Provider always asks for tools — should hit max iterations.
        // With max_tool_iterations = 3, the loop runs 3 times, then
        // one final call is made outside the loop = 4 provider calls total.
        let responses = vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_0".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "loop"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "loop"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_2".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "loop"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            // 4th call: forced final answer (after max iterations).
            ChatResult {
                output_text: "Forced final answer.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ];

        let agent = Agent::new(
            AgentConfig {
                max_tool_iterations: 3,
                loop_detection_no_progress_threshold: 0, // disable to test max iter
                ..AgentConfig::default()
            },
            Box::new(StructuredProvider::new(responses)),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "loop forever".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed with forced answer");

        assert_eq!(response.text, "Forced final answer.");
    }

    #[tokio::test]
    async fn structured_fallback_to_text_when_disabled() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "text path used".to_string(),
        };

        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: false,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "text path used");
    }

    #[tokio::test]
    async fn structured_fallback_when_no_tool_schemas() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TestProvider {
            received_prompts: prompts.clone(),
            response_text: "text path used".to_string(),
        };

        // EchoTool has no input_schema, so no tool definitions → text path.
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "text path used");
    }

    #[tokio::test]
    async fn structured_parallel_tool_calls() {
        let provider = StructuredProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![
                    ToolUseRequest {
                        id: "call_1".to_string(),
                        name: "echo".to_string(),
                        input: json!({"text": "a"}),
                    },
                    ToolUseRequest {
                        id: "call_2".to_string(),
                        name: "upper".to_string(),
                        input: json!({"text": "b"}),
                    },
                ],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "Both tools ran.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);
        let received = provider.received_messages.clone();

        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool), Box::new(StructuredUpperTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "do both".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Both tools ran.");

        // Both tool results should be in the second call.
        let msgs = received.lock().expect("lock");
        let tool_results: Vec<_> = msgs[1]
            .iter()
            .filter(|m| matches!(m, ConversationMessage::ToolResult(_)))
            .collect();
        assert_eq!(tool_results.len(), 2, "should have two tool results");
    }

    #[tokio::test]
    async fn structured_memory_integration() {
        let memory = TestMemory::default();
        // Pre-populate memory with a prior conversation.
        memory
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "old question".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        memory
            .append(MemoryEntry {
                role: "assistant".to_string(),
                content: "old answer".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();

        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "I see your history.".to_string(),
            tool_calls: vec![],
            stop_reason: Some(StopReason::EndTurn),
            ..Default::default()
        }]);
        let received = provider.received_messages.clone();

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(StructuredEchoTool)],
        );

        agent
            .respond(
                UserMessage {
                    text: "new question".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed");

        let msgs = received.lock().expect("lock");
        // Should have: [User("old question"), Assistant("old answer"), User("new question")]
        // Plus the user message we write to memory before respond_with_tools runs.
        assert!(msgs[0].len() >= 3, "should include memory messages");
        // First message should be the oldest memory entry.
        assert!(matches!(
            &msgs[0][0],
            ConversationMessage::User { content, .. } if content == "old question"
        ));
    }

    #[tokio::test]
    async fn prepare_tool_input_extracts_single_string_field() {
        let tool = StructuredEchoTool;
        let input = json!({"text": "hello world"});
        let result = prepare_tool_input(&tool, &input).expect("valid input");
        assert_eq!(result, "hello world");
    }

    #[tokio::test]
    async fn prepare_tool_input_serializes_multi_field_json() {
        // A tool with multiple required fields should serialize to JSON.
        struct MultiFieldTool;
        #[async_trait]
        impl Tool for MultiFieldTool {
            fn name(&self) -> &'static str {
                "multi"
            }
            fn input_schema(&self) -> Option<serde_json::Value> {
                Some(json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "content": { "type": "string" }
                    },
                    "required": ["path", "content"]
                }))
            }
            async fn execute(
                &self,
                _input: &str,
                _ctx: &ToolContext,
            ) -> anyhow::Result<ToolResult> {
                unreachable!()
            }
        }

        let tool = MultiFieldTool;
        let input = json!({"path": "a.txt", "content": "hello"});
        let result = prepare_tool_input(&tool, &input).expect("valid input");
        // Should be valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");
        assert_eq!(parsed["path"], "a.txt");
        assert_eq!(parsed["content"], "hello");
    }

    #[tokio::test]
    async fn prepare_tool_input_unwraps_bare_string() {
        let tool = StructuredEchoTool;
        let input = json!("plain text");
        let result = prepare_tool_input(&tool, &input).expect("valid input");
        assert_eq!(result, "plain text");
    }

    #[tokio::test]
    async fn prepare_tool_input_rejects_schema_violating_input() {
        struct StrictTool;
        #[async_trait]
        impl Tool for StrictTool {
            fn name(&self) -> &'static str {
                "strict"
            }
            fn input_schema(&self) -> Option<serde_json::Value> {
                Some(json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string" }
                    }
                }))
            }
            async fn execute(
                &self,
                _input: &str,
                _ctx: &ToolContext,
            ) -> anyhow::Result<ToolResult> {
                unreachable!("should not be called with invalid input")
            }
        }

        // Missing required field
        let result = prepare_tool_input(&StrictTool, &json!({}));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Invalid input for tool 'strict'"));
        assert!(err.contains("missing required field"));
    }

    #[tokio::test]
    async fn prepare_tool_input_rejects_wrong_type() {
        struct TypedTool;
        #[async_trait]
        impl Tool for TypedTool {
            fn name(&self) -> &'static str {
                "typed"
            }
            fn input_schema(&self) -> Option<serde_json::Value> {
                Some(json!({
                    "type": "object",
                    "required": ["count"],
                    "properties": {
                        "count": { "type": "integer" }
                    }
                }))
            }
            async fn execute(
                &self,
                _input: &str,
                _ctx: &ToolContext,
            ) -> anyhow::Result<ToolResult> {
                unreachable!("should not be called with invalid input")
            }
        }

        // Wrong type: string instead of integer
        let result = prepare_tool_input(&TypedTool, &json!({"count": "not a number"}));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("expected type \"integer\""));
    }

    #[test]
    fn truncate_messages_preserves_small_conversation() {
        let mut msgs = vec![
            ConversationMessage::user("hello".to_string()),
            ConversationMessage::Assistant {
                content: Some("world".to_string()),
                tool_calls: vec![],
            },
        ];
        truncate_messages(&mut msgs, 1000);
        assert_eq!(msgs.len(), 2, "should not truncate small conversation");
    }

    #[test]
    fn truncate_messages_drops_middle() {
        let mut msgs = vec![
            ConversationMessage::user("A".repeat(100)),
            ConversationMessage::Assistant {
                content: Some("B".repeat(100)),
                tool_calls: vec![],
            },
            ConversationMessage::user("C".repeat(100)),
            ConversationMessage::Assistant {
                content: Some("D".repeat(100)),
                tool_calls: vec![],
            },
        ];
        // Budget of 250 chars: first (100) + last (100) = 200 fits,
        // but first + last two = 300 doesn't fit.
        truncate_messages(&mut msgs, 250);
        assert!(msgs.len() < 4, "should have dropped some messages");
        // First message preserved.
        assert!(matches!(
            &msgs[0],
            ConversationMessage::User { content, .. } if content.starts_with("AAA")
        ));
    }

    #[test]
    fn memory_to_messages_reverses_order() {
        let entries = vec![
            MemoryEntry {
                role: "assistant".to_string(),
                content: "newest".to_string(),
                ..Default::default()
            },
            MemoryEntry {
                role: "user".to_string(),
                content: "oldest".to_string(),
                ..Default::default()
            },
        ];
        let msgs = memory_to_messages(&entries);
        assert_eq!(msgs.len(), 2);
        assert!(matches!(
            &msgs[0],
            ConversationMessage::User { content, .. } if content == "oldest"
        ));
        assert!(matches!(
            &msgs[1],
            ConversationMessage::Assistant { content: Some(c), .. } if c == "newest"
        ));
    }

    #[test]
    fn single_required_string_field_detects_single() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        });
        assert_eq!(
            single_required_string_field(&schema),
            Some("path".to_string())
        );
    }

    #[test]
    fn single_required_string_field_none_for_multiple() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "mode": { "type": "string" }
            },
            "required": ["path", "mode"]
        });
        assert_eq!(single_required_string_field(&schema), None);
    }

    #[test]
    fn single_required_string_field_none_for_non_string() {
        let schema = json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer" }
            },
            "required": ["count"]
        });
        assert_eq!(single_required_string_field(&schema), None);
    }

    // --- Streaming tests ---

    use crate::types::StreamChunk;

    /// Provider that sends individual token chunks through the streaming sink.
    struct StreamingProvider {
        responses: Vec<ChatResult>,
        call_count: AtomicUsize,
        received_messages: Arc<Mutex<Vec<Vec<ConversationMessage>>>>,
    }

    impl StreamingProvider {
        fn new(responses: Vec<ChatResult>) -> Self {
            Self {
                responses,
                call_count: AtomicUsize::new(0),
                received_messages: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl Provider for StreamingProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(self.responses.get(idx).cloned().unwrap_or_default())
        }

        async fn complete_streaming(
            &self,
            _prompt: &str,
            sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
        ) -> anyhow::Result<ChatResult> {
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let result = self.responses.get(idx).cloned().unwrap_or_default();
            // Send word-by-word chunks.
            for word in result.output_text.split_whitespace() {
                let _ = sender.send(StreamChunk {
                    delta: format!("{word} "),
                    done: false,
                    tool_call_delta: None,
                });
            }
            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });
            Ok(result)
        }

        async fn complete_with_tools(
            &self,
            messages: &[ConversationMessage],
            _tools: &[ToolDefinition],
            _reasoning: &ReasoningConfig,
        ) -> anyhow::Result<ChatResult> {
            self.received_messages
                .lock()
                .expect("lock")
                .push(messages.to_vec());
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(self.responses.get(idx).cloned().unwrap_or_default())
        }

        async fn complete_streaming_with_tools(
            &self,
            messages: &[ConversationMessage],
            _tools: &[ToolDefinition],
            _reasoning: &ReasoningConfig,
            sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
        ) -> anyhow::Result<ChatResult> {
            self.received_messages
                .lock()
                .expect("lock")
                .push(messages.to_vec());
            let idx = self.call_count.fetch_add(1, Ordering::Relaxed);
            let result = self.responses.get(idx).cloned().unwrap_or_default();
            // Send character-by-character for text.
            for ch in result.output_text.chars() {
                let _ = sender.send(StreamChunk {
                    delta: ch.to_string(),
                    done: false,
                    tool_call_delta: None,
                });
            }
            let _ = sender.send(StreamChunk {
                delta: String::new(),
                done: true,
                tool_call_delta: None,
            });
            Ok(result)
        }
    }

    #[tokio::test]
    async fn streaming_text_only_sends_chunks() {
        let provider = StreamingProvider::new(vec![ChatResult {
            output_text: "Hello world".to_string(),
            ..Default::default()
        }]);
        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: false,
                ..Default::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![],
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Hello world");

        // Collect all chunks.
        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        assert!(chunks.len() >= 2, "should have at least text + done chunks");
        assert!(chunks.last().unwrap().done, "last chunk should be done");
    }

    #[tokio::test]
    async fn streaming_single_tool_call_round_trip() {
        let provider = StreamingProvider::new(vec![
            ChatResult {
                output_text: "I'll echo that.".to_string(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "hello"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "Done echoing.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "echo hello".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Done echoing.");

        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        // Should have chunks from both provider calls (tool use + final response).
        assert!(chunks.len() >= 2, "should have streaming chunks");
    }

    #[tokio::test]
    async fn streaming_multi_iteration_tool_calls() {
        let provider = StreamingProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: json!({"text": "first"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_2".to_string(),
                    name: "upper".to_string(),
                    input: json!({"text": "second"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "All done.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool), Box::new(StructuredUpperTool)],
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "do two things".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "All done.");

        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        // Chunks from 3 provider calls.
        let done_count = chunks.iter().filter(|c| c.done).count();
        assert!(
            done_count >= 3,
            "should have done chunks from 3 provider calls, got {}",
            done_count
        );
    }

    #[tokio::test]
    async fn streaming_timeout_returns_error() {
        struct StreamingSlowProvider;

        #[async_trait]
        impl Provider for StreamingSlowProvider {
            async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
                sleep(Duration::from_millis(500)).await;
                Ok(ChatResult::default())
            }

            async fn complete_streaming(
                &self,
                _prompt: &str,
                _sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
            ) -> anyhow::Result<ChatResult> {
                sleep(Duration::from_millis(500)).await;
                Ok(ChatResult::default())
            }
        }

        let agent = Agent::new(
            AgentConfig {
                request_timeout_ms: 50,
                model_supports_tool_use: false,
                ..Default::default()
            },
            Box::new(StreamingSlowProvider),
            Box::new(TestMemory::default()),
            vec![],
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = agent
            .respond_streaming(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await;

        assert!(result.is_err());
        match result.unwrap_err() {
            AgentError::Timeout { timeout_ms } => assert_eq!(timeout_ms, 50),
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn streaming_no_schema_fallback_sends_chunks() {
        // Tools without schemas → text-only fallback path.
        let provider = StreamingProvider::new(vec![ChatResult {
            output_text: "Fallback response".to_string(),
            ..Default::default()
        }]);
        let agent = Agent::new(
            AgentConfig::default(), // model_supports_tool_use: true, but no schemas
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(EchoTool)], // EchoTool has no input_schema
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Fallback response");

        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }
        assert!(!chunks.is_empty(), "should have streaming chunks");
        assert!(chunks.last().unwrap().done, "last chunk should be done");
    }

    #[tokio::test]
    async fn streaming_tool_error_does_not_abort() {
        let provider = StreamingProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "boom".to_string(),
                    input: json!({}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "Recovered from error.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredFailingTool)],
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "boom".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed despite tool error");

        assert_eq!(response.text, "Recovered from error.");
    }

    #[tokio::test]
    async fn streaming_parallel_tools() {
        let provider = StreamingProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![
                    ToolUseRequest {
                        id: "call_1".to_string(),
                        name: "echo".to_string(),
                        input: json!({"text": "a"}),
                    },
                    ToolUseRequest {
                        id: "call_2".to_string(),
                        name: "upper".to_string(),
                        input: json!({"text": "b"}),
                    },
                ],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "Parallel done.".to_string(),
                tool_calls: vec![],
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);

        let agent = Agent::new(
            AgentConfig {
                parallel_tools: true,
                ..Default::default()
            },
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool), Box::new(StructuredUpperTool)],
        );

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let response = agent
            .respond_streaming(
                UserMessage {
                    text: "parallel test".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        assert_eq!(response.text, "Parallel done.");
    }

    #[tokio::test]
    async fn streaming_done_chunk_sentinel() {
        let provider = StreamingProvider::new(vec![ChatResult {
            output_text: "abc".to_string(),
            tool_calls: vec![],
            stop_reason: Some(StopReason::EndTurn),
            ..Default::default()
        }]);

        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(TestMemory::default()),
            vec![Box::new(StructuredEchoTool)],
        );

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let _response = agent
            .respond_streaming(
                UserMessage {
                    text: "test".to_string(),
                },
                &test_ctx(),
                tx,
            )
            .await
            .expect("should succeed");

        let mut chunks = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            chunks.push(chunk);
        }

        // Must have at least one done chunk.
        let done_chunks: Vec<_> = chunks.iter().filter(|c| c.done).collect();
        assert!(
            !done_chunks.is_empty(),
            "must have at least one done sentinel chunk"
        );
        // The last chunk should be done.
        assert!(chunks.last().unwrap().done, "final chunk must be done=true");
        // Non-done chunks should have content.
        let content_chunks: Vec<_> = chunks
            .iter()
            .filter(|c| !c.done && !c.delta.is_empty())
            .collect();
        assert!(!content_chunks.is_empty(), "should have content chunks");
    }

    // ===== System prompt tests =====

    #[tokio::test]
    async fn system_prompt_prepended_in_structured_path() {
        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "I understand.".to_string(),
            stop_reason: Some(StopReason::EndTurn),
            ..Default::default()
        }]);
        let received = provider.received_messages.clone();
        let memory = TestMemory::default();

        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: true,
                system_prompt: Some("You are a math tutor.".to_string()),
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(StructuredEchoTool)],
        );

        let result = agent
            .respond(
                UserMessage {
                    text: "What is 2+2?".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        assert_eq!(result.text, "I understand.");

        let msgs = received.lock().expect("lock");
        assert!(!msgs.is_empty(), "provider should have been called");
        let first_call = &msgs[0];
        // First message should be the system prompt.
        match &first_call[0] {
            ConversationMessage::System { content } => {
                assert_eq!(content, "You are a math tutor.");
            }
            other => panic!("expected System message first, got {other:?}"),
        }
        // Second message should be the user message.
        match &first_call[1] {
            ConversationMessage::User { content, .. } => {
                assert_eq!(content, "What is 2+2?");
            }
            other => panic!("expected User message second, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn no_system_prompt_omits_system_message() {
        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "ok".to_string(),
            stop_reason: Some(StopReason::EndTurn),
            ..Default::default()
        }]);
        let received = provider.received_messages.clone();
        let memory = TestMemory::default();

        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: true,
                system_prompt: None,
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(StructuredEchoTool)],
        );

        agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let msgs = received.lock().expect("lock");
        let first_call = &msgs[0];
        // First message should be User, not System.
        match &first_call[0] {
            ConversationMessage::User { content, .. } => {
                assert_eq!(content, "hello");
            }
            other => panic!("expected User message first (no system prompt), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn system_prompt_persists_across_tool_iterations() {
        // Provider: first call requests a tool, second call returns final answer.
        let provider = StructuredProvider::new(vec![
            ChatResult {
                output_text: String::new(),
                tool_calls: vec![ToolUseRequest {
                    id: "call_1".to_string(),
                    name: "echo".to_string(),
                    input: serde_json::json!({"text": "ping"}),
                }],
                stop_reason: Some(StopReason::ToolUse),
                ..Default::default()
            },
            ChatResult {
                output_text: "pong".to_string(),
                stop_reason: Some(StopReason::EndTurn),
                ..Default::default()
            },
        ]);
        let received = provider.received_messages.clone();
        let memory = TestMemory::default();

        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: true,
                system_prompt: Some("Always be concise.".to_string()),
                ..AgentConfig::default()
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(StructuredEchoTool)],
        );

        let result = agent
            .respond(
                UserMessage {
                    text: "test".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");
        assert_eq!(result.text, "pong");

        let msgs = received.lock().expect("lock");
        // Both provider calls should have the system prompt as first message.
        for (i, call_msgs) in msgs.iter().enumerate() {
            match &call_msgs[0] {
                ConversationMessage::System { content } => {
                    assert_eq!(content, "Always be concise.", "call {i} system prompt");
                }
                other => panic!("call {i}: expected System first, got {other:?}"),
            }
        }
    }

    #[test]
    fn agent_config_system_prompt_defaults_to_none() {
        let config = AgentConfig::default();
        assert!(config.system_prompt.is_none());
    }

    #[tokio::test]
    async fn memory_write_propagates_source_channel_from_context() {
        let memory = TestMemory::default();
        let entries = memory.entries.clone();
        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "noted".to_string(),
            tool_calls: vec![],
            stop_reason: None,
            ..Default::default()
        }]);
        let config = AgentConfig {
            privacy_boundary: "encrypted_only".to_string(),
            ..AgentConfig::default()
        };
        let agent = Agent::new(config, Box::new(provider), Box::new(memory), vec![]);

        let mut ctx = ToolContext::new(".".to_string());
        ctx.source_channel = Some("telegram".to_string());
        ctx.privacy_boundary = "encrypted_only".to_string();

        agent
            .respond(
                UserMessage {
                    text: "hello".to_string(),
                },
                &ctx,
            )
            .await
            .expect("respond should succeed");

        let stored = entries.lock().expect("memory lock poisoned");
        // First entry is user, second is assistant.
        assert_eq!(stored[0].source_channel.as_deref(), Some("telegram"));
        assert_eq!(stored[0].privacy_boundary, "encrypted_only");
        assert_eq!(stored[1].source_channel.as_deref(), Some("telegram"));
        assert_eq!(stored[1].privacy_boundary, "encrypted_only");
    }

    #[tokio::test]
    async fn memory_write_source_channel_none_when_ctx_empty() {
        let memory = TestMemory::default();
        let entries = memory.entries.clone();
        let provider = StructuredProvider::new(vec![ChatResult {
            output_text: "ok".to_string(),
            tool_calls: vec![],
            stop_reason: None,
            ..Default::default()
        }]);
        let agent = Agent::new(
            AgentConfig::default(),
            Box::new(provider),
            Box::new(memory),
            vec![],
        );

        agent
            .respond(
                UserMessage {
                    text: "hi".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("respond should succeed");

        let stored = entries.lock().expect("memory lock poisoned");
        assert!(stored[0].source_channel.is_none());
        assert!(stored[1].source_channel.is_none());
    }

    // -----------------------------------------------------------------------
    // Text-based tool call extraction tests (local model fallback)
    // -----------------------------------------------------------------------

    fn sample_tools() -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "web_search".to_string(),
                description: "Search the web".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
            ToolDefinition {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            },
        ]
    }

    #[test]
    fn extract_tool_call_from_json_code_block() {
        let text = "I'll search for that.\n```json\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"AI regulation EU\"}}\n```";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
        assert_eq!(result.input["query"], "AI regulation EU");
    }

    #[test]
    fn extract_tool_call_from_bare_code_block() {
        let text = "```\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"test\"}}\n```";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
    }

    #[test]
    fn extract_tool_call_from_bare_json() {
        let text = "{\"name\": \"web_search\", \"arguments\": {\"query\": \"test\"}}";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
        assert_eq!(result.input["query"], "test");
    }

    #[test]
    fn extract_tool_call_with_parameters_key() {
        let text = "{\"name\": \"web_search\", \"parameters\": {\"query\": \"test\"}}";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
        assert_eq!(result.input["query"], "test");
    }

    #[test]
    fn extract_tool_call_ignores_unknown_tool() {
        let text = "{\"name\": \"unknown_tool\", \"arguments\": {}}";
        let tools = sample_tools();
        assert!(extract_tool_call_from_text(text, &tools).is_none());
    }

    #[test]
    fn extract_tool_call_returns_none_for_plain_text() {
        let text = "I don't know how to help with that.";
        let tools = sample_tools();
        assert!(extract_tool_call_from_text(text, &tools).is_none());
    }

    #[test]
    fn extract_tool_call_returns_none_for_non_tool_json() {
        let text = "{\"message\": \"hello\", \"status\": \"ok\"}";
        let tools = sample_tools();
        assert!(extract_tool_call_from_text(text, &tools).is_none());
    }

    #[test]
    fn extract_tool_call_with_surrounding_text() {
        let text = "Sure, let me search for that.\n{\"name\": \"web_search\", \"arguments\": {\"query\": \"AI regulation\"}}\nI'll get back to you.";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
    }

    #[test]
    fn extract_tool_call_no_arguments_field() {
        let text = "{\"name\": \"web_search\"}";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
        assert!(result.input.is_object());
    }

    #[test]
    fn extract_json_block_handles_nested_braces() {
        let text =
            "```json\n{\"name\": \"read_file\", \"arguments\": {\"path\": \"/tmp/{test}\"}}\n```";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "read_file");
        assert_eq!(result.input["path"], "/tmp/{test}");
    }

    #[test]
    fn extract_bare_json_handles_escaped_quotes() {
        let text =
            "{\"name\": \"web_search\", \"arguments\": {\"query\": \"test \\\"quoted\\\" word\"}}";
        let tools = sample_tools();
        let result = extract_tool_call_from_text(text, &tools).expect("should extract");
        assert_eq!(result.name, "web_search");
    }

    #[test]
    fn extract_tool_call_empty_string() {
        let tools = sample_tools();
        assert!(extract_tool_call_from_text("", &tools).is_none());
    }

    // -----------------------------------------------------------------------
    // Integration test: local model text fallback through agent loop
    // -----------------------------------------------------------------------

    /// Provider that returns a tool call as text (simulating local model behavior)
    /// on the first call, then returns a final response on subsequent calls.
    struct TextToolCallProvider {
        call_count: AtomicUsize,
    }

    impl TextToolCallProvider {
        fn new() -> Self {
            Self {
                call_count: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl Provider for TextToolCallProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Ok(ChatResult::default())
        }

        async fn complete_with_tools(
            &self,
            messages: &[ConversationMessage],
            _tools: &[ToolDefinition],
            _reasoning: &ReasoningConfig,
        ) -> anyhow::Result<ChatResult> {
            let n = self.call_count.fetch_add(1, Ordering::Relaxed);
            if n == 0 {
                // Simulate local model: returns tool call as text, not structured.
                Ok(ChatResult {
                    output_text: "```json\n{\"name\": \"echo\", \"arguments\": {\"message\": \"hello from text\"}}\n```".to_string(),
                    tool_calls: vec![],
                    stop_reason: Some(StopReason::EndTurn),
                    ..Default::default()
                })
            } else {
                // After tool result is fed back, return a final text response.
                let tool_output = messages
                    .iter()
                    .rev()
                    .find_map(|m| match m {
                        ConversationMessage::ToolResult(r) => Some(r.content.as_str()),
                        _ => None,
                    })
                    .unwrap_or("no tool result");
                Ok(ChatResult {
                    output_text: format!("Got it: {tool_output}"),
                    tool_calls: vec![],
                    stop_reason: Some(StopReason::EndTurn),
                    ..Default::default()
                })
            }
        }
    }

    /// Echo tool with schema (needed for structured tool path).
    struct TestEchoTool;

    #[async_trait]
    impl crate::types::Tool for TestEchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        fn description(&self) -> &'static str {
            "Echo back the message"
        }
        fn input_schema(&self) -> Option<serde_json::Value> {
            Some(serde_json::json!({
                "type": "object",
                "required": ["message"],
                "properties": {
                    "message": {"type": "string"}
                }
            }))
        }
        async fn execute(
            &self,
            input: &str,
            _ctx: &ToolContext,
        ) -> anyhow::Result<crate::types::ToolResult> {
            let v: serde_json::Value =
                serde_json::from_str(input).unwrap_or(Value::String(input.to_string()));
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or(input);
            Ok(crate::types::ToolResult {
                output: format!("echoed:{msg}"),
            })
        }
    }

    #[tokio::test]
    async fn text_tool_extraction_dispatches_through_agent_loop() {
        let agent = Agent::new(
            AgentConfig {
                model_supports_tool_use: true,
                max_tool_iterations: 5,
                request_timeout_ms: 10_000,
                ..AgentConfig::default()
            },
            Box::new(TextToolCallProvider::new()),
            Box::new(TestMemory::default()),
            vec![Box::new(TestEchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "echo hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed with text-extracted tool call");

        assert!(
            response.text.contains("echoed:hello from text"),
            "expected tool result in response, got: {}",
            response.text
        );
    }

    #[tokio::test]
    async fn tool_execution_timeout_fires() {
        let memory = TestMemory::default();
        let agent = Agent::new(
            AgentConfig {
                max_tool_iterations: 3,
                tool_timeout_ms: 50, // 50ms — SlowTool sleeps 500ms
                ..AgentConfig::default()
            },
            Box::new(ScriptedProvider::new(vec!["done"])),
            Box::new(memory),
            vec![Box::new(SlowTool)],
        );

        let result = agent
            .respond(
                UserMessage {
                    text: "tool:slow go".to_string(),
                },
                &test_ctx(),
            )
            .await;

        let err = result.expect_err("should time out");
        let msg = format!("{err}");
        assert!(
            msg.contains("timed out"),
            "expected timeout error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn tool_execution_no_timeout_when_disabled() {
        let memory = TestMemory::default();
        let agent = Agent::new(
            AgentConfig {
                max_tool_iterations: 3,
                tool_timeout_ms: 0, // disabled — tool should run without timeout
                ..AgentConfig::default()
            },
            Box::new(ScriptedProvider::new(vec!["done"])),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        );

        let response = agent
            .respond(
                UserMessage {
                    text: "tool:echo hello".to_string(),
                },
                &test_ctx(),
            )
            .await
            .expect("should succeed with timeout disabled");

        // ScriptedProvider returns "done" after tool executes; the tool ran
        // without a timeout wrapper, confirming the 0 = disabled path works.
        assert_eq!(response.text, "done");
    }
}

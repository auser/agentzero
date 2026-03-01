use crate::types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, HookEvent, HookFailureMode,
    HookRiskTier, HookSink, MemoryEntry, MemoryStore, MetricsSink, Provider, ResearchTrigger, Tool,
    ToolContext, UserMessage,
};
use agentzero_security::redaction::redact_text;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

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

pub struct Agent {
    config: AgentConfig,
    provider: Box<dyn Provider>,
    memory: Box<dyn MemoryStore>,
    tools: Vec<Box<dyn Tool>>,
    audit: Option<Box<dyn AuditSink>>,
    hooks: Option<Box<dyn HookSink>>,
    metrics: Option<Box<dyn MetricsSink>>,
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
        }
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

    async fn execute_tool(
        &self,
        tool: &dyn Tool,
        tool_name: &str,
        tool_input: &str,
        ctx: &ToolContext,
        request_id: &str,
        iteration: usize,
    ) -> Result<crate::types::ToolResult, AgentError> {
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
        let tool_started = Instant::now();
        let result = match tool.execute(tool_input, ctx).await {
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
    ) -> Result<String, AgentError> {
        let recent_memory = self
            .memory
            .recent(self.config.memory_window_size)
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
        let completion = match self
            .provider
            .complete_with_reasoning(&provider_prompt, &self.config.reasoning)
            .await
        {
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
            .call_provider_with_context(&research_prompt, request_id)
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
                    .call_provider_with_context(&next_prompt, request_id)
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

    pub async fn respond(
        &self,
        user: UserMessage,
        ctx: &ToolContext,
    ) -> Result<AssistantMessage, AgentError> {
        let request_id = Self::next_request_id();
        self.increment_counter("requests_total");
        let run_started = Instant::now();
        self.hook("before_run", json!({"request_id": request_id}))
            .await?;
        let timed = timeout(
            Duration::from_millis(self.config.request_timeout_ms),
            self.respond_inner(&request_id, user, ctx),
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
        info!(
            request_id = %request_id,
            duration_ms = %run_started.elapsed().as_millis(),
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
        self.write_to_memory("user", &user.text, request_id).await?;

        let research_context = if self.should_research(&user.text) {
            self.run_research_phase(&user.text, ctx, request_id).await?
        } else {
            String::new()
        };

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

        let response_text = self.call_provider_with_context(&prompt, request_id).await?;
        self.write_to_memory("assistant", &response_text, request_id)
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
            },
            MemoryEntry {
                role: "user".to_string(),
                content: "recent-before-request".to_string(),
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
}

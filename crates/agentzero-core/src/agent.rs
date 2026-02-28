use crate::types::{
    AgentConfig, AgentError, AssistantMessage, AuditEvent, AuditSink, HookEvent, HookSink,
    MemoryEntry, MemoryStore, MetricsSink, Provider, Tool, ToolContext, UserMessage,
};
use agentzero_security::redaction::redact_text;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::time::{timeout, Duration};
use tracing::info;

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
        match timeout(
            Duration::from_millis(self.config.hooks.timeout_ms),
            hook_call,
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                if self.config.hooks.fail_closed {
                    Err(AgentError::Hook {
                        stage: stage.to_string(),
                        source: err,
                    })
                } else {
                    self.audit(
                        "hook_error_ignored",
                        json!({"stage": stage, "error": redact_text(&err.to_string())}),
                    )
                    .await;
                    Ok(())
                }
            }
            Err(_) => {
                if self.config.hooks.fail_closed {
                    Err(AgentError::Hook {
                        stage: stage.to_string(),
                        source: anyhow::anyhow!(
                            "hook execution timed out after {} ms",
                            self.config.hooks.timeout_ms
                        ),
                    })
                } else {
                    self.audit(
                        "hook_timeout_ignored",
                        json!({"stage": stage, "timeout_ms": self.config.hooks.timeout_ms}),
                    )
                    .await;
                    Ok(())
                }
            }
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

        self.hook(
            "before_memory_write",
            json!({"request_id": request_id, "role": "user"}),
        )
        .await?;
        self.memory
            .append(MemoryEntry {
                role: "user".to_string(),
                content: user.text.clone(),
            })
            .await
            .map_err(|source| AgentError::Memory { source })?;
        self.hook(
            "after_memory_write",
            json!({"request_id": request_id, "role": "user"}),
        )
        .await?;
        self.audit("memory_append_user", json!({"request_id": request_id}))
            .await;

        let mut prompt = user.text;

        // Minimal iteration guard to avoid accidental runaway tool loops.
        for iteration in 0..self.config.max_tool_iterations {
            if let Some(rest) = prompt.strip_prefix("tool:") {
                let mut parts = rest.trim().splitn(2, ' ');
                let tool_name = parts.next().unwrap_or_default();
                let tool_input = parts.next().unwrap_or_default();
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
                    self.hook(
                        "before_tool_call",
                        json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name}),
                    )
                    .await?;
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
                    prompt = format!("Tool output from {tool_name}: {}", result.output);
                    continue;
                }
                self.audit(
                    "tool_not_found",
                    json!({"request_id": request_id, "iteration": iteration, "tool_name": tool_name}),
                )
                .await;
            }
            break;
        }

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
            build_provider_prompt(&prompt, &recent_memory, self.config.max_prompt_chars);
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
        let completion = match self.provider.complete(&provider_prompt).await {
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

        self.hook(
            "before_memory_write",
            json!({"request_id": request_id, "role": "assistant"}),
        )
        .await?;
        self.memory
            .append(MemoryEntry {
                role: "assistant".to_string(),
                content: completion.output_text.clone(),
            })
            .await
            .map_err(|source| AgentError::Memory { source })?;
        self.hook(
            "after_memory_write",
            json!({"request_id": request_id, "role": "assistant"}),
        )
        .await?;
        self.audit("memory_append_assistant", json!({"request_id": request_id}))
            .await;
        self.audit("respond_success", json!({"request_id": request_id}))
            .await;

        self.hook(
            "before_response_emit",
            json!({"request_id": request_id, "response_len": completion.output_text.len()}),
        )
        .await?;
        let response = AssistantMessage {
            text: completion.output_text,
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
    use crate::types::{ChatResult, HookEvent, HookPolicy, ToolResult};
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::HashMap;
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
        ToolContext {
            workspace_root: ".".to_string(),
        }
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
                memory_window_size: 8,
                max_prompt_chars: 8_000,
                hooks: HookPolicy::default(),
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
                max_tool_iterations: 4,
                request_timeout_ms: 10_000,
                memory_window_size: 8,
                max_prompt_chars: 8_000,
                hooks: HookPolicy {
                    enabled: true,
                    timeout_ms: 50,
                    fail_closed: true,
                },
            },
            Box::new(provider),
            Box::new(memory),
            vec![Box::new(EchoTool)],
        )
        .with_hooks(Box::new(RecordingHookSink {
            events: events.clone(),
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

        let stages = events.lock().expect("hook lock poisoned");
        assert!(stages.contains(&"before_run".to_string()));
        assert!(stages.contains(&"after_run".to_string()));
        assert!(stages.contains(&"before_tool_call".to_string()));
        assert!(stages.contains(&"after_tool_call".to_string()));
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
}

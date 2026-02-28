use crate::command_core::{AgentZeroCommand, CommandContext};
use crate::commands::memory::build_memory_store;
use agentzero_config::{load, load_audit_policy, load_env_var, load_tool_security_policy};
use agentzero_core::{
    Agent, AgentConfig, AuditEvent, AuditSink, HookEvent, HookPolicy, HookSink, RuntimeMetrics,
    Tool, ToolContext, UserMessage,
};
use agentzero_infra::audit::FileAuditSink;
use agentzero_infra::tools::default_tools;
use agentzero_provider_openai::OpenAiCompatibleProvider;
use anyhow::Context;
use async_trait::async_trait;
use serde_json::json;
use tracing::info;

pub struct AgentOptions {
    /// Message to send to agent
    pub message: String,
}

pub struct AgentCommand;

struct AuditHookSink {
    sink: FileAuditSink,
}

#[async_trait]
impl HookSink for AuditHookSink {
    async fn record(&self, event: HookEvent) -> anyhow::Result<()> {
        self.sink
            .record(AuditEvent {
                stage: format!("hook.{}", event.stage),
                detail: json!({ "hook": event.detail }),
            })
            .await
    }
}

#[async_trait]
impl AgentZeroCommand for AgentCommand {
    type Options = AgentOptions;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        let key = load_env_var(&ctx.config_path, "OPENAI_API_KEY")?.context(
            "missing OPENAI_API_KEY (set env var or .env/.env.local/.env.<environment>)",
        )?;
        let config = load(&ctx.config_path)?;

        let provider =
            OpenAiCompatibleProvider::new(config.provider.base_url, key, config.provider.model);
        let memory = build_memory_store(ctx).await?;
        let workspace = ctx.workspace_root.clone();
        let tool_policy = load_tool_security_policy(&ctx.workspace_root, &ctx.config_path)?;
        let tools: Vec<Box<dyn Tool>> = default_tools(&tool_policy)?;
        let audit_policy = load_audit_policy(&ctx.workspace_root, &ctx.config_path)?;
        let audit_path = audit_policy.path.clone();
        let mut agent = Agent::new(
            AgentConfig {
                max_tool_iterations: config.agent.max_tool_iterations,
                request_timeout_ms: config.agent.request_timeout_ms,
                memory_window_size: config.agent.memory_window_size,
                max_prompt_chars: config.agent.max_prompt_chars,
                hooks: HookPolicy {
                    enabled: config.agent.hooks.enabled,
                    timeout_ms: config.agent.hooks.timeout_ms,
                    fail_closed: config.agent.hooks.fail_closed,
                },
            },
            Box::new(provider),
            memory,
            tools,
        );
        let runtime_metrics = RuntimeMetrics::new();
        agent = agent.with_metrics(Box::new(runtime_metrics.clone()));
        if audit_policy.enabled {
            agent = agent.with_audit(Box::new(FileAuditSink::new(audit_path.clone())));
        }
        if config.agent.hooks.enabled {
            let hook_sink = AuditHookSink {
                sink: FileAuditSink::new(audit_path),
            };
            agent = agent.with_hooks(Box::new(hook_sink));
        }

        let response = agent
            .respond(
                UserMessage { text: opts.message },
                &ToolContext {
                    workspace_root: workspace.to_string_lossy().to_string(),
                },
            )
            .await;
        export_runtime_metrics(&runtime_metrics);

        match response {
            Ok(response) => {
                println!("{}", response.text);
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }
}

fn export_runtime_metrics(metrics: &RuntimeMetrics) {
    info!(
        metrics = %metrics.export_json(),
        "runtime metrics snapshot"
    );
}

#[cfg(test)]
mod tests {
    use super::export_runtime_metrics;
    use agentzero_core::{MetricsSink, RuntimeMetrics};

    #[test]
    fn runtime_metrics_export_snapshot_is_structured_json() {
        let metrics = RuntimeMetrics::new();
        metrics.increment_counter("requests_total", 1);
        metrics.observe_histogram("provider_latency_ms", 12.5);

        let snapshot = metrics.export_json();
        assert_eq!(snapshot["counters"]["requests_total"], 1);
        assert_eq!(snapshot["histograms"]["provider_latency_ms"]["count"], 1);
    }

    #[test]
    fn export_runtime_metrics_handles_empty_snapshot() {
        let metrics = RuntimeMetrics::new();
        export_runtime_metrics(&metrics);
        let snapshot = metrics.export_json();
        assert!(snapshot["histograms"]
            .as_object()
            .expect("histograms should be object")
            .is_empty());
    }
}

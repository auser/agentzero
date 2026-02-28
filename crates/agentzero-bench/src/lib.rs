use agentzero_core::{
    Agent, AgentConfig, AgentError, AssistantMessage, ChatResult, MemoryEntry, MemoryStore,
    Provider, Tool, ToolContext, UserMessage,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct BenchMemory {
    entries: Arc<Mutex<Vec<MemoryEntry>>>,
}

#[async_trait]
impl MemoryStore for BenchMemory {
    async fn append(&self, entry: MemoryEntry) -> anyhow::Result<()> {
        self.entries
            .lock()
            .expect("bench memory lock poisoned")
            .push(entry);
        Ok(())
    }

    async fn recent(&self, limit: usize) -> anyhow::Result<Vec<MemoryEntry>> {
        let entries = self.entries.lock().expect("bench memory lock poisoned");
        Ok(entries.iter().rev().take(limit).cloned().collect())
    }
}

struct BenchProvider {
    response_text: String,
}

#[async_trait]
impl Provider for BenchProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Ok(ChatResult {
            output_text: self.response_text.clone(),
        })
    }
}

struct FailingBenchProvider;

#[async_trait]
impl Provider for FailingBenchProvider {
    async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
        Err(anyhow::anyhow!("bench provider failure"))
    }
}

struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn execute(
        &self,
        input: &str,
        _ctx: &ToolContext,
    ) -> anyhow::Result<agentzero_core::ToolResult> {
        Ok(agentzero_core::ToolResult {
            output: format!("echoed:{input}"),
        })
    }
}

fn bench_ctx() -> ToolContext {
    ToolContext {
        workspace_root: ".".to_string(),
    }
}

pub async fn run_core_loop_iteration(message: &str) -> anyhow::Result<AssistantMessage> {
    let agent = Agent::new(
        AgentConfig::default(),
        Box::new(BenchProvider {
            response_text: "ok".to_string(),
        }),
        Box::new(BenchMemory::default()),
        vec![Box::new(EchoTool)],
    );

    agent
        .respond(
            UserMessage {
                text: message.to_string(),
            },
            &bench_ctx(),
        )
        .await
        .map_err(anyhow::Error::from)
}

pub async fn run_core_loop_iteration_failure(
    message: &str,
) -> Result<AssistantMessage, AgentError> {
    let agent = Agent::new(
        AgentConfig::default(),
        Box::new(FailingBenchProvider),
        Box::new(BenchMemory::default()),
        vec![],
    );
    agent
        .respond(
            UserMessage {
                text: message.to_string(),
            },
            &bench_ctx(),
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::{run_core_loop_iteration, run_core_loop_iteration_failure};
    use agentzero_core::AgentError;

    #[tokio::test]
    async fn core_loop_iteration_success_path() {
        let response = run_core_loop_iteration("hello")
            .await
            .expect("core loop iteration should succeed");
        assert_eq!(response.text, "ok");
    }

    #[tokio::test]
    async fn core_loop_iteration_negative_path_provider_failure() {
        let result = run_core_loop_iteration_failure("hello").await;
        match result {
            Err(AgentError::Provider { source }) => {
                assert!(source.to_string().contains("bench provider failure"));
            }
            other => panic!("expected provider failure, got {other:?}"),
        }
    }
}

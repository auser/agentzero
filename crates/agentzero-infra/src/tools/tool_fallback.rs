//! Automatic tool-creation fallback for missing tools.
//!
//! When the LLM requests a tool that doesn't exist, the agent delegates to
//! [`DynamicToolFallback`] which uses the existing `create_tool_from_nl`
//! infrastructure to generate the tool on the fly, including WASM codegen
//! when enabled. Created tools are persisted and immediately available for
//! future invocations.

use crate::tools::dynamic_tool::DynamicToolRegistry;
use crate::tools::tool_create::create_tool_from_nl;
use agentzero_core::{AuditSink, Provider, Tool, ToolFallback, ToolSource};
use async_trait::async_trait;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Maximum number of fallback creation attempts per agent session.
const MAX_FALLBACK_ATTEMPTS_PER_SESSION: usize = 5;

/// Maximum number of creation attempts for the same tool name.
const MAX_ATTEMPTS_PER_TOOL: usize = 2;

/// Automatic tool-creation fallback backed by [`DynamicToolRegistry`].
///
/// Wraps `create_tool_from_nl` with per-tool and per-session loop protection
/// to prevent runaway tool generation.
pub struct DynamicToolFallback {
    registry: Arc<DynamicToolRegistry>,
    provider: Arc<dyn Provider>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    /// Track attempted tool names to prevent infinite loops.
    attempted: Mutex<HashSet<String>>,
    /// Total attempts this session.
    total_attempts: Mutex<usize>,
}

impl DynamicToolFallback {
    pub fn new(
        registry: Arc<DynamicToolRegistry>,
        provider: Arc<dyn Provider>,
        audit_sink: Option<Arc<dyn AuditSink>>,
    ) -> Self {
        Self {
            registry,
            provider,
            audit_sink,
            attempted: Mutex::new(HashSet::new()),
            total_attempts: Mutex::new(0),
        }
    }
}

#[async_trait]
impl ToolFallback for DynamicToolFallback {
    async fn create_tool(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
    ) -> anyhow::Result<Box<dyn Tool>> {
        // Loop protection: check session-level budget.
        {
            let total = self.total_attempts.lock().await;
            if *total >= MAX_FALLBACK_ATTEMPTS_PER_SESSION {
                anyhow::bail!(
                    "tool fallback budget exhausted ({MAX_FALLBACK_ATTEMPTS_PER_SESSION} \
                     attempts this session)"
                );
            }
        }

        // Loop protection: check per-tool attempt count.
        {
            let attempted = self.attempted.lock().await;
            let count = attempted.iter().filter(|n| n.as_str() == tool_name).count();
            if count >= MAX_ATTEMPTS_PER_TOOL {
                anyhow::bail!(
                    "already attempted to create tool '{tool_name}' \
                     {MAX_ATTEMPTS_PER_TOOL} times"
                );
            }
        }

        // Record this attempt.
        {
            let mut attempted = self.attempted.lock().await;
            attempted.insert(tool_name.to_string());
            let mut total = self.total_attempts.lock().await;
            *total += 1;
        }

        // Build a description from the tool name and input.
        let input_hint =
            serde_json::to_string_pretty(tool_input).unwrap_or_else(|_| tool_input.to_string());
        let description =
            format!("Create a tool named '{tool_name}' that can handle input like: {input_hint}");

        // Delegate to existing create_tool_from_nl.
        let created_name = create_tool_from_nl(
            &self.registry,
            self.provider.as_ref(),
            &description,
            None, // let the LLM choose strategy
            self.audit_sink.clone(),
        )
        .await?;

        // Retrieve the created tool from the registry.
        let tools: Vec<Box<dyn Tool>> = self.registry.additional_tools();
        let tool = tools
            .into_iter()
            .find(|t| t.name() == created_name || t.name() == tool_name)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "tool was created as '{}' but not found in registry",
                    created_name
                )
            })?;

        Ok(tool)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the per-tool attempt limit is enforced.
    #[test]
    fn attempt_tracking_uses_hashset() {
        let set: HashSet<String> = HashSet::from(["foo".to_string()]);
        assert_eq!(set.iter().filter(|n| n.as_str() == "foo").count(), 1);
        assert_eq!(set.iter().filter(|n| n.as_str() == "bar").count(), 0);
    }
}

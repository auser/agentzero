//! Step dispatcher implementation for workflow execution in the gateway.
//!
//! Implements [`StepDispatcher`] to bridge the workflow executor with the
//! gateway's agent runtime infrastructure. Agent nodes are injected with a
//! `ConverseTool` so they can have multi-turn conversations with other agents
//! in the same workflow.

use agentzero_core::AgentEndpoint;
use agentzero_infra::runtime::{run_agent_once, RunAgentRequest};
use agentzero_orchestrator::workflow_executor::{
    ExecutionPlan, ExecutionStep, NodeType, StepDispatcher,
};
use agentzero_tools::ConverseTool;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Dispatches workflow steps through the gateway's real execution infrastructure.
///
/// For agent steps, uses `run_agent_once` with provider/model overrides from
/// the resolved workflow config, and injects a `ConverseTool` so agents in the
/// same workflow can talk to each other. For tool steps, looks up the tool from
/// the default toolset and executes directly. Channel sends are stubbed for now.
pub(crate) struct GatewayStepDispatcher {
    config_path: PathBuf,
    workspace_root: PathBuf,
    agent_store: Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
    /// Agent endpoints for all agent nodes in the workflow, keyed by node name.
    /// Injected into each agent via ConverseTool so they can converse.
    agent_endpoints: HashMap<String, Arc<dyn AgentEndpoint>>,
    /// Channel registry for dispatching messages to real platforms.
    channels: Arc<agentzero_channels::ChannelRegistry>,
    /// Gate resume senders: `(run_id, node_id) → oneshot::Sender<decision>`.
    gate_senders: crate::state::GateSenderMap,
    /// Run ID for this workflow execution (used as key for gate senders).
    run_id: String,
}

impl GatewayStepDispatcher {
    /// Create a dispatcher from the current gateway state and execution plan.
    ///
    /// Builds `WorkflowAgentEndpoint`s for every agent/subagent node in the
    /// plan so each agent can reach the others via `ConverseTool`.
    ///
    /// Returns `None` if the gateway was started without a config/workspace path
    /// (i.e. agent execution is not available).
    pub(crate) fn from_state(
        state: &crate::state::GatewayState,
        plan: &ExecutionPlan,
        run_id: String,
    ) -> Option<Self> {
        let config_path = state.config_path.as_ref()?.as_ref().clone();
        let workspace_root = state
            .workspace_root
            .as_ref()
            .map(|p| p.as_ref().clone())
            .unwrap_or_else(|| PathBuf::from("."));
        let agent_store = state
            .agent_store
            .as_ref()
            .map(|s| Arc::clone(s) as Arc<dyn agentzero_core::agent_store::AgentStoreApi>);

        // Build endpoints for all agent nodes in the workflow.
        let mut agent_endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = HashMap::new();
        for level in &plan.levels {
            for step in level {
                if matches!(step.node_type, NodeType::Agent | NodeType::SubAgent) {
                    let ep: Arc<dyn AgentEndpoint> = Arc::new(WorkflowAgentEndpoint {
                        agent_name: step.name.clone(),
                        config_path: config_path.clone(),
                        workspace_root: workspace_root.clone(),
                        provider: step.config.provider.clone(),
                        model: step.config.model.clone(),
                        role_description: step.config.role_description.clone(),
                        agent_store: agent_store.clone(),
                    });
                    agent_endpoints.insert(step.name.clone(), ep);
                }
            }
        }

        Some(Self {
            config_path,
            workspace_root,
            agent_store,
            agent_endpoints,
            channels: Arc::clone(&state.channels),
            gate_senders: Arc::clone(&state.gate_senders),
            run_id,
        })
    }
}

#[async_trait]
impl StepDispatcher for GatewayStepDispatcher {
    async fn run_agent(
        &self,
        step: &ExecutionStep,
        input: &str,
        context: Option<&serde_json::Value>,
    ) -> anyhow::Result<String> {
        let mut message = input.to_string();
        if let Some(ctx) = context {
            message = format!("Context: {ctx}\n\nTask: {input}");
        }

        // Apply role description as a prefix if resolved from a Role config node.
        if let Some(ref role_desc) = step.config.role_description {
            message = format!("Role: {role_desc}\n\n{message}");
        }

        // Build ConverseTool with endpoints to all other agents in the workflow.
        let mut extra_tools: Vec<Box<dyn agentzero_core::Tool>> = Vec::new();
        if self.agent_endpoints.len() > 1 {
            let peer_endpoints: HashMap<String, Arc<dyn AgentEndpoint>> = self
                .agent_endpoints
                .iter()
                .filter(|(name, _)| *name != &step.name)
                .map(|(name, ep)| (name.clone(), Arc::clone(ep)))
                .collect();

            if !peer_endpoints.is_empty() {
                let peer_names: Vec<String> = peer_endpoints.keys().cloned().collect();
                tracing::info!(
                    agent = %step.name,
                    peers = ?peer_names,
                    "injecting ConverseTool for workflow agent"
                );
                extra_tools.push(Box::new(
                    ConverseTool::new(peer_endpoints)
                        .with_max_turns(3)
                        .with_turn_timeout_secs(60),
                ));

                // Instruct the agent to actively converse with its peers.
                message = format!(
                    "{message}\n\n\
                     You are part of a multi-agent workflow. You have access to a `converse` tool \
                     that lets you send a message to another agent: {peers}. \
                     Use it only if you need specific input from a peer — do NOT have open-ended \
                     back-and-forth discussions. Limit yourself to at most 2 converse calls total. \
                     After gathering any needed input, write your final response directly.",
                    peers = peer_names.join(", ")
                );
            }
        }

        let req = RunAgentRequest {
            workspace_root: self.workspace_root.clone(),
            config_path: self.config_path.clone(),
            message,
            provider_override: step.config.provider.clone(),
            model_override: step.config.model.clone(),
            profile_override: None,
            extra_tools,
            conversation_id: None,
            agent_store: self.agent_store.clone(),
            // Workflow agents are ephemeral — no persistent memory needed.
            memory_override: Some(Box::new(agentzero_core::EphemeralMemory::default())),
        };

        let output = run_agent_once(req).await?;
        Ok(output.response_text)
    }

    async fn run_tool(&self, tool_name: &str, input: &serde_json::Value) -> anyhow::Result<String> {
        let policy =
            agentzero_config::load_tool_security_policy(&self.workspace_root, &self.config_path)?;
        let tools = agentzero_infra::tools::default_tools(&policy, None, None)?;

        let tool = tools
            .iter()
            .find(|t| t.name() == tool_name)
            .ok_or_else(|| anyhow::anyhow!("tool '{tool_name}' not found"))?;

        let ctx =
            agentzero_core::ToolContext::new(self.workspace_root.to_string_lossy().to_string());
        let result = tool.execute(&input.to_string(), &ctx).await?;
        Ok(result.output)
    }

    async fn send_channel(&self, channel_type: &str, message: &str) -> anyhow::Result<()> {
        let payload = serde_json::json!({
            "text": message,
            "content": message,
            "message": message,
        });

        match self.channels.dispatch(channel_type, payload).await {
            Some(delivery) if delivery.accepted => {
                tracing::info!(channel = channel_type, "workflow channel send dispatched");
                Ok(())
            }
            Some(_) => {
                tracing::warn!(channel = channel_type, "channel rejected message");
                anyhow::bail!("channel '{channel_type}' rejected the message")
            }
            None => {
                tracing::warn!(channel = channel_type, "channel not found or offline");
                anyhow::bail!("channel '{channel_type}' not found in registry")
            }
        }
    }

    async fn suspend_gate(&self, _run_id: &str, node_id: &str, node_name: &str) -> String {
        let (tx, rx) = tokio::sync::oneshot::channel();

        // Store the sender so the resume endpoint can send the decision.
        {
            let mut senders = self.gate_senders.lock().expect("gate_senders lock");
            senders.insert((self.run_id.clone(), node_id.to_string()), tx);
        }

        tracing::info!(
            run_id = %self.run_id,
            node_id = %node_id,
            node_name = %node_name,
            "gate suspended — waiting for human decision via POST /v1/workflows/runs/:run_id/resume"
        );

        // Emit a structured approval event for monitoring/notification systems.
        // External integrations (Slack bots, email hooks) can subscribe to the
        // EventBus "approval.requested" topic or watch structured logs.
        tracing::info!(
            target: "approval",
            run_id = %self.run_id,
            node_id = %node_id,
            node_name = %node_name,
            resume_url = %format!("/v1/workflows/runs/{}/resume", self.run_id),
            "approval requested — gate awaiting human decision"
        );

        // Block until the resume endpoint sends a decision, or timeout.
        // Default timeout: 24 hours.
        let timeout = std::time::Duration::from_secs(24 * 60 * 60);

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => {
                tracing::info!(
                    run_id = %self.run_id,
                    node_id = %node_id,
                    decision = %decision,
                    "gate resumed"
                );
                decision
            }
            Ok(Err(_)) => {
                // Sender dropped (run cancelled) — auto-deny.
                tracing::warn!(
                    node_id = %node_id,
                    "gate resume channel closed — auto-denying"
                );
                "denied".to_string()
            }
            Err(_) => {
                // Timeout expired — auto-deny and clean up the sender.
                {
                    let mut senders = self.gate_senders.lock().expect("gate_senders lock");
                    senders.remove(&(self.run_id.clone(), node_id.to_string()));
                }
                tracing::warn!(
                    run_id = %self.run_id,
                    node_id = %node_id,
                    timeout_secs = timeout.as_secs(),
                    "gate timed out — auto-denying"
                );
                "denied".to_string()
            }
        }
    }
}

// ── Workflow Agent Endpoint ─────────────────────────────────────────────────

/// An [`AgentEndpoint`] that runs a workflow agent via `run_agent_once`.
///
/// Used by `ConverseTool` so that when agent A calls `converse(agent="B", ...)`,
/// it actually executes agent B with the message and returns the response.
struct WorkflowAgentEndpoint {
    agent_name: String,
    config_path: PathBuf,
    workspace_root: PathBuf,
    provider: Option<String>,
    model: Option<String>,
    role_description: Option<String>,
    agent_store: Option<Arc<dyn agentzero_core::agent_store::AgentStoreApi>>,
}

#[async_trait]
impl AgentEndpoint for WorkflowAgentEndpoint {
    async fn send(&self, message: &str, _conversation_id: &str) -> anyhow::Result<String> {
        let mut full_message = message.to_string();
        if let Some(ref role_desc) = self.role_description {
            full_message = format!("Role: {role_desc}\n\n{full_message}");
        }

        let req = RunAgentRequest {
            workspace_root: self.workspace_root.clone(),
            config_path: self.config_path.clone(),
            message: full_message,
            provider_override: self.provider.clone(),
            model_override: self.model.clone(),
            profile_override: None,
            extra_tools: vec![],
            conversation_id: None,
            agent_store: self.agent_store.clone(),
            memory_override: Some(Box::new(agentzero_core::EphemeralMemory::default())),
        };

        let output = run_agent_once(req).await?;
        Ok(output.response_text)
    }

    fn agent_id(&self) -> &str {
        &self.agent_name
    }
}

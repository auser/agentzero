use crate::api_keys::ApiKeyStore;
use crate::gateway_channel::GatewayChannel;
use crate::token_store::save_paired_tokens;
use agentzero_channels::pipeline::PerplexityFilterSettings;
use agentzero_channels::ChannelRegistry;
use agentzero_config::AgentZeroConfig;
use agentzero_core::canvas::CanvasStore;
use agentzero_core::{EventBus, MemoryStore};
use agentzero_orchestrator::{
    AgentStore, JobStore, NodeStatus, PresenceStore, TemplateStore, WorkflowStore,
};

/// Type alias for the gate resume sender map to avoid clippy type_complexity.
pub(crate) type GateSenderMap =
    Arc<Mutex<HashMap<(String, String), tokio::sync::oneshot::Sender<String>>>>;

use serde::Serialize;

/// Snapshot of a workflow run's state, updated in real-time by the executor.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WorkflowRunState {
    pub run_id: String,
    pub workflow_id: String,
    pub status: String,
    pub node_statuses: HashMap<String, NodeStatus>,
    /// Per-node output text (populated on completion).
    pub node_outputs: HashMap<String, String>,
    /// Flattened outputs: "node_id:port" → value.
    pub outputs: HashMap<String, serde_json::Value>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub error: Option<String>,
}
use metrics_exporter_prometheus::PrometheusHandle;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct GatewayState {
    pub(crate) service_name: Arc<String>,
    pub(crate) bearer_token: Option<Arc<String>>,
    pub(crate) channels: Arc<ChannelRegistry>,
    pub(crate) pairing_code: Option<Arc<String>>,
    pub(crate) paired_tokens: Arc<Mutex<HashSet<String>>>,
    /// Paired token creation timestamps (token → epoch seconds).
    pub(crate) paired_token_timestamps: Arc<Mutex<HashMap<String, u64>>>,
    /// Session TTL for paired tokens in seconds. `None` = no expiry.
    pub(crate) session_ttl_secs: Option<u64>,
    pub(crate) otp_secret: Arc<String>,
    pub(crate) token_store_path: Option<Arc<PathBuf>>,
    pub(crate) perplexity_filter: Arc<PerplexityFilterSettings>,
    /// Pairing code creation timestamp (for TTL-based expiry).
    pub(crate) pairing_created_at: Instant,
    /// How many seconds the pairing code is valid. `None` = no expiry.
    pub(crate) pairing_ttl_secs: Option<u64>,
    /// Require pairing flow (from `[gateway]` config).
    pub(crate) require_pairing: bool,
    /// Allow binding to non-loopback interfaces (from `[gateway]` config).
    pub(crate) allow_public_bind: bool,
    /// Path to the agentzero config file (for building agent runtime).
    pub(crate) config_path: Option<Arc<PathBuf>>,
    /// Workspace root directory.
    pub(crate) workspace_root: Option<Arc<PathBuf>>,
    /// Prometheus metrics render handle.
    pub(crate) prometheus_handle: Arc<PrometheusHandle>,
    /// Noise session store for E2E encrypted communication.
    #[cfg(feature = "privacy")]
    pub(crate) noise_sessions: Option<Arc<crate::privacy_state::NoiseSessionStore>>,
    /// Server's Noise static keypair.
    #[cfg(feature = "privacy")]
    pub(crate) noise_keypair: Option<agentzero_core::privacy::noise::NoiseKeypair>,
    /// In-progress Noise handshakes.
    #[cfg(feature = "privacy")]
    pub(crate) noise_handshakes: Option<crate::noise_handshake::HandshakeMap>,
    /// Relay mode: when true, only relay routes are active.
    #[cfg(feature = "privacy")]
    pub(crate) relay_mode: bool,
    /// Relay mailbox for sealed envelopes.
    #[cfg(feature = "privacy")]
    pub(crate) relay_mailbox: Option<Arc<crate::relay::RelayMailbox>>,
    /// Live config receiver for hot-reload. When present, accessor methods
    /// read from this instead of static fields.
    pub(crate) live_config: Option<watch::Receiver<AgentZeroConfig>>,
    /// Async job store for tracking /v1/runs submissions.
    pub(crate) job_store: Option<Arc<JobStore>>,
    /// Agent presence tracking for /v1/agents endpoint.
    pub(crate) presence_store: Option<Arc<PresenceStore>>,
    /// Shared memory store for transcript retrieval.
    pub(crate) memory_store: Option<Arc<dyn MemoryStore>>,
    /// Gateway channel for bridging API requests into the swarm pipeline.
    pub(crate) gateway_channel: Option<Arc<GatewayChannel>>,
    /// API key store for scope-based authorization.
    pub(crate) api_key_store: Option<Arc<ApiKeyStore>>,
    /// Distributed event bus for real-time event streaming.
    pub(crate) event_bus: Option<Arc<dyn EventBus>>,
    /// Dynamic agent store for runtime agent CRUD.
    pub(crate) agent_store: Option<Arc<AgentStore>>,
    /// MCP Server for tool execution (used by /v1/tool-execute and /mcp/*).
    pub(crate) mcp_server: Option<Arc<agentzero_infra::mcp_server::McpServer>>,
    /// A2A task store for Agent-to-Agent protocol.
    pub(crate) a2a_tasks: crate::a2a::A2aTaskStore,
    /// Canvas store for live canvas rendering.
    pub(crate) canvas_store: Option<Arc<CanvasStore>>,
    /// Workflow store for visual workflow definitions.
    pub(crate) workflow_store: Option<Arc<WorkflowStore>>,
    /// Template store for reusable workflow templates.
    pub(crate) template_store: Option<Arc<TemplateStore>>,
    /// In-flight workflow runs — updated in real-time as nodes execute.
    pub(crate) workflow_runs: Arc<Mutex<HashMap<String, WorkflowRunState>>>,
    /// Gate resume channels: `(run_id, node_id) → oneshot::Sender<decision>`.
    /// Used by the resume endpoint to unblock suspended gate nodes.
    pub(crate) gate_senders: GateSenderMap,
    /// Dynamic tool registry for sharing tools via the API.
    pub(crate) dynamic_tool_registry:
        Option<Arc<agentzero_infra::tools::dynamic_tool::DynamicToolRegistry>>,
    /// Recipe store for sharing tool recipes via the API.
    #[allow(dead_code)]
    pub(crate) recipe_store:
        Option<Arc<std::sync::Mutex<agentzero_infra::tool_recipes::RecipeStore>>>,
    /// Configurable WebSocket timeouts.
    pub(crate) ws_config: agentzero_config::WebSocketConfig,
}

impl GatewayState {
    pub(crate) fn new(
        pairing_code: Option<String>,
        otp_secret: String,
        paired_tokens: HashSet<String>,
        token_store_path: Option<PathBuf>,
        prometheus_handle: PrometheusHandle,
    ) -> Self {
        Self {
            service_name: Arc::new("agentzero-gateway".to_string()),
            bearer_token: std::env::var("AGENTZERO_GATEWAY_BEARER_TOKEN")
                .ok()
                .map(|token| Arc::new(token.trim().to_string()))
                .filter(|token| !token.is_empty()),
            channels: Arc::new(ChannelRegistry::with_builtin_handlers()),
            pairing_code: pairing_code.map(Arc::new),
            paired_tokens: Arc::new(Mutex::new(paired_tokens)),
            paired_token_timestamps: Arc::new(Mutex::new(HashMap::new())),
            session_ttl_secs: None,
            otp_secret: Arc::new(otp_secret),
            token_store_path: token_store_path.map(Arc::new),
            perplexity_filter: Arc::new(PerplexityFilterSettings::default()),
            pairing_created_at: Instant::now(),
            pairing_ttl_secs: None,
            require_pairing: true,
            allow_public_bind: false,
            config_path: None,
            workspace_root: None,
            prometheus_handle: Arc::new(prometheus_handle),
            #[cfg(feature = "privacy")]
            noise_sessions: None,
            #[cfg(feature = "privacy")]
            noise_keypair: None,
            #[cfg(feature = "privacy")]
            noise_handshakes: None,
            #[cfg(feature = "privacy")]
            relay_mode: false,
            #[cfg(feature = "privacy")]
            relay_mailbox: None,
            live_config: None,
            job_store: None,
            presence_store: None,
            memory_store: None,
            gateway_channel: None,
            api_key_store: None,
            event_bus: None,
            agent_store: None,
            mcp_server: None,
            a2a_tasks: crate::a2a::A2aTaskStore::new(),
            canvas_store: None,
            workflow_store: None,
            template_store: None,
            workflow_runs: Arc::new(Mutex::new(HashMap::new())),
            gate_senders: Arc::new(Mutex::new(HashMap::new())),
            dynamic_tool_registry: None,
            recipe_store: None,
            ws_config: agentzero_config::WebSocketConfig::default(),
        }
    }

    /// Attach a dynamic tool registry for tool sharing.
    #[allow(dead_code)]
    pub(crate) fn with_dynamic_tool_registry(
        mut self,
        registry: Arc<agentzero_infra::tools::dynamic_tool::DynamicToolRegistry>,
    ) -> Self {
        self.dynamic_tool_registry = Some(registry);
        self
    }

    /// Attach a recipe store for recipe sharing.
    #[allow(dead_code)]
    pub(crate) fn with_recipe_store(
        mut self,
        store: Arc<std::sync::Mutex<agentzero_infra::tool_recipes::RecipeStore>>,
    ) -> Self {
        self.recipe_store = Some(store);
        self
    }

    /// Set the canvas store for live canvas rendering.
    #[allow(dead_code)]
    pub(crate) fn with_canvas_store(mut self, store: Arc<CanvasStore>) -> Self {
        self.canvas_store = Some(store);
        self
    }

    /// Set the workflow store for visual workflow definitions.
    #[allow(dead_code)]
    pub(crate) fn with_workflow_store(mut self, store: Arc<WorkflowStore>) -> Self {
        self.workflow_store = Some(store);
        self
    }

    /// Set the template store for reusable workflow templates.
    #[allow(dead_code)]
    pub(crate) fn with_template_store(mut self, store: Arc<TemplateStore>) -> Self {
        self.template_store = Some(store);
        self
    }

    /// Set the distributed event bus for real-time event streaming.
    #[allow(dead_code)]
    pub(crate) fn with_event_bus(mut self, bus: Arc<dyn EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    /// Set the dynamic agent store for runtime agent CRUD.
    #[allow(dead_code)]
    pub(crate) fn with_agent_store(mut self, store: Arc<AgentStore>) -> Self {
        self.agent_store = Some(store);
        self
    }

    /// Set the API key store for scope-based authorization.
    /// Used when wiring up the gateway with multi-tenant API key management.
    #[allow(dead_code)]
    pub(crate) fn with_api_key_store(mut self, store: Arc<ApiKeyStore>) -> Self {
        self.api_key_store = Some(store);
        self
    }

    /// Configure Noise Protocol privacy for E2E encrypted communication.
    #[cfg(feature = "privacy")]
    pub(crate) fn with_noise_privacy(
        mut self,
        sessions: Arc<crate::privacy_state::NoiseSessionStore>,
        keypair: agentzero_core::privacy::noise::NoiseKeypair,
    ) -> Self {
        self.noise_sessions = Some(sessions);
        self.noise_keypair = Some(keypair);
        self.noise_handshakes = Some(std::sync::Arc::new(dashmap::DashMap::new()));
        self
    }

    /// Enable relay mode with a mailbox for sealed envelope routing.
    #[cfg(feature = "privacy")]
    pub(crate) fn with_relay_mode(mut self, mailbox: Arc<crate::relay::RelayMailbox>) -> Self {
        self.relay_mode = true;
        self.relay_mailbox = Some(mailbox);
        self
    }

    /// Set the pairing code TTL. After `ttl_secs` seconds, `pairing_code_valid()`
    /// returns `None` even if a code was set.
    #[cfg(test)]
    pub(crate) fn with_pairing_ttl(mut self, ttl_secs: u64) -> Self {
        self.pairing_ttl_secs = Some(ttl_secs);
        self
    }

    /// Set session TTL for paired tokens (in seconds).
    /// Tokens older than this are rejected. `None` = no expiry.
    #[allow(dead_code)]
    pub(crate) fn with_session_ttl(mut self, ttl_secs: u64) -> Self {
        self.session_ttl_secs = Some(ttl_secs);
        self
    }

    /// Set gateway config fields from loaded config.
    pub(crate) fn with_gateway_config(
        mut self,
        require_pairing: bool,
        allow_public_bind: bool,
    ) -> Self {
        self.require_pairing = require_pairing;
        self.allow_public_bind = allow_public_bind;
        self
    }

    /// Set WebSocket configuration from the gateway config.
    pub(crate) fn with_ws_config(mut self, ws_config: agentzero_config::WebSocketConfig) -> Self {
        self.ws_config = ws_config;
        self
    }

    /// Set workspace paths for agent runtime execution.
    pub(crate) fn with_agent_paths(
        mut self,
        config_path: PathBuf,
        workspace_root: PathBuf,
    ) -> Self {
        self.config_path = Some(Arc::new(config_path));
        self.workspace_root = Some(Arc::new(workspace_root));
        self
    }

    /// Set the async job store for /v1/runs endpoints.
    #[allow(dead_code)]
    pub(crate) fn with_job_store(mut self, store: Arc<JobStore>) -> Self {
        self.job_store = Some(store);
        self
    }

    /// Set the shared memory store for transcript retrieval.
    pub(crate) fn with_memory_store(mut self, store: Arc<dyn MemoryStore>) -> Self {
        self.memory_store = Some(store);
        self
    }

    /// Set the gateway channel for swarm pipeline integration.
    pub(crate) fn with_gateway_channel(mut self, ch: Arc<GatewayChannel>) -> Self {
        self.gateway_channel = Some(ch);
        self
    }

    /// Return the current pairing code if it exists and hasn't expired.
    pub(crate) fn pairing_code_valid(&self) -> Option<&str> {
        let code = self.pairing_code.as_deref()?;
        if let Some(ttl) = self.pairing_ttl_secs {
            if self.pairing_created_at.elapsed().as_secs() >= ttl {
                return None;
            }
        }
        Some(code)
    }

    pub(crate) fn with_perplexity_filter(mut self, settings: PerplexityFilterSettings) -> Self {
        self.perplexity_filter = Arc::new(settings);
        self
    }

    /// Attach a live config receiver for hot-reload support.
    pub(crate) fn with_live_config(mut self, rx: watch::Receiver<AgentZeroConfig>) -> Self {
        self.live_config = Some(rx);
        self
    }

    /// Read `require_pairing` from live config if available, otherwise use the static field.
    #[allow(dead_code)]
    pub(crate) fn effective_require_pairing(&self) -> bool {
        self.live_config
            .as_ref()
            .map(|rx| rx.borrow().gateway.require_pairing)
            .unwrap_or(self.require_pairing)
    }

    /// Read perplexity filter settings from live config if available, otherwise use the static field.
    pub(crate) fn effective_perplexity_filter(&self) -> PerplexityFilterSettings {
        self.live_config
            .as_ref()
            .map(|rx| {
                let pf = &rx.borrow().security.perplexity_filter;
                PerplexityFilterSettings {
                    enabled: pf.enable_perplexity_filter,
                    perplexity_threshold: pf.perplexity_threshold,
                    suffix_window_chars: pf.suffix_window_chars,
                    min_prompt_chars: pf.min_prompt_chars,
                    symbol_ratio_threshold: pf.symbol_ratio_threshold,
                }
            })
            .unwrap_or_else(|| (*self.perplexity_filter).clone())
    }

    pub(crate) fn add_paired_token(&self, token: String) -> anyhow::Result<()> {
        let path = self.token_store_path.as_deref().map(PathBuf::as_path);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Record the creation timestamp for session TTL enforcement.
        self.paired_token_timestamps
            .lock()
            .expect("token timestamp lock poisoned")
            .insert(token.clone(), now);
        let mut guard = self.paired_tokens.lock().expect("pairing lock poisoned");
        guard.insert(token);
        save_paired_tokens(path, &guard)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_prometheus_handle() -> PrometheusHandle {
        // Build without installing a global recorder — metrics macros become
        // no-ops in tests, but the handle can still render (empty output).
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        recorder.handle()
    }

    #[cfg(test)]
    pub(crate) fn test_with_bearer(token: Option<&str>) -> Self {
        Self {
            service_name: Arc::new("agentzero-gateway".to_string()),
            bearer_token: token.map(|value| Arc::new(value.to_string())),
            channels: Arc::new(ChannelRegistry::with_builtin_handlers()),
            pairing_code: Some(Arc::new("406823".to_string())),
            paired_tokens: Arc::new(Mutex::new(HashSet::new())),
            paired_token_timestamps: Arc::new(Mutex::new(HashMap::new())),
            session_ttl_secs: None,
            otp_secret: Arc::new("OTPSECRET".to_string()),
            token_store_path: None,
            perplexity_filter: Arc::new(PerplexityFilterSettings::default()),
            pairing_created_at: Instant::now(),
            pairing_ttl_secs: None,
            require_pairing: true,
            allow_public_bind: false,
            config_path: None,
            workspace_root: None,
            prometheus_handle: Arc::new(Self::test_prometheus_handle()),
            #[cfg(feature = "privacy")]
            noise_sessions: None,
            #[cfg(feature = "privacy")]
            noise_keypair: None,
            #[cfg(feature = "privacy")]
            noise_handshakes: None,
            #[cfg(feature = "privacy")]
            relay_mode: false,
            #[cfg(feature = "privacy")]
            relay_mailbox: None,
            live_config: None,
            job_store: None,
            presence_store: None,
            memory_store: None,
            gateway_channel: None,
            api_key_store: None,
            event_bus: None,
            agent_store: None,
            mcp_server: None,
            a2a_tasks: crate::a2a::A2aTaskStore::new(),
            canvas_store: None,
            workflow_store: None,
            template_store: None,
            workflow_runs: Arc::new(Mutex::new(HashMap::new())),
            gate_senders: Arc::new(Mutex::new(HashMap::new())),
            dynamic_tool_registry: None,
            recipe_store: None,
            ws_config: agentzero_config::WebSocketConfig::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_with_existing_pair(token: &str) -> Self {
        let mut paired_tokens = HashSet::new();
        paired_tokens.insert(token.to_string());
        Self {
            service_name: Arc::new("agentzero-gateway".to_string()),
            bearer_token: None,
            channels: Arc::new(ChannelRegistry::with_builtin_handlers()),
            pairing_code: None,
            paired_tokens: Arc::new(Mutex::new(paired_tokens)),
            paired_token_timestamps: Arc::new(Mutex::new(HashMap::new())),
            session_ttl_secs: None,
            otp_secret: Arc::new("OTPSECRET".to_string()),
            token_store_path: None,
            perplexity_filter: Arc::new(PerplexityFilterSettings::default()),
            pairing_created_at: Instant::now(),
            pairing_ttl_secs: None,
            require_pairing: true,
            allow_public_bind: false,
            config_path: None,
            workspace_root: None,
            prometheus_handle: Arc::new(Self::test_prometheus_handle()),
            #[cfg(feature = "privacy")]
            noise_sessions: None,
            #[cfg(feature = "privacy")]
            noise_keypair: None,
            #[cfg(feature = "privacy")]
            noise_handshakes: None,
            #[cfg(feature = "privacy")]
            relay_mode: false,
            #[cfg(feature = "privacy")]
            relay_mailbox: None,
            live_config: None,
            job_store: None,
            presence_store: None,
            memory_store: None,
            gateway_channel: None,
            api_key_store: None,
            event_bus: None,
            agent_store: None,
            mcp_server: None,
            a2a_tasks: crate::a2a::A2aTaskStore::new(),
            canvas_store: None,
            workflow_store: None,
            template_store: None,
            workflow_runs: Arc::new(Mutex::new(HashMap::new())),
            gate_senders: Arc::new(Mutex::new(HashMap::new())),
            dynamic_tool_registry: None,
            recipe_store: None,
            ws_config: agentzero_config::WebSocketConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_valid_returns_code_when_no_ttl() {
        let state = GatewayState::test_with_bearer(None);
        assert_eq!(state.pairing_code_valid(), Some("406823"));
    }

    #[test]
    fn pairing_code_valid_returns_none_after_ttl_expires() {
        let mut state = GatewayState::test_with_bearer(None);
        state.pairing_ttl_secs = Some(0); // Immediately expired
        assert!(state.pairing_code_valid().is_none());
    }

    #[test]
    fn pairing_code_valid_returns_none_when_no_code() {
        let state = GatewayState::test_with_existing_pair("tok");
        assert!(state.pairing_code_valid().is_none());
    }

    #[test]
    fn with_gateway_config_sets_fields() {
        let state = GatewayState::test_with_bearer(None).with_gateway_config(false, true);
        assert!(!state.require_pairing);
        assert!(state.allow_public_bind);
    }

    #[test]
    fn with_pairing_ttl_sets_expiry() {
        let state = GatewayState::test_with_bearer(None).with_pairing_ttl(300);
        assert_eq!(state.pairing_ttl_secs, Some(300));
        // Code should still be valid (just created).
        assert!(state.pairing_code_valid().is_some());
    }
}

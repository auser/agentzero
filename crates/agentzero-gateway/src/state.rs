use crate::token_store::save_paired_tokens;
use agentzero_channels::pipeline::PerplexityFilterSettings;
use agentzero_channels::ChannelRegistry;
use agentzero_config::AgentZeroConfig;
use agentzero_core::MemoryStore;
use agentzero_orchestrator::{JobStore, PresenceStore};
use metrics_exporter_prometheus::PrometheusHandle;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio::sync::watch;

#[derive(Clone)]
pub(crate) struct GatewayState {
    pub(crate) service_name: Arc<String>,
    pub(crate) bearer_token: Option<Arc<String>>,
    pub(crate) channels: Arc<ChannelRegistry>,
    pub(crate) pairing_code: Option<Arc<String>>,
    pub(crate) paired_tokens: Arc<Mutex<HashSet<String>>>,
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
        }
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

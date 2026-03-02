use crate::token_store::save_paired_tokens;
use agentzero_channels::pipeline::PerplexityFilterSettings;
use agentzero_channels::ChannelRegistry;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Instant,
};

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
    #[allow(dead_code)]
    pub(crate) pairing_created_at: Instant,
    /// How many seconds the pairing code is valid. `None` = no expiry.
    #[allow(dead_code)]
    pub(crate) pairing_ttl_secs: Option<u64>,
    /// Require pairing flow (from `[gateway]` config).
    #[allow(dead_code)]
    pub(crate) require_pairing: bool,
    /// Allow binding to non-loopback interfaces (from `[gateway]` config).
    #[allow(dead_code)]
    pub(crate) allow_public_bind: bool,
}

impl GatewayState {
    pub(crate) fn new(
        pairing_code: Option<String>,
        otp_secret: String,
        paired_tokens: HashSet<String>,
        token_store_path: Option<PathBuf>,
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
        }
    }

    /// Set the pairing code TTL. After `ttl_secs` seconds, `pairing_code_valid()`
    /// returns `None` even if a code was set.
    #[allow(dead_code)]
    pub(crate) fn with_pairing_ttl(mut self, ttl_secs: u64) -> Self {
        self.pairing_ttl_secs = Some(ttl_secs);
        self
    }

    /// Set gateway config fields from loaded config.
    #[allow(dead_code)]
    pub(crate) fn with_gateway_config(
        mut self,
        require_pairing: bool,
        allow_public_bind: bool,
    ) -> Self {
        self.require_pairing = require_pairing;
        self.allow_public_bind = allow_public_bind;
        self
    }

    /// Return the current pairing code if it exists and hasn't expired.
    #[allow(dead_code)]
    pub(crate) fn pairing_code_valid(&self) -> Option<&str> {
        let code = self.pairing_code.as_deref()?;
        if let Some(ttl) = self.pairing_ttl_secs {
            if self.pairing_created_at.elapsed().as_secs() >= ttl {
                return None;
            }
        }
        Some(code)
    }

    #[allow(dead_code)]
    pub(crate) fn with_perplexity_filter(mut self, settings: PerplexityFilterSettings) -> Self {
        self.perplexity_filter = Arc::new(settings);
        self
    }

    pub(crate) fn add_paired_token(&self, token: String) -> anyhow::Result<()> {
        let path = self.token_store_path.as_deref().map(PathBuf::as_path);
        let mut guard = self.paired_tokens.lock().expect("pairing lock poisoned");
        guard.insert(token);
        save_paired_tokens(path, &guard)?;
        Ok(())
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

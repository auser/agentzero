use crate::token_store::save_paired_tokens;
use agentzero_channels::ChannelRegistry;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
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
        }
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
        }
    }
}

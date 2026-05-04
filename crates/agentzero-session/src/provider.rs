use agentzero_core::DataClassification;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ModelProviderError {
    #[error("model call denied: {0}")]
    Denied(String),
    #[error("model unavailable: {0}")]
    Unavailable(String),
    #[error("model error: {0}")]
    Failed(String),
}

/// Whether a model provider is local or remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLocation {
    Local,
    Remote,
}

/// A model provider that can generate completions.
///
/// Implementations are added per-provider. This defines the contract.
pub trait ModelProvider: Send + Sync {
    /// Human-readable name of the provider.
    fn name(&self) -> &str;

    /// Whether this provider runs locally or remotely.
    fn location(&self) -> ModelLocation;

    /// Check whether a given data classification is safe to send to this provider.
    fn accepts_classification(&self, classification: DataClassification) -> bool {
        match self.location() {
            ModelLocation::Local => true,
            ModelLocation::Remote => classification.allows_remote_unredacted(),
        }
    }
}

/// A stub local model provider for testing and demo purposes.
///
/// Always returns a canned response. No network calls.
pub struct LocalStubProvider;

impl ModelProvider for LocalStubProvider {
    fn name(&self) -> &str {
        "local-stub"
    }

    fn location(&self) -> ModelLocation {
        ModelLocation::Local
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_provider_accepts_all_classifications() {
        let provider = LocalStubProvider;
        assert!(provider.accepts_classification(DataClassification::Secret));
        assert!(provider.accepts_classification(DataClassification::Pii));
        assert!(provider.accepts_classification(DataClassification::Private));
        assert!(provider.accepts_classification(DataClassification::Public));
    }

    #[test]
    fn local_stub_is_local() {
        let provider = LocalStubProvider;
        assert_eq!(provider.location(), ModelLocation::Local);
        assert_eq!(provider.name(), "local-stub");
    }
}

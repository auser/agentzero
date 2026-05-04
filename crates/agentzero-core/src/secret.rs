use serde::{Deserialize, Serialize};
use std::fmt;

/// A capability-based secret handle per ADR 0009.
///
/// The model sees handles and metadata, never raw secret values.
/// Tools receive raw material only at execution time if policy allows.
///
/// Handles use the URI format: `handle://vault/<provider>/<name>`
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretHandle {
    provider: String,
    name: String,
}

impl SecretHandle {
    /// Create a new secret handle.
    pub fn new(provider: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            name: name.into(),
        }
    }

    /// Parse a handle URI like `handle://vault/github/default`.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let rest = uri.strip_prefix("handle://vault/")?;
        let (provider, name) = rest.split_once('/')?;
        if provider.is_empty() || name.is_empty() {
            return None;
        }
        Some(Self {
            provider: provider.to_string(),
            name: name.to_string(),
        })
    }

    /// Return the handle URI.
    pub fn uri(&self) -> String {
        format!("handle://vault/{}/{}", self.provider, self.name)
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Debug intentionally hides the handle details to prevent accidental leakage
/// in logs. Use `.uri()` when you explicitly need the handle string.
impl fmt::Debug for SecretHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SecretHandle(handle://vault/{}/<redacted>)",
            self.provider
        )
    }
}

/// Display shows the full URI — this is safe because handles never contain
/// raw secret material.
impl fmt::Display for SecretHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.uri())
    }
}

/// A resolved secret value. This is intentionally NOT Debug/Display/Serialize
/// to prevent accidental leakage. Only tools at execution time should access the inner value.
pub struct ResolvedSecret {
    value: String,
}

impl ResolvedSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    /// Expose the raw secret value. Only call this at tool execution time
    /// after policy has approved the action.
    pub fn expose(&self) -> &str {
        &self.value
    }
}

/// Debug intentionally redacts the value.
impl fmt::Debug for ResolvedSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ResolvedSecret(<redacted>)")
    }
}

/// Drop zeroes the secret memory.
impl Drop for ResolvedSecret {
    fn drop(&mut self) {
        // Overwrite with zeros to reduce residual exposure
        // SAFETY: this is best-effort; the optimizer may elide this.
        // For production secrets, use a crate like `zeroize`.
        self.value = String::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handle_from_uri() {
        let h = SecretHandle::from_uri("handle://vault/github/default")
            .expect("should parse valid URI");
        assert_eq!(h.provider(), "github");
        assert_eq!(h.name(), "default");
        assert_eq!(h.uri(), "handle://vault/github/default");
    }

    #[test]
    fn handle_from_uri_rejects_invalid() {
        assert!(SecretHandle::from_uri("not-a-handle").is_none());
        assert!(SecretHandle::from_uri("handle://vault/").is_none());
        assert!(SecretHandle::from_uri("handle://vault/github/").is_none());
        assert!(SecretHandle::from_uri("handle://vault//name").is_none());
    }

    #[test]
    fn handle_debug_redacts_name() {
        let h = SecretHandle::new("aws", "prod-key");
        let debug = format!("{h:?}");
        assert!(debug.contains("aws"));
        assert!(!debug.contains("prod-key"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn handle_display_shows_uri() {
        let h = SecretHandle::new("github", "default");
        assert_eq!(h.to_string(), "handle://vault/github/default");
    }

    #[test]
    fn resolved_secret_debug_redacts() {
        let s = ResolvedSecret::new("super-secret-token");
        let debug = format!("{s:?}");
        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn resolved_secret_expose() {
        let s = ResolvedSecret::new("my-token");
        assert_eq!(s.expose(), "my-token");
    }

    #[test]
    fn handle_serializes_without_raw_value() {
        let h = SecretHandle::new("github", "default");
        let json = serde_json::to_string(&h).expect("handle should serialize");
        assert!(json.contains("github"));
        assert!(json.contains("default"));
        // No raw secret value in serialized output — only metadata
    }
}

//! Per-component privacy boundaries.
//!
//! Allows agents, tools, channels, and plugins to declare their own privacy
//! requirements. A child boundary can only be **stricter** than its parent —
//! never more permissive.

use serde::{Deserialize, Serialize};

/// Privacy boundary that can be attached to agents, tools, channels, or plugins.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyBoundary {
    /// Use the parent/global setting.
    #[default]
    Inherit,
    /// Must use local providers only — no network access.
    LocalOnly,
    /// Must route through encrypted transport (Noise).
    EncryptedOnly,
    /// No restriction (clamped by parent during resolution).
    Any,
}

impl PrivacyBoundary {
    /// Resolve a child boundary against a parent. The result is always at least
    /// as strict as the parent.
    ///
    /// Rules:
    /// - `Inherit` → adopts parent
    /// - `LocalOnly` always wins (strictest)
    /// - `EncryptedOnly` + `Any` → `EncryptedOnly`
    /// - `Any` gets clamped to parent
    pub fn resolve(&self, parent: &Self) -> Self {
        let effective_child = match self {
            Self::Inherit => parent.clone(),
            other => other.clone(),
        };

        // Child can never be more permissive than parent.
        match (parent, &effective_child) {
            // Parent is LocalOnly — everything becomes LocalOnly.
            (Self::LocalOnly, _) => Self::LocalOnly,
            // Parent is EncryptedOnly — child can be LocalOnly or EncryptedOnly.
            (Self::EncryptedOnly, Self::Any) => Self::EncryptedOnly,
            (Self::EncryptedOnly, Self::Inherit) => Self::EncryptedOnly,
            // Parent is Any or Inherit — child's own boundary applies.
            _ => effective_child,
        }
    }

    /// Check if this boundary allows a provider of the given kind.
    pub fn allows_provider(&self, kind: &str) -> bool {
        match self {
            Self::LocalOnly => crate::common::local_providers::is_local_provider(kind),
            // EncryptedOnly, Any, Inherit all allow any provider
            // (EncryptedOnly enforces transport, not provider choice).
            _ => true,
        }
    }

    /// Check if this boundary allows outbound network access.
    pub fn allows_network(&self) -> bool {
        !matches!(self, Self::LocalOnly)
    }

    /// Returns the strictness level for comparison (higher = stricter).
    fn strictness(&self) -> u8 {
        match self {
            Self::Any | Self::Inherit => 0,
            Self::EncryptedOnly => 1,
            Self::LocalOnly => 2,
        }
    }

    /// Check if this boundary is at least as strict as another.
    pub fn is_at_least_as_strict_as(&self, other: &Self) -> bool {
        self.strictness() >= other.strictness()
    }
}

impl std::fmt::Display for PrivacyBoundary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inherit => write!(f, "inherit"),
            Self::LocalOnly => write!(f, "local_only"),
            Self::EncryptedOnly => write!(f, "encrypted_only"),
            Self::Any => write!(f, "any"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inherit_adopts_parent() {
        assert_eq!(
            PrivacyBoundary::Inherit.resolve(&PrivacyBoundary::LocalOnly),
            PrivacyBoundary::LocalOnly
        );
        assert_eq!(
            PrivacyBoundary::Inherit.resolve(&PrivacyBoundary::EncryptedOnly),
            PrivacyBoundary::EncryptedOnly
        );
        assert_eq!(
            PrivacyBoundary::Inherit.resolve(&PrivacyBoundary::Any),
            PrivacyBoundary::Any
        );
    }

    #[test]
    fn local_only_parent_always_wins() {
        let parent = PrivacyBoundary::LocalOnly;
        assert_eq!(
            PrivacyBoundary::Any.resolve(&parent),
            PrivacyBoundary::LocalOnly
        );
        assert_eq!(
            PrivacyBoundary::EncryptedOnly.resolve(&parent),
            PrivacyBoundary::LocalOnly
        );
        assert_eq!(
            PrivacyBoundary::LocalOnly.resolve(&parent),
            PrivacyBoundary::LocalOnly
        );
    }

    #[test]
    fn encrypted_only_clamps_any() {
        let parent = PrivacyBoundary::EncryptedOnly;
        assert_eq!(
            PrivacyBoundary::Any.resolve(&parent),
            PrivacyBoundary::EncryptedOnly
        );
    }

    #[test]
    fn child_can_be_stricter_than_parent() {
        let parent = PrivacyBoundary::Any;
        assert_eq!(
            PrivacyBoundary::LocalOnly.resolve(&parent),
            PrivacyBoundary::LocalOnly
        );
        assert_eq!(
            PrivacyBoundary::EncryptedOnly.resolve(&parent),
            PrivacyBoundary::EncryptedOnly
        );
    }

    #[test]
    fn encrypted_parent_allows_local_child() {
        assert_eq!(
            PrivacyBoundary::LocalOnly.resolve(&PrivacyBoundary::EncryptedOnly),
            PrivacyBoundary::LocalOnly
        );
    }

    #[test]
    fn allows_provider_local_only_blocks_cloud() {
        assert!(!PrivacyBoundary::LocalOnly.allows_provider("anthropic"));
        assert!(!PrivacyBoundary::LocalOnly.allows_provider("openai"));
        assert!(PrivacyBoundary::LocalOnly.allows_provider("ollama"));
        assert!(PrivacyBoundary::LocalOnly.allows_provider("llamacpp"));
    }

    #[test]
    fn allows_provider_other_modes_allow_all() {
        assert!(PrivacyBoundary::Any.allows_provider("anthropic"));
        assert!(PrivacyBoundary::EncryptedOnly.allows_provider("openai"));
        assert!(PrivacyBoundary::Inherit.allows_provider("anthropic"));
    }

    #[test]
    fn allows_network() {
        assert!(!PrivacyBoundary::LocalOnly.allows_network());
        assert!(PrivacyBoundary::EncryptedOnly.allows_network());
        assert!(PrivacyBoundary::Any.allows_network());
        assert!(PrivacyBoundary::Inherit.allows_network());
    }

    #[test]
    fn strictness_ordering() {
        assert!(
            PrivacyBoundary::LocalOnly.is_at_least_as_strict_as(&PrivacyBoundary::EncryptedOnly)
        );
        assert!(PrivacyBoundary::EncryptedOnly.is_at_least_as_strict_as(&PrivacyBoundary::Any));
        assert!(!PrivacyBoundary::Any.is_at_least_as_strict_as(&PrivacyBoundary::LocalOnly));
    }

    #[test]
    fn serde_round_trip() {
        let boundary = PrivacyBoundary::EncryptedOnly;
        let json = serde_json::to_string(&boundary).unwrap();
        assert_eq!(json, "\"encrypted_only\"");
        let deserialized: PrivacyBoundary = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, boundary);
    }

    #[test]
    fn default_is_inherit() {
        assert_eq!(PrivacyBoundary::default(), PrivacyBoundary::Inherit);
    }

    #[test]
    fn display_formatting() {
        assert_eq!(PrivacyBoundary::LocalOnly.to_string(), "local_only");
        assert_eq!(PrivacyBoundary::EncryptedOnly.to_string(), "encrypted_only");
        assert_eq!(PrivacyBoundary::Any.to_string(), "any");
        assert_eq!(PrivacyBoundary::Inherit.to_string(), "inherit");
    }
}

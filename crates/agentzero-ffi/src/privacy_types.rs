//! FFI-safe privacy types for foreign language consumers.
//!
//! Exposes read-only query types for inspecting privacy status and boundaries —
//! not the full crypto machinery. Mobile/Node apps need to check privacy state,
//! not perform raw Noise handshakes.

use agentzero_core::privacy::boundary::PrivacyBoundary as CoreBoundary;

/// Privacy boundary level (FFI-safe mirror of `agentzero_core::privacy::PrivacyBoundary`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum PrivacyBoundary {
    Inherit,
    LocalOnly,
    EncryptedOnly,
    Any,
}

impl From<CoreBoundary> for PrivacyBoundary {
    fn from(b: CoreBoundary) -> Self {
        match b {
            CoreBoundary::Inherit => Self::Inherit,
            CoreBoundary::LocalOnly => Self::LocalOnly,
            CoreBoundary::EncryptedOnly => Self::EncryptedOnly,
            CoreBoundary::Any => Self::Any,
        }
    }
}

impl From<PrivacyBoundary> for CoreBoundary {
    fn from(b: PrivacyBoundary) -> Self {
        match b {
            PrivacyBoundary::Inherit => Self::Inherit,
            PrivacyBoundary::LocalOnly => Self::LocalOnly,
            PrivacyBoundary::EncryptedOnly => Self::EncryptedOnly,
            PrivacyBoundary::Any => Self::Any,
        }
    }
}

impl PrivacyBoundary {
    /// Parse from a string like "local_only", "encrypted_only", "any", "inherit".
    pub fn from_string(s: &str) -> Self {
        match s {
            "local_only" => Self::LocalOnly,
            "encrypted_only" => Self::EncryptedOnly,
            "any" => Self::Any,
            _ => Self::Inherit,
        }
    }

    /// Convert to the canonical string representation.
    pub fn to_str(&self) -> &'static str {
        match self {
            Self::Inherit => "inherit",
            Self::LocalOnly => "local_only",
            Self::EncryptedOnly => "encrypted_only",
            Self::Any => "any",
        }
    }
}

/// Gateway privacy capabilities reported to clients.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PrivacyInfo {
    pub noise_enabled: bool,
    pub handshake_pattern: String,
    pub public_key: Option<String>,
    pub key_fingerprint: Option<String>,
    pub sealed_envelopes_enabled: bool,
    pub relay_mode: bool,
    pub supported_patterns: Vec<String>,
}

impl Default for PrivacyInfo {
    fn default() -> Self {
        Self {
            noise_enabled: false,
            handshake_pattern: "XX".to_string(),
            public_key: None,
            key_fingerprint: None,
            sealed_envelopes_enabled: false,
            relay_mode: false,
            supported_patterns: vec!["XX".to_string()],
        }
    }
}

/// Current privacy status snapshot.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct PrivacyStatus {
    /// Current mode: "off", "local_only", "encrypted", "full".
    pub mode: String,
    /// Effective boundary as a string.
    pub effective_boundary: String,
    /// Whether a Noise session is active.
    pub noise_active: bool,
    /// Current key epoch (rotation counter), if available.
    pub key_epoch: Option<u32>,
    /// Fingerprint of the current key, if available.
    pub key_fingerprint: Option<String>,
}

impl Default for PrivacyStatus {
    fn default() -> Self {
        Self {
            mode: "off".to_string(),
            effective_boundary: "inherit".to_string(),
            noise_active: false,
            key_epoch: None,
            key_fingerprint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_boundary_ffi_round_trip() {
        let core = CoreBoundary::LocalOnly;
        let ffi: PrivacyBoundary = core.clone().into();
        let back: CoreBoundary = ffi.into();
        assert_eq!(back, core);
    }

    #[test]
    fn privacy_boundary_all_variants_round_trip() {
        for core in [
            CoreBoundary::Inherit,
            CoreBoundary::LocalOnly,
            CoreBoundary::EncryptedOnly,
            CoreBoundary::Any,
        ] {
            let ffi: PrivacyBoundary = core.clone().into();
            let back: CoreBoundary = ffi.into();
            assert_eq!(back, core);
        }
    }

    #[test]
    fn privacy_info_construction() {
        let info = PrivacyInfo {
            noise_enabled: true,
            handshake_pattern: "IK".to_string(),
            public_key: Some("base64key".to_string()),
            key_fingerprint: Some("abc123".to_string()),
            sealed_envelopes_enabled: true,
            relay_mode: false,
            supported_patterns: vec!["XX".to_string(), "IK".to_string()],
        };
        assert!(info.noise_enabled);
        assert_eq!(info.handshake_pattern, "IK");
        assert_eq!(info.public_key.as_deref(), Some("base64key"));
        assert_eq!(info.supported_patterns.len(), 2);
    }

    #[test]
    fn privacy_status_default() {
        let status = PrivacyStatus::default();
        assert_eq!(status.mode, "off");
        assert_eq!(status.effective_boundary, "inherit");
        assert!(!status.noise_active);
        assert!(status.key_epoch.is_none());
        assert!(status.key_fingerprint.is_none());
    }

    #[test]
    fn privacy_boundary_string_conversion() {
        assert_eq!(
            PrivacyBoundary::from_string("local_only"),
            PrivacyBoundary::LocalOnly
        );
        assert_eq!(
            PrivacyBoundary::from_string("encrypted_only"),
            PrivacyBoundary::EncryptedOnly
        );
        assert_eq!(PrivacyBoundary::from_string("any"), PrivacyBoundary::Any);
        assert_eq!(
            PrivacyBoundary::from_string("inherit"),
            PrivacyBoundary::Inherit
        );
        assert_eq!(
            PrivacyBoundary::from_string("unknown"),
            PrivacyBoundary::Inherit
        );

        assert_eq!(PrivacyBoundary::LocalOnly.to_str(), "local_only");
        assert_eq!(PrivacyBoundary::EncryptedOnly.to_str(), "encrypted_only");
        assert_eq!(PrivacyBoundary::Any.to_str(), "any");
        assert_eq!(PrivacyBoundary::Inherit.to_str(), "inherit");
    }

    #[test]
    fn privacy_info_default() {
        let info = PrivacyInfo::default();
        assert!(!info.noise_enabled);
        assert_eq!(info.handshake_pattern, "XX");
        assert!(info.public_key.is_none());
        assert!(!info.sealed_envelopes_enabled);
        assert!(!info.relay_mode);
        assert_eq!(info.supported_patterns, vec!["XX"]);
    }
}

//! TOTP (Time-based One-Time Password) implementation per RFC 6238.
//!
//! Used for OTP gating of sensitive actions, domains, and estop resume.

use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha1 = Hmac<Sha1>;

/// Generate a TOTP code for the given secret and time step.
///
/// - `secret`: shared secret key (raw bytes, typically base32-decoded)
/// - `time_step_secs`: time step in seconds (default: 30)
/// - `digits`: number of digits in the OTP (default: 6)
pub fn generate_totp(secret: &[u8], time_step_secs: u64, digits: u32) -> anyhow::Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("system clock before unix epoch"))?;
    let counter = now.as_secs() / time_step_secs;
    generate_hotp(secret, counter, digits)
}

/// Generate a TOTP code for a specific unix timestamp (for testing).
pub fn generate_totp_at(
    secret: &[u8],
    time_step_secs: u64,
    digits: u32,
    unix_secs: u64,
) -> anyhow::Result<String> {
    let counter = unix_secs / time_step_secs;
    generate_hotp(secret, counter, digits)
}

/// Validate a TOTP code with a time window for clock skew tolerance.
///
/// - `code`: the OTP code to validate
/// - `secret`: shared secret key
/// - `time_step_secs`: time step in seconds
/// - `digits`: number of digits
/// - `window`: number of time steps to check before and after current
pub fn validate_totp(
    code: &str,
    secret: &[u8],
    time_step_secs: u64,
    digits: u32,
    window: u64,
) -> anyhow::Result<bool> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| anyhow::anyhow!("system clock before unix epoch"))?;
    let current_counter = now.as_secs() / time_step_secs;

    for offset in 0..=window {
        // Check current and both directions
        let counters = if offset == 0 {
            vec![current_counter]
        } else {
            vec![
                current_counter.wrapping_add(offset),
                current_counter.wrapping_sub(offset),
            ]
        };

        for counter in counters {
            let expected = generate_hotp(secret, counter, digits)?;
            if constant_time_eq(code.as_bytes(), expected.as_bytes()) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// Validate a TOTP code at a specific timestamp (for testing).
pub fn validate_totp_at(
    code: &str,
    secret: &[u8],
    time_step_secs: u64,
    digits: u32,
    window: u64,
    unix_secs: u64,
) -> anyhow::Result<bool> {
    let current_counter = unix_secs / time_step_secs;

    for offset in 0..=window {
        let counters = if offset == 0 {
            vec![current_counter]
        } else {
            vec![
                current_counter.wrapping_add(offset),
                current_counter.wrapping_sub(offset),
            ]
        };

        for counter in counters {
            let expected = generate_hotp(secret, counter, digits)?;
            if constant_time_eq(code.as_bytes(), expected.as_bytes()) {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

/// HOTP (HMAC-based One-Time Password) per RFC 4226.
fn generate_hotp(secret: &[u8], counter: u64, digits: u32) -> anyhow::Result<String> {
    let mut mac =
        HmacSha1::new_from_slice(secret).map_err(|e| anyhow::anyhow!("HMAC key error: {e}"))?;
    mac.update(&counter.to_be_bytes());
    let result = mac.finalize().into_bytes();

    // Dynamic truncation (RFC 4226 section 5.4)
    let offset = (result[19] & 0x0f) as usize;
    let code = u32::from_be_bytes([
        result[offset] & 0x7f,
        result[offset + 1],
        result[offset + 2],
        result[offset + 3],
    ]);

    let modulus = 10u32.pow(digits);
    Ok(format!(
        "{:0>width$}",
        code % modulus,
        width = digits as usize
    ))
}

/// Constant-time comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ---------------------------------------------------------------------------
// OTP Gating Engine
// ---------------------------------------------------------------------------

/// Decision from the OTP gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtpGateResult {
    /// Action is not gated — proceed without OTP.
    NotGated,
    /// Action requires OTP validation.
    RequiresOtp { reason: String },
    /// OTP was validated and the action is approved (cached for `cache_valid_secs`).
    Approved,
    /// OTP validation failed.
    Denied { reason: String },
}

/// OTP gating engine — checks whether an action or domain requires OTP.
#[derive(Debug, Clone)]
pub struct OtpGate {
    pub enabled: bool,
    pub gated_actions: Vec<String>,
    pub gated_domains: Vec<String>,
    pub gated_domain_categories: Vec<String>,
    pub cache_valid_secs: u64,
    /// Tracks approved actions with their approval timestamp.
    approvals: HashMap<String, u64>,
}

impl OtpGate {
    pub fn new(
        enabled: bool,
        gated_actions: Vec<String>,
        gated_domains: Vec<String>,
        gated_domain_categories: Vec<String>,
        cache_valid_secs: u64,
    ) -> Self {
        Self {
            enabled,
            gated_actions,
            gated_domains,
            gated_domain_categories,
            cache_valid_secs,
            approvals: HashMap::new(),
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            gated_actions: Vec::new(),
            gated_domains: Vec::new(),
            gated_domain_categories: Vec::new(),
            cache_valid_secs: 300,
            approvals: HashMap::new(),
        }
    }

    /// Check if an action requires OTP.
    pub fn check_action(&self, action: &str) -> OtpGateResult {
        if !self.enabled {
            return OtpGateResult::NotGated;
        }

        let is_gated = self
            .gated_actions
            .iter()
            .any(|a| a.eq_ignore_ascii_case(action));

        if !is_gated {
            return OtpGateResult::NotGated;
        }

        // Check cached approval
        let cache_key = format!("action:{action}");
        if self.is_cached_approval(&cache_key) {
            return OtpGateResult::Approved;
        }

        OtpGateResult::RequiresOtp {
            reason: format!("Action `{action}` requires OTP verification"),
        }
    }

    /// Check if a domain requires OTP.
    pub fn check_domain(&self, domain: &str) -> OtpGateResult {
        if !self.enabled {
            return OtpGateResult::NotGated;
        }

        let is_gated = self
            .gated_domains
            .iter()
            .any(|d| d.eq_ignore_ascii_case(domain) || domain.ends_with(&format!(".{d}")));

        if !is_gated {
            return OtpGateResult::NotGated;
        }

        let cache_key = format!("domain:{domain}");
        if self.is_cached_approval(&cache_key) {
            return OtpGateResult::Approved;
        }

        OtpGateResult::RequiresOtp {
            reason: format!("Domain `{domain}` requires OTP verification"),
        }
    }

    /// Record an OTP approval for caching.
    pub fn record_approval(&mut self, key: &str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.approvals.insert(key.to_string(), now);
    }

    /// Record an action approval after successful OTP validation.
    pub fn approve_action(&mut self, action: &str) {
        self.record_approval(&format!("action:{action}"));
    }

    /// Record a domain approval after successful OTP validation.
    pub fn approve_domain(&mut self, domain: &str) {
        self.record_approval(&format!("domain:{domain}"));
    }

    fn is_cached_approval(&self, key: &str) -> bool {
        if let Some(&approved_at) = self.approvals.get(key) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now.saturating_sub(approved_at) < self.cache_valid_secs
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn hotp_rfc4226_test_vectors() {
        // RFC 4226 Appendix D test vectors for HOTP with secret "12345678901234567890"
        let expected = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583",
            "399871", "520489",
        ];

        for (counter, expected_code) in expected.iter().enumerate() {
            let code = generate_hotp(TEST_SECRET, counter as u64, 6).unwrap();
            assert_eq!(&code, expected_code, "HOTP mismatch at counter {counter}");
        }
    }

    #[test]
    fn totp_at_known_time() {
        // With time_step=30, at time=59 the counter=1
        let code = generate_totp_at(TEST_SECRET, 30, 6, 59).unwrap();
        // Counter 1 should produce "287082" per RFC 4226
        assert_eq!(code, "287082");
    }

    #[test]
    fn totp_at_boundary() {
        // At time=30, counter=1
        let code_30 = generate_totp_at(TEST_SECRET, 30, 6, 30).unwrap();
        // At time=29, counter=0
        let code_29 = generate_totp_at(TEST_SECRET, 30, 6, 29).unwrap();
        // These should be different (different counters)
        assert_ne!(code_30, code_29);
    }

    #[test]
    fn validate_totp_correct_code() {
        let unix_time = 59u64;
        let code = generate_totp_at(TEST_SECRET, 30, 6, unix_time).unwrap();
        let valid = validate_totp_at(&code, TEST_SECRET, 30, 6, 0, unix_time).unwrap();
        assert!(valid);
    }

    #[test]
    fn validate_totp_wrong_code() {
        let valid = validate_totp_at("000000", TEST_SECRET, 30, 6, 0, 59).unwrap();
        assert!(!valid);
    }

    #[test]
    fn validate_totp_with_window() {
        // Generate code at time=30 (counter=1)
        let code = generate_totp_at(TEST_SECRET, 30, 6, 30).unwrap();
        // Validate at time=60 (counter=2), with window=1 should accept counter=1
        let valid = validate_totp_at(&code, TEST_SECRET, 30, 6, 1, 60).unwrap();
        assert!(valid);
        // Without window, should reject
        let invalid = validate_totp_at(&code, TEST_SECRET, 30, 6, 0, 60).unwrap();
        assert!(!invalid);
    }

    #[test]
    fn digits_8_generates_8_chars() {
        let code = generate_totp_at(TEST_SECRET, 30, 8, 59).unwrap();
        assert_eq!(code.len(), 8);
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
    }

    // --- OTP Gate tests ---

    #[test]
    fn disabled_gate_never_requires_otp() {
        let gate = OtpGate::disabled();
        assert_eq!(gate.check_action("shell"), OtpGateResult::NotGated);
        assert_eq!(gate.check_domain("example.com"), OtpGateResult::NotGated);
    }

    #[test]
    fn gate_requires_otp_for_gated_action() {
        let gate = OtpGate::new(
            true,
            vec!["shell".into(), "file_write".into()],
            Vec::new(),
            Vec::new(),
            300,
        );

        match gate.check_action("shell") {
            OtpGateResult::RequiresOtp { reason } => {
                assert!(reason.contains("shell"));
            }
            other => panic!("expected RequiresOtp, got {other:?}"),
        }

        assert_eq!(gate.check_action("file_read"), OtpGateResult::NotGated);
    }

    #[test]
    fn gate_requires_otp_for_gated_domain() {
        let gate = OtpGate::new(true, Vec::new(), vec!["bank.com".into()], Vec::new(), 300);

        match gate.check_domain("bank.com") {
            OtpGateResult::RequiresOtp { reason } => {
                assert!(reason.contains("bank.com"));
            }
            other => panic!("expected RequiresOtp, got {other:?}"),
        }

        // Subdomain should also be gated
        match gate.check_domain("api.bank.com") {
            OtpGateResult::RequiresOtp { .. } => {}
            other => panic!("expected RequiresOtp for subdomain, got {other:?}"),
        }

        assert_eq!(gate.check_domain("example.com"), OtpGateResult::NotGated);
    }

    #[test]
    fn gate_caches_approval() {
        let mut gate = OtpGate::new(true, vec!["shell".into()], Vec::new(), Vec::new(), 300);

        // First check should require OTP
        assert!(matches!(
            gate.check_action("shell"),
            OtpGateResult::RequiresOtp { .. }
        ));

        // Record approval
        gate.approve_action("shell");

        // Now should be approved (cached)
        assert_eq!(gate.check_action("shell"), OtpGateResult::Approved);
    }

    #[test]
    fn gate_case_insensitive_action_check() {
        let gate = OtpGate::new(true, vec!["Shell".into()], Vec::new(), Vec::new(), 300);

        assert!(matches!(
            gate.check_action("shell"),
            OtpGateResult::RequiresOtp { .. }
        ));
        assert!(matches!(
            gate.check_action("SHELL"),
            OtpGateResult::RequiresOtp { .. }
        ));
    }
}

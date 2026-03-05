use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tracing::warn;

/// Action to take when a credential leak is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeakAction {
    Redact,
    Block,
}

impl LeakAction {
    pub fn from_str_loose(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "block" => Self::Block,
            _ => Self::Redact,
        }
    }
}

/// Configuration for the outbound leak guard.
#[derive(Debug, Clone)]
pub struct LeakGuardPolicy {
    pub enabled: bool,
    pub action: LeakAction,
    pub sensitivity: f64,
}

impl Default for LeakGuardPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            action: LeakAction::Redact,
            sensitivity: 0.7,
        }
    }
}

/// Result of scanning text for credential leaks.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub has_leaks: bool,
    pub findings: Vec<LeakFinding>,
    pub redacted_text: Option<String>,
}

/// A single credential leak finding.
#[derive(Debug, Clone)]
pub struct LeakFinding {
    pub pattern_name: &'static str,
    pub matched_text: String,
    pub start: usize,
    pub end: usize,
}

static PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        // API keys with common prefixes.
        (
            "api_key_prefix",
            Regex::new(r"(?i)(sk-|api[_-]?key[=:\s]+)[a-zA-Z0-9_\-]{20,}").unwrap(),
        ),
        // Bearer tokens.
        (
            "bearer_token",
            Regex::new(r"(?i)bearer\s+[a-zA-Z0-9_\-\.]{20,}").unwrap(),
        ),
        // JWT tokens (3-part base64 with dots).
        (
            "jwt_token",
            Regex::new(r"eyJ[a-zA-Z0-9_-]+\.eyJ[a-zA-Z0-9_-]+\.[a-zA-Z0-9_-]+").unwrap(),
        ),
        // AWS access keys.
        ("aws_access_key", Regex::new(r"AKIA[A-Z0-9]{16}").unwrap()),
        // Private key markers.
        (
            "private_key",
            Regex::new(r"-----BEGIN\s+(RSA\s+)?PRIVATE\s+KEY-----").unwrap(),
        ),
        // GitHub tokens.
        (
            "github_token",
            Regex::new(r"gh[ps]_[A-Za-z0-9_]{36,}").unwrap(),
        ),
        // Anthropic API keys.
        (
            "anthropic_key",
            Regex::new(r"sk-ant-[a-zA-Z0-9_\-]{20,}").unwrap(),
        ),
        // OpenAI API keys.
        ("openai_key", Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap()),
        // Slack tokens.
        (
            "slack_token",
            Regex::new(r"xox[baprs]-[A-Za-z0-9\-]+").unwrap(),
        ),
        // X25519 / Noise session keys (64-char hex strings that look like key material).
        (
            "x25519_key_material",
            Regex::new(
                r"(?i)(noise[_-]?session|x25519[_-]?key|privacy[_-]?key)[=:\s]+[a-fA-F0-9]{64}",
            )
            .unwrap(),
        ),
    ]
});

/// High-entropy token detector (Shannon entropy).
fn shannon_entropy(s: &str) -> f64 {
    if s.is_empty() {
        return 0.0;
    }
    let mut counts = [0u32; 256];
    for byte in s.bytes() {
        counts[byte as usize] += 1;
    }
    let len = s.len() as f64;
    counts
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / len;
            -p * p.log2()
        })
        .sum()
}

/// Detect high-entropy strings that might be leaked secrets.
static HIGH_ENTROPY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{32,}").unwrap());

impl LeakGuardPolicy {
    /// Scan text for credential leaks. Returns findings and optionally redacted text.
    pub fn scan(&self, text: &str) -> ScanResult {
        if !self.enabled {
            return ScanResult {
                has_leaks: false,
                findings: Vec::new(),
                redacted_text: None,
            };
        }

        let mut findings = Vec::new();

        // Pattern-based detection.
        for (name, pattern) in PATTERNS.iter() {
            for mat in pattern.find_iter(text) {
                findings.push(LeakFinding {
                    pattern_name: name,
                    matched_text: mat.as_str().to_string(),
                    start: mat.start(),
                    end: mat.end(),
                });
            }
        }

        // High-entropy token detection (only if sensitivity >= 0.5).
        if self.sensitivity >= 0.5 {
            let threshold = 4.0 + (1.0 - self.sensitivity) * 2.0;
            for mat in HIGH_ENTROPY_RE.find_iter(text) {
                let s = mat.as_str();
                if shannon_entropy(s) > threshold {
                    let already_found = findings
                        .iter()
                        .any(|f| f.start <= mat.start() && f.end >= mat.end());
                    if !already_found {
                        findings.push(LeakFinding {
                            pattern_name: "high_entropy_token",
                            matched_text: s.to_string(),
                            start: mat.start(),
                            end: mat.end(),
                        });
                    }
                }
            }
        }

        let has_leaks = !findings.is_empty();

        let redacted_text = if has_leaks && self.action == LeakAction::Redact {
            // Merge overlapping ranges, then replace from end to start.
            let mut sorted = findings.clone();
            sorted.sort_by_key(|f| f.start);
            let mut merged: Vec<LeakFinding> = Vec::new();
            for f in sorted {
                if let Some(last) = merged.last_mut() {
                    if f.start <= last.end {
                        // Overlapping or adjacent — extend the previous finding.
                        if f.end > last.end {
                            last.end = f.end;
                            last.matched_text = text[last.start..last.end].to_string();
                        }
                        continue;
                    }
                }
                merged.push(f);
            }
            // Replace from end to preserve byte offsets.
            let mut redacted = text.to_string();
            for finding in merged.iter().rev() {
                let replacement = format!("[REDACTED:{}]", finding.pattern_name);
                redacted.replace_range(finding.start..finding.end, &replacement);
            }
            Some(redacted)
        } else {
            None
        };

        if has_leaks {
            for finding in &findings {
                warn!(
                    pattern = finding.pattern_name,
                    "credential leak detected in outbound message"
                );
            }
        }

        ScanResult {
            has_leaks,
            findings,
            redacted_text,
        }
    }

    /// Process text: redact leaks or block if action = Block.
    pub fn process(&self, text: &str) -> Result<String, String> {
        let result = self.scan(text);
        if !result.has_leaks {
            return Ok(text.to_string());
        }
        match self.action {
            LeakAction::Redact => Ok(result.redacted_text.unwrap_or_else(|| text.to_string())),
            LeakAction::Block => Err(format!(
                "outbound message blocked: {} credential leak(s) detected",
                result.findings.len()
            )),
        }
    }

    /// Check whether outbound content to a target channel respects privacy
    /// boundaries.  Returns `Err` with a description when content from a
    /// `local_only` boundary is about to be sent to a non-local channel.
    ///
    /// This is a defense-in-depth heuristic — it cannot cryptographically
    /// guarantee isolation but catches the most common leakage paths.
    pub fn check_boundary(
        &self,
        _text: &str,
        content_boundary: &str,
        target_channel: &str,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if content_boundary == "local_only" && !crate::is_local_channel(target_channel) {
            warn!(
                target_channel,
                content_boundary, "leak guard blocked local_only content to non-local channel"
            );
            return Err(format!(
                "outbound message blocked: content with boundary 'local_only' \
                 cannot be sent to non-local channel '{target_channel}'"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> LeakGuardPolicy {
        LeakGuardPolicy {
            enabled: true,
            action: LeakAction::Redact,
            sensitivity: 0.7,
        }
    }

    #[test]
    fn detects_api_key_prefix() {
        let g = guard();
        let result = g.scan("Here is my key: sk-abc123def456ghi789jkl012");
        assert!(result.has_leaks);
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern_name == "openai_key"));
    }

    #[test]
    fn detects_jwt_token() {
        let g = guard();
        let jwt =
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ0ZXN0In0.dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let result = g.scan(&format!("Token: {jwt}"));
        assert!(result.has_leaks);
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern_name == "jwt_token"));
    }

    #[test]
    fn detects_aws_access_key() {
        let g = guard();
        let result = g.scan("AWS key: AKIAIOSFODNN7EXAMPLE");
        assert!(result.has_leaks);
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern_name == "aws_access_key"));
    }

    #[test]
    fn detects_private_key_marker() {
        let g = guard();
        let result = g.scan("-----BEGIN RSA PRIVATE KEY-----\nMIIE...");
        assert!(result.has_leaks);
        assert!(result
            .findings
            .iter()
            .any(|f| f.pattern_name == "private_key"));
    }

    #[test]
    fn redacts_detected_leaks() {
        let g = guard();
        let text = "Key: sk-abc123def456ghi789jkl012mno345";
        let result = g.scan(text);
        assert!(result.has_leaks);
        let redacted = result.redacted_text.unwrap();
        assert!(redacted.contains("[REDACTED:"));
        assert!(!redacted.contains("sk-abc123"));
    }

    #[test]
    fn block_action_returns_error() {
        let g = LeakGuardPolicy {
            enabled: true,
            action: LeakAction::Block,
            sensitivity: 0.7,
        };
        let result = g.process("Key: sk-abc123def456ghi789jkl012mno345");
        assert!(result.is_err());
    }

    #[test]
    fn clean_text_passes_through() {
        let g = guard();
        let result = g.scan("Hello, how can I help you today?");
        assert!(!result.has_leaks);
    }

    #[test]
    fn disabled_guard_passes_everything() {
        let g = LeakGuardPolicy {
            enabled: false,
            ..guard()
        };
        let result = g.scan("sk-abc123def456ghi789jkl012mno345");
        assert!(!result.has_leaks);
    }

    #[test]
    fn shannon_entropy_varies_with_randomness() {
        assert!(shannon_entropy("aaaaaaaaaa") < 1.0);
        assert!(shannon_entropy("aB3$xZ9!qW") > 3.0);
    }

    #[test]
    fn check_boundary_blocks_local_only_to_nonlocal() {
        let g = guard();
        let result = g.check_boundary("some content", "local_only", "telegram");
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("local_only"));
        assert!(msg.contains("telegram"));
    }

    #[test]
    fn check_boundary_allows_local_only_to_cli() {
        let g = guard();
        let result = g.check_boundary("some content", "local_only", "cli");
        assert!(result.is_ok());
    }

    #[test]
    fn check_boundary_allows_any_to_nonlocal() {
        let g = guard();
        let result = g.check_boundary("some content", "any", "telegram");
        assert!(result.is_ok());
    }

    #[test]
    fn check_boundary_allows_empty_to_nonlocal() {
        let g = guard();
        let result = g.check_boundary("some content", "", "slack");
        assert!(result.is_ok());
    }

    #[test]
    fn check_boundary_disabled_allows_everything() {
        let g = LeakGuardPolicy {
            enabled: false,
            ..guard()
        };
        let result = g.check_boundary("content", "local_only", "discord");
        assert!(result.is_ok());
    }

    #[test]
    fn check_boundary_blocks_local_only_to_slack() {
        let g = guard();
        assert!(g.check_boundary("secret", "local_only", "slack").is_err());
    }

    #[test]
    fn check_boundary_blocks_local_only_to_discord() {
        let g = guard();
        assert!(g.check_boundary("secret", "local_only", "discord").is_err());
    }

    #[test]
    fn check_boundary_allows_local_only_to_transcription() {
        let g = guard();
        assert!(g
            .check_boundary("secret", "local_only", "transcription")
            .is_ok());
    }
}

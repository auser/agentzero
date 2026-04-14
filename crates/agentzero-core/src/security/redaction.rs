use regex::Regex;
use std::error::Error;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Shared PII / credential patterns
// ---------------------------------------------------------------------------

/// A compiled pattern that identifies a class of PII or credential material.
/// Used by both the audit-sink redactor (`redact_text`) and the provider-side
/// `PiiRedactionGuard` so that every path through the system applies identical
/// coverage.
pub struct RedactionPattern {
    pub name: &'static str,
    pub regex: Regex,
    pub redaction: &'static str,
}

/// Canonical list of PII / credential patterns.  Ordering matters: more
/// specific patterns run first so they replace their target text before a less
/// specific pattern can match a sub-component (e.g. `db_connection_string`
/// must precede `email` because `user:pass@host.com` would otherwise be
/// partially matched by the email regex).
pub static PII_PATTERNS: LazyLock<Vec<RedactionPattern>> = LazyLock::new(|| {
    vec![
        // --- Most specific / structured patterns first ---
        RedactionPattern {
            name: "db_connection_string",
            regex: Regex::new(r"(?:postgres|mysql|mongodb(?:\+srv)?|redis)://\S+:\S+@\S+")
                .expect("db_conn regex should compile"),
            redaction: "[DB_CONN_REDACTED]",
        },
        RedactionPattern {
            name: "ssh_private_key",
            regex: Regex::new(r"-----BEGIN (?:RSA |DSA |EC |OPENSSH )?PRIVATE KEY-----")
                .expect("ssh_key regex should compile"),
            redaction: "[SSH_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "jwt",
            regex: Regex::new(r"\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b")
                .expect("jwt regex should compile"),
            redaction: "[JWT_REDACTED]",
        },
        // --- Credential patterns (also used by leak guard) ---
        RedactionPattern {
            name: "anthropic_key",
            regex: Regex::new(r"sk-ant-[a-zA-Z0-9_\-]{20,}").expect("anthropic key regex"),
            redaction: "[API_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "aws_access_key",
            regex: Regex::new(r"AKIA[A-Z0-9]{16}").expect("aws key regex"),
            redaction: "[API_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "github_token",
            regex: Regex::new(r"gh[ps]_[A-Za-z0-9_]{36,}").expect("github token regex"),
            redaction: "[API_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "slack_token",
            regex: Regex::new(r"xox[baprs]-[A-Za-z0-9\-]+").expect("slack token regex"),
            redaction: "[API_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "x25519_key_material",
            regex: Regex::new(
                r"(?i)(noise[_-]?session|x25519[_-]?key|privacy[_-]?key)[=:\s]+[a-fA-F0-9]{64}",
            )
            .expect("x25519 key regex"),
            redaction: "[KEY_MATERIAL_REDACTED]",
        },
        RedactionPattern {
            name: "api_key",
            regex: Regex::new(r"\b(?:sk-[a-zA-Z0-9]{20,}|AKIA[A-Z0-9]{16}|ghp_[a-zA-Z0-9]{36})\b")
                .expect("api_key regex should compile"),
            redaction: "[API_KEY_REDACTED]",
        },
        RedactionPattern {
            name: "credit_card",
            // Require at least one separator (space/hyphen) between digit groups
            // to avoid false positives on timestamps and other long integers.
            // Matches: "4111 1111 1111 1111", "4111-1111-1111-1111",
            //          "4111 1111 11111111", "5500-0000-0000-0004"
            regex: Regex::new(r"\b\d{4}[ -]\d{4}[ -]?\d{4}[ -]?\d{1,7}\b")
                .expect("credit_card regex should compile"),
            redaction: "[CC_REDACTED]",
        },
        // --- Less specific patterns last ---
        RedactionPattern {
            name: "email",
            regex: Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}")
                .expect("email regex should compile"),
            redaction: "[EMAIL_REDACTED]",
        },
        RedactionPattern {
            name: "phone_us",
            regex: Regex::new(r"\b(?:\+1[-.\s]?)?\(?\d{3}\)?[-.\s]?\d{3}[-.\s]?\d{4}\b")
                .expect("phone regex should compile"),
            redaction: "[PHONE_REDACTED]",
        },
        RedactionPattern {
            name: "ssn",
            regex: Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").expect("ssn regex should compile"),
            redaction: "[SSN_REDACTED]",
        },
        RedactionPattern {
            name: "ipv4_address",
            regex: Regex::new(
                r"\b(?:(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\.){3}(?:25[0-5]|2[0-4]\d|[01]?\d\d?)\b",
            )
            .expect("ipv4 regex should compile"),
            redaction: "[IP_REDACTED]",
        },
        // --- Auth header patterns (previously only in old redact_text) ---
        RedactionPattern {
            name: "env_secret",
            regex: Regex::new(r#"(?i)\b(OPENAI_API_KEY|TURSO_AUTH_TOKEN)\s*=\s*([^\s,;]+)"#)
                .expect("env_secret regex"),
            redaction: "$1=[REDACTED]",
        },
        RedactionPattern {
            name: "json_secret",
            regex: Regex::new(r#"(?i)"(api[_-]?key|auth[_-]?token)"\s*:\s*"[^"]*""#)
                .expect("json_secret regex"),
            redaction: r#""$1":"[REDACTED]""#,
        },
        RedactionPattern {
            name: "bearer_header",
            regex: Regex::new(r"(?i)\b(authorization:\s*bearer)\s+[^\s,;]+")
                .expect("bearer header regex"),
            redaction: "$1 [REDACTED]",
        },
        RedactionPattern {
            name: "bearer_token",
            regex: Regex::new(r"(?i)\b(bearer)\s+[^\s,;]+").expect("bearer regex"),
            redaction: "$1 [REDACTED]",
        },
    ]
});

/// Shannon entropy of a string in bits per byte.  Used to detect
/// high-entropy tokens that don't match any known credential pattern.
pub fn shannon_entropy(s: &str) -> f64 {
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

static HIGH_ENTROPY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9+/=_\-]{32,}").expect("entropy regex"));

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Redact all known PII / credential patterns from `input`.  This is the
/// single authoritative redaction function — used by audit sinks, error
/// chains, panic hooks, and (indirectly via `PII_PATTERNS`) by the
/// `PiiRedactionGuard` provider layer.
pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for p in PII_PATTERNS.iter() {
        out = p.regex.replace_all(&out, p.redaction).to_string();
    }
    // High-entropy catch-all for tokens not matching any known pattern.
    let threshold = 4.5; // conservative — slightly stricter than leak guard default
    for mat in HIGH_ENTROPY_RE.find_iter(input) {
        let s = mat.as_str();
        if shannon_entropy(s) > threshold {
            // Only replace if the text still exists in `out` (may have been
            // replaced by a pattern above).
            if out.contains(s) {
                out = out.replace(s, "[HIGH_ENTROPY_REDACTED]");
            }
        }
    }
    out
}

pub fn redact_error_chain(err: &(dyn Error + 'static)) -> String {
    let mut parts = vec![redact_text(&err.to_string())];
    let mut source = err.source();

    while let Some(cause) = source {
        parts.push(redact_text(&cause.to_string()));
        source = cause.source();
    }

    parts.join(": ")
}

pub fn install_redacting_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = if let Some(text) = panic_info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = panic_info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "panic occurred".to_string()
        };

        eprintln!("panic: {}", redact_text(&message));
        if let Some(location) = panic_info.location() {
            eprintln!(
                "at {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::{Display, Formatter};

    // --- Original tests ---

    #[test]
    fn redacts_common_secret_formats() {
        let input = "OPENAI_API_KEY=sk-supersecret12345 Authorization: Bearer token123 {\"auth_token\":\"abc\"}";
        let out = redact_text(input);

        assert!(
            !out.contains("sk-supersecret12345"),
            "API key leaked: {out}"
        );
        assert!(!out.contains("token123"), "bearer token leaked: {out}");
        assert!(!out.contains("\"abc\""), "auth token leaked: {out}");
    }

    #[test]
    fn leaves_non_secret_text_unchanged() {
        let input = "status ok; model=gpt-4o-mini";
        assert_eq!(redact_text(input), input);
    }

    #[test]
    fn redacts_error_chain_output() {
        #[derive(Debug)]
        struct Root;
        impl Display for Root {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "root OPENAI_API_KEY=sk-exposed012345")
            }
        }
        impl Error for Root {}

        #[derive(Debug)]
        struct Wrapped(Root);
        impl Display for Wrapped {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                write!(f, "wrapper failed")
            }
        }
        impl Error for Wrapped {
            fn source(&self) -> Option<&(dyn Error + 'static)> {
                Some(&self.0)
            }
        }

        let wrapped = Wrapped(Root);
        let text = redact_error_chain(&wrapped);
        assert!(
            !text.contains("sk-exposed012345"),
            "API key leaked in error chain: {text}"
        );
    }

    // --- Sprint 85 PII patterns ---

    #[test]
    fn redacts_credit_card() {
        let out = redact_text("My card is 4111 1111 1111 1111");
        assert!(
            out.contains("[CC_REDACTED]"),
            "credit card not redacted: {out}"
        );
        assert!(!out.contains("4111"), "credit card digits leaked: {out}");
    }

    #[test]
    fn redacts_jwt() {
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let out = redact_text(&format!("token: {jwt}"));
        assert!(out.contains("[JWT_REDACTED]"), "JWT not redacted: {out}");
        assert!(!out.contains("eyJhbGci"), "JWT leaked: {out}");
    }

    #[test]
    fn redacts_ssh_private_key() {
        let out = redact_text("-----BEGIN RSA PRIVATE KEY-----\nMIIEpQIBAAKC...");
        assert!(
            out.contains("[SSH_KEY_REDACTED]"),
            "SSH key not redacted: {out}"
        );
    }

    #[test]
    fn redacts_db_connection_string() {
        let out = redact_text("postgres://admin:s3cret@db.prod.internal:5432/mydb");
        assert!(
            out.contains("[DB_CONN_REDACTED]"),
            "DB conn not redacted: {out}"
        );
        assert!(!out.contains("s3cret"), "DB password leaked: {out}");
    }

    #[test]
    fn redacts_ipv4_address() {
        let out = redact_text("server at 203.0.113.42");
        assert!(out.contains("[IP_REDACTED]"), "IPv4 not redacted: {out}");
        assert!(!out.contains("203.0.113.42"), "IP leaked: {out}");
    }

    #[test]
    fn redacts_email() {
        let out = redact_text("Contact john@example.com please");
        assert!(
            out.contains("[EMAIL_REDACTED]"),
            "email not redacted: {out}"
        );
        assert!(!out.contains("john@example.com"), "email leaked: {out}");
    }

    #[test]
    fn redacts_ssn() {
        let out = redact_text("SSN: 123-45-6789");
        assert!(out.contains("[SSN_REDACTED]"), "SSN not redacted: {out}");
    }

    #[test]
    fn redacts_phone() {
        let out = redact_text("Call me at +1-555-867-5309");
        assert!(
            out.contains("[PHONE_REDACTED]"),
            "phone not redacted: {out}"
        );
    }

    // --- Leak guard credential patterns (newly unified) ---

    #[test]
    fn redacts_anthropic_key() {
        let out = redact_text("key: sk-ant-abc123def456ghi789jkl012mno");
        assert!(
            out.contains("[API_KEY_REDACTED]"),
            "anthropic key not redacted: {out}"
        );
        assert!(
            !out.contains("sk-ant-abc123"),
            "anthropic key leaked: {out}"
        );
    }

    #[test]
    fn redacts_slack_token() {
        let out = redact_text("SLACK_TOKEN=xoxb-123456789012-abcdefghijkl");
        assert!(
            out.contains("[API_KEY_REDACTED]"),
            "slack token not redacted: {out}"
        );
        assert!(!out.contains("xoxb-"), "slack token leaked: {out}");
    }

    #[test]
    fn redacts_github_token() {
        let out = redact_text("GH_TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklm");
        assert!(
            out.contains("[API_KEY_REDACTED]"),
            "github token not redacted: {out}"
        );
    }

    #[test]
    fn redacts_aws_access_key() {
        let out = redact_text("AWS key: AKIAIOSFODNN7EXAMPLE");
        assert!(
            out.contains("[API_KEY_REDACTED]"),
            "AWS key not redacted: {out}"
        );
    }

    #[test]
    fn redacts_x25519_key_material() {
        let hex_key = "a".repeat(64);
        let out = redact_text(&format!("noise_session={hex_key}"));
        assert!(
            out.contains("[KEY_MATERIAL_REDACTED]"),
            "x25519 key not redacted: {out}"
        );
    }

    #[test]
    fn redacts_high_entropy_token() {
        // A 40-char random-looking string that doesn't match any known pattern
        let out = redact_text("secret=Zk9xR3mPqW7vB2nL8cYhT4dF6aJ0eS1uOiKgXwN5");
        assert!(
            out.contains("[HIGH_ENTROPY_REDACTED]") || out.contains("[REDACTED]"),
            "high-entropy token not caught: {out}"
        );
    }

    #[test]
    fn shannon_entropy_calculation() {
        assert!(shannon_entropy("") == 0.0);
        assert!(shannon_entropy("aaaa") < 1.0);
        assert!(shannon_entropy("abcdefghijklmnop") > 3.0);
    }

    // --- Consistency: redact_text covers everything PiiRedactionGuard covers ---

    #[test]
    fn redact_text_covers_all_pii_pattern_names() {
        let names: Vec<&str> = PII_PATTERNS.iter().map(|p| p.name).collect();
        // Every pattern from guardrails.rs Sprint 85 must be present
        for expected in &[
            "db_connection_string",
            "ssh_private_key",
            "jwt",
            "api_key",
            "credit_card",
            "email",
            "phone_us",
            "ssn",
            "ipv4_address",
        ] {
            assert!(
                names.contains(expected),
                "PII_PATTERNS missing guardrails pattern: {expected}"
            );
        }
        // Every pattern from leak_guard.rs must be present
        for expected in &[
            "anthropic_key",
            "slack_token",
            "github_token",
            "aws_access_key",
            "x25519_key_material",
        ] {
            assert!(
                names.contains(expected),
                "PII_PATTERNS missing leak guard pattern: {expected}"
            );
        }
    }

    // ── Property-based tests ──────────────────────────────────────────
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        /// Any string that contains a known secret pattern must have that
        /// pattern replaced after redaction — the original secret must
        /// never survive `redact_text`.
        #[test]
        fn pii_redaction_never_leaks_known_patterns() {
            let known_secrets = vec![
                ("sk-ant-aBcDeFgHiJkLmNoPqRsT1234", "anthropic_key"),
                ("AKIAIOSFODNN7EXAMPLE", "aws_access_key"),
                ("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef1234", "github_token"),
                ("xoxb-1234567890-abcdefghij", "slack_token"),
                (
                    "postgres://user:pass@db.example.com:5432/mydb",
                    "db_connection_string",
                ),
                ("-----BEGIN RSA PRIVATE KEY-----", "ssh_private_key"),
                (
                    "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456",
                    "jwt",
                ),
            ];
            for (secret, name) in &known_secrets {
                let redacted = redact_text(secret);
                assert!(
                    !redacted.contains(secret),
                    "{name}: secret survived redaction: input={secret}, output={redacted}"
                );
            }
        }

        proptest! {
            /// redact_text must never panic on arbitrary input.
            #[test]
            fn redact_text_never_panics(input in "\\PC{0,500}") {
                let _ = redact_text(&input);
            }

            /// redact_text is idempotent — running it twice produces the same
            /// result as running it once. This ensures redaction markers
            /// themselves are not re-matched.
            #[test]
            fn redact_text_is_idempotent(input in "\\PC{0,200}") {
                let once = redact_text(&input);
                let twice = redact_text(&once);
                prop_assert_eq!(&once, &twice, "redaction is not idempotent");
            }

            /// Text without any secrets or PII-like patterns should pass
            /// through unchanged. We generate only lowercase ASCII letters
            /// and spaces, which can't trigger any pattern.
            #[test]
            fn innocent_text_passes_through(input in "[a-z ]{0,100}") {
                let redacted = redact_text(&input);
                prop_assert_eq!(input, redacted, "innocent text was modified by redaction");
            }

            /// Shannon entropy of a string of identical characters should be 0.
            #[test]
            fn entropy_of_uniform_string_is_zero(c in proptest::char::range('a', 'z'), len in 1usize..50) {
                let s: String = std::iter::repeat(c).take(len).collect();
                let e = shannon_entropy(&s);
                prop_assert!((e - 0.0).abs() < f64::EPSILON, "entropy of uniform string should be 0, got {e}");
            }

            /// Embedding a known API key inside arbitrary prefix/suffix must
            /// always be redacted.
            #[test]
            fn embedded_api_key_always_redacted(
                prefix in "[a-z ]{0,20}",
                suffix in "[a-z ]{0,20}"
            ) {
                let key = "sk-ant-aBcDeFgHiJkLmNoPqRsT1234";
                let input = format!("{prefix}{key}{suffix}");
                let redacted = redact_text(&input);
                prop_assert!(
                    !redacted.contains(key),
                    "API key survived redaction in: {input}"
                );
            }
        }
    }
}

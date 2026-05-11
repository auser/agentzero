use serde::{Deserialize, Serialize};

use crate::DataClassification;

/// A single redaction applied to content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Redaction {
    pub start: usize,
    pub end: usize,
    pub classification: DataClassification,
    pub placeholder: String,
}

/// Result of scanning content for sensitive data.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RedactionResult {
    pub redactions: Vec<Redaction>,
}

impl RedactionResult {
    pub fn is_clean(&self) -> bool {
        self.redactions.is_empty()
    }

    /// Apply the redactions to content, replacing sensitive regions with placeholders.
    ///
    /// Redactions must be sorted by start position and non-overlapping.
    pub fn apply(&self, content: &str) -> String {
        if self.is_clean() {
            return content.to_string();
        }

        let mut output = String::with_capacity(content.len());
        let mut cursor = 0;

        for r in &self.redactions {
            if r.start > cursor {
                output.push_str(&content[cursor..r.start]);
            }
            output.push_str(&r.placeholder);
            cursor = r.end;
        }

        if cursor < content.len() {
            output.push_str(&content[cursor..]);
        }

        output
    }
}

/// Generate a token-preserving placeholder for a redacted region.
///
/// The placeholder preserves enough structure to be useful in context
/// while hiding the actual value.
pub fn placeholder_for(classification: DataClassification, index: usize) -> String {
    let tag = match classification {
        DataClassification::Pii => "PII",
        DataClassification::Secret => "SECRET",
        DataClassification::Credential => "CREDENTIAL",
        DataClassification::Private => "PRIVATE",
        DataClassification::Regulated => "REGULATED",
        _ => "REDACTED",
    };
    format!("[{tag}_{index}]")
}

/// Known patterns for secret and PII detection.
///
/// Each entry is a (prefix, classification) pair. The scanner finds the prefix
/// in content and extends the match to the next word boundary.
const SENSITIVE_PATTERNS: &[(&str, DataClassification)] = &[
    ("@gmail.com", DataClassification::Pii),
    ("@yahoo.com", DataClassification::Pii),
    ("@hotmail.com", DataClassification::Pii),
    ("@outlook.com", DataClassification::Pii),
    ("ghp_", DataClassification::Secret),
    ("gho_", DataClassification::Secret),
    ("sk-", DataClassification::Secret),
    ("AKIA", DataClassification::Secret),
];

/// Scan content for known secret and PII patterns, returning a `RedactionResult`.
///
/// This is the single source of truth for pattern-based redaction scanning.
/// Used by `Session::prepare_for_model()`, audit logging, and tool argument redaction.
pub fn scan_for_secrets(content: &str) -> RedactionResult {
    let lower = content.to_lowercase();
    let mut redactions = Vec::new();

    for (pattern, classification) in SENSITIVE_PATTERNS {
        let pattern_lower = pattern.to_lowercase();
        let mut search_from = 0;
        while let Some(pos) = lower[search_from..].find(&pattern_lower) {
            let abs_pos = search_from + pos;
            let end = content[abs_pos..]
                .find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',')
                .map_or(content.len(), |e| abs_pos + e);

            let idx = redactions.len();
            redactions.push(Redaction {
                start: abs_pos,
                end,
                classification: *classification,
                placeholder: placeholder_for(*classification, idx),
            });
            search_from = end;
        }
    }

    redactions.sort_by_key(|r| r.start);
    RedactionResult { redactions }
}

/// Redact known sensitive patterns from a JSON value for safe storage/display.
///
/// Walks all string values in the JSON structure and replaces detected
/// secrets and PII with placeholders. Used before storing tool arguments
/// in `ToolCallRecord` or displaying in approval prompts.
pub fn redact_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let result = scan_for_secrets(s);
            if result.is_clean() {
                value.clone()
            } else {
                serde_json::Value::String(result.apply(s))
            }
        }
        serde_json::Value::Object(map) => {
            let redacted: serde_json::Map<String, serde_json::Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), redact_json_value(v)))
                .collect();
            serde_json::Value::Object(redacted)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(redact_json_value).collect())
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_result_returns_original() {
        let result = RedactionResult::default();
        assert!(result.is_clean());
        assert_eq!(result.apply("hello world"), "hello world");
    }

    #[test]
    fn single_redaction() {
        let result = RedactionResult {
            redactions: vec![Redaction {
                start: 10,
                end: 30,
                classification: DataClassification::Credential,
                placeholder: "[CREDENTIAL_0]".into(),
            }],
        };
        let input = "my key is AKIAIOSFODNN7EXAMPLE and done";
        let output = result.apply(input);
        assert_eq!(output, "my key is [CREDENTIAL_0] and done");
        assert!(!output.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn multiple_redactions() {
        let result = RedactionResult {
            redactions: vec![
                Redaction {
                    start: 0,
                    end: 5,
                    classification: DataClassification::Pii,
                    placeholder: "[PII_0]".into(),
                },
                Redaction {
                    start: 10,
                    end: 15,
                    classification: DataClassification::Pii,
                    placeholder: "[PII_1]".into(),
                },
            ],
        };
        let output = result.apply("Alice met Bobby today");
        assert_eq!(output, "[PII_0] met [PII_1] today");
    }

    #[test]
    fn placeholder_for_pii() {
        assert_eq!(placeholder_for(DataClassification::Pii, 0), "[PII_0]");
    }

    #[test]
    fn placeholder_for_secret() {
        assert_eq!(placeholder_for(DataClassification::Secret, 3), "[SECRET_3]");
    }

    #[test]
    fn placeholder_for_credential() {
        assert_eq!(
            placeholder_for(DataClassification::Credential, 1),
            "[CREDENTIAL_1]"
        );
    }

    #[test]
    fn placeholder_for_unknown_uses_redacted() {
        assert_eq!(
            placeholder_for(DataClassification::Unknown, 0),
            "[REDACTED_0]"
        );
    }

    #[test]
    fn scan_finds_api_keys() {
        let result = scan_for_secrets("my key is sk-1234567890abcdef done");
        assert!(!result.is_clean());
        let redacted = result.apply("my key is sk-1234567890abcdef done");
        assert!(!redacted.contains("sk-1234567890abcdef"));
        assert!(redacted.contains("[SECRET_"));
    }

    #[test]
    fn scan_finds_github_tokens() {
        let result = scan_for_secrets("token ghp_ABCDabcd1234567890 end");
        assert!(!result.is_clean());
        let redacted = result.apply("token ghp_ABCDabcd1234567890 end");
        assert!(!redacted.contains("ghp_"));
    }

    #[test]
    fn scan_finds_emails() {
        let result = scan_for_secrets("contact user@gmail.com please");
        assert!(!result.is_clean());
        let redacted = result.apply("contact user@gmail.com please");
        assert!(!redacted.contains("@gmail.com"));
        assert!(redacted.contains("[PII_"));
    }

    #[test]
    fn scan_finds_aws_keys() {
        let result = scan_for_secrets("key AKIAIOSFODNN7EXAMPLE here");
        assert!(!result.is_clean());
        let redacted = result.apply("key AKIAIOSFODNN7EXAMPLE here");
        assert!(!redacted.contains("AKIAIOSFODNN7EXAMPLE"));
    }

    #[test]
    fn scan_clean_content() {
        let result = scan_for_secrets("nothing sensitive here");
        assert!(result.is_clean());
    }

    #[test]
    fn redact_json_hides_nested_secrets() {
        let value = serde_json::json!({
            "path": "/tmp/test",
            "content": "API_KEY=sk-secret123456789",
            "nested": {"email": "user@gmail.com"}
        });
        let redacted = redact_json_value(&value);
        let s = redacted.to_string();
        assert!(!s.contains("sk-secret"));
        assert!(!s.contains("@gmail.com"));
        assert!(s.contains("/tmp/test"));
    }

    #[test]
    fn redact_json_preserves_clean() {
        let value = serde_json::json!({"path": "/tmp/safe", "count": 42});
        let redacted = redact_json_value(&value);
        assert_eq!(value, redacted);
    }

    #[test]
    fn redaction_result_serializes() {
        let result = RedactionResult {
            redactions: vec![Redaction {
                start: 0,
                end: 5,
                classification: DataClassification::Pii,
                placeholder: "[PII_0]".into(),
            }],
        };
        let json = serde_json::to_string(&result).expect("should serialize");
        assert!(json.contains("PII_0"));
    }
}

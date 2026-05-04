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

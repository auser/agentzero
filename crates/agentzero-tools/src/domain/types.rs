use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A domain is a pluggable research configuration that defines where to search,
/// how to verify findings, what workflows are available, and how the AI should behave.
///
/// Stored as `.agentzero/domains/{name}.json` (project) or
/// `~/.config/agentzero/domains/{name}.json` (global).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Domain {
    /// Unique identifier (alphanumeric, hyphens, underscores).
    pub name: String,
    /// Human-readable description of what this domain covers.
    pub description: String,
    /// Data sources to search within this domain.
    pub sources: Vec<SourceConfig>,
    /// How to verify findings from this domain.
    pub verification: VerificationConfig,
    /// Pre-built workflow templates specific to this domain.
    #[serde(default)]
    pub workflows: Vec<WorkflowTemplate>,
    /// System prompt fragment injected when working in this domain.
    #[serde(default)]
    pub system_prompt: String,
    /// Quality and formatting constraints.
    #[serde(default)]
    pub constraints: DomainConstraints,
    /// ISO 8601 timestamp of when this domain was created.
    pub created_at: String,
    /// ISO 8601 timestamp of the last update.
    #[serde(default)]
    pub updated_at: String,
    /// Whether the domain is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Configuration for a single data source within a domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    /// Adapter kind: "arxiv", "semantic_scholar", "openalex", "web_search", "http_api".
    pub kind: String,
    /// Human-readable label for this source (e.g. "arXiv", "PubMed").
    pub label: String,
    /// Adapter-specific configuration (API keys, URL templates, headers, field mappings).
    #[serde(default)]
    pub config: serde_json::Value,
    /// Priority ordering (lower = searched first). Default 0.
    #[serde(default)]
    pub priority: i32,
    /// Whether this source is active.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Verification configuration for a domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Ordered list of verification strategies to apply.
    pub strategies: Vec<VerificationStrategy>,
    /// Minimum confidence threshold (0.0-1.0) to consider a finding verified.
    #[serde(default = "default_confidence")]
    pub min_confidence: f64,
}

fn default_confidence() -> f64 {
    0.5
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            strategies: vec![VerificationStrategy::ExistenceCheck],
            min_confidence: default_confidence(),
        }
    }
}

/// A strategy for verifying the accuracy of research findings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VerificationStrategy {
    /// Check that the source URL or DOI resolves successfully.
    ExistenceCheck,
    /// Verify that metadata fields (title, authors, date) match the claimed values.
    MetadataMatch {
        #[serde(default = "default_metadata_fields")]
        fields: Vec<String>,
    },
    /// Spot-check content by fetching and comparing snippets.
    ContentSpotCheck {
        #[serde(default = "default_sample_size")]
        sample_size: usize,
    },
    /// Resolve DOI via doi.org.
    DoiResolve,
    /// Cross-reference findings across multiple sources.
    CrossReference {
        #[serde(default = "default_min_sources")]
        min_sources: usize,
    },
    /// Custom HTTP-based verification check.
    CustomHttp {
        url_template: String,
        #[serde(default = "default_expected_status")]
        expected_status: u16,
    },
}

fn default_metadata_fields() -> Vec<String> {
    vec![
        "title".to_string(),
        "authors".to_string(),
        "year".to_string(),
    ]
}

fn default_sample_size() -> usize {
    3
}

fn default_min_sources() -> usize {
    2
}

fn default_expected_status() -> u16 {
    200
}

/// A pre-built workflow template that creates an SOP when instantiated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTemplate {
    /// Template name (e.g. "literature_review").
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// SOP step titles.
    pub steps: Vec<String>,
    /// Indices of steps that require approval before advancing.
    #[serde(default)]
    pub approval_required: Vec<usize>,
}

/// Quality and formatting constraints for a domain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainConstraints {
    /// Required fields in search results (e.g. ["title", "authors", "year", "doi"]).
    #[serde(default)]
    pub required_fields: Vec<String>,
    /// Preferred output format (e.g. "markdown_table", "bibtex", "apa").
    #[serde(default)]
    pub output_format: String,
    /// Quality filters (e.g. min_citations, min_year, peer_reviewed_only).
    #[serde(default)]
    pub quality_filters: HashMap<String, serde_json::Value>,
}

/// A single search result returned by a source adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub source_kind: String,
    #[serde(default)]
    pub snippet: String,
    /// Flexible metadata: year, doi, citation_count, arxiv_id, etc.
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

/// A finding to be verified by the verification system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingToVerify {
    pub title: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub year: Option<u32>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub claimed_content: Option<String>,
}

/// Result of verifying a single finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub title: String,
    pub status: VerificationStatus,
    pub confidence: f64,
    pub details: Vec<String>,
}

/// Status of a verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Verified,
    Partial,
    NotFound,
    MetadataMismatch,
}

impl std::fmt::Display for VerificationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Verified => write!(f, "verified"),
            Self::Partial => write!(f, "partial"),
            Self::NotFound => write!(f, "not_found"),
            Self::MetadataMismatch => write!(f, "metadata_mismatch"),
        }
    }
}

/// Validate a domain name (alphanumeric + hyphens + underscores).
pub fn validate_domain_name(name: &str) -> anyhow::Result<()> {
    if name.trim().is_empty() {
        anyhow::bail!("domain name must not be empty");
    }
    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "domain name must contain only alphanumeric characters, hyphens, or underscores"
        );
    }
    if name.len() > 64 {
        anyhow::bail!("domain name must be 64 characters or fewer");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_roundtrip_json() {
        let domain = Domain {
            name: "test-domain".to_string(),
            description: "A test domain".to_string(),
            sources: vec![SourceConfig {
                kind: "arxiv".to_string(),
                label: "arXiv".to_string(),
                config: serde_json::json!({}),
                priority: 0,
                enabled: true,
            }],
            verification: VerificationConfig::default(),
            workflows: vec![WorkflowTemplate {
                name: "lit-review".to_string(),
                description: "Literature review".to_string(),
                steps: vec!["Search".to_string(), "Synthesize".to_string()],
                approval_required: vec![1],
            }],
            system_prompt: "Be rigorous.".to_string(),
            constraints: DomainConstraints::default(),
            created_at: "2026-03-16T00:00:00Z".to_string(),
            updated_at: String::new(),
            enabled: true,
        };

        let json = serde_json::to_string_pretty(&domain).expect("serialize should succeed");
        let parsed: Domain = serde_json::from_str(&json).expect("deserialize should succeed");
        assert_eq!(parsed.name, "test-domain");
        assert_eq!(parsed.sources.len(), 1);
        assert_eq!(parsed.sources[0].kind, "arxiv");
        assert_eq!(parsed.workflows.len(), 1);
        assert_eq!(parsed.workflows[0].approval_required, vec![1]);
    }

    #[test]
    fn domain_deserialize_minimal() {
        let json = r#"{
            "name": "minimal",
            "description": "Minimal domain",
            "sources": [],
            "verification": { "strategies": [] },
            "created_at": "2026-01-01T00:00:00Z"
        }"#;
        let domain: Domain = serde_json::from_str(json).expect("minimal should parse");
        assert!(domain.enabled);
        assert!(domain.workflows.is_empty());
        assert!(domain.system_prompt.is_empty());
    }

    #[test]
    fn verification_strategy_tagged_serde() {
        let json = r#"{"type": "metadata_match", "fields": ["title", "year"]}"#;
        let strategy: VerificationStrategy =
            serde_json::from_str(json).expect("should parse tagged variant");
        match strategy {
            VerificationStrategy::MetadataMatch { fields } => {
                assert_eq!(fields, vec!["title", "year"]);
            }
            _ => panic!("expected MetadataMatch"),
        }
    }

    #[test]
    fn validate_domain_name_accepts_valid() {
        validate_domain_name("academic-research").expect("should accept hyphens");
        validate_domain_name("my_domain_123").expect("should accept underscores and digits");
    }

    #[test]
    fn validate_domain_name_rejects_empty() {
        let err = validate_domain_name("").expect_err("empty should fail");
        assert!(err.to_string().contains("must not be empty"));
    }

    #[test]
    fn validate_domain_name_rejects_special_chars() {
        let err = validate_domain_name("bad/name").expect_err("slash should fail");
        assert!(err.to_string().contains("alphanumeric"));
    }

    #[test]
    fn validate_domain_name_rejects_long() {
        let long = "a".repeat(65);
        let err = validate_domain_name(&long).expect_err("too long should fail");
        assert!(err.to_string().contains("64 characters"));
    }

    #[test]
    fn search_result_deserialize_minimal() {
        let json = r#"{"title": "A Paper"}"#;
        let result: SearchResult = serde_json::from_str(json).expect("should parse");
        assert_eq!(result.title, "A Paper");
        assert!(result.authors.is_empty());
        assert!(result.metadata.is_empty());
    }
}

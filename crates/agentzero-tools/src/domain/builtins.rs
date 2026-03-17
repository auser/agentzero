use super::types::{
    Domain, DomainConstraints, SourceConfig, VerificationConfig, VerificationStrategy,
    WorkflowTemplate,
};

/// Get a built-in domain template by name.
pub fn get_builtin(name: &str) -> anyhow::Result<Domain> {
    match name {
        "academic_research" => Ok(academic_research()),
        "competitive_intelligence" => Ok(competitive_intelligence()),
        "patent_search" => Ok(patent_search()),
        _ => {
            anyhow::bail!(
                "unknown built-in template: {name}. Available: academic_research, competitive_intelligence, patent_search"
            )
        }
    }
}

/// List available built-in template names.
pub fn builtin_names() -> &'static [&'static str] {
    &[
        "academic_research",
        "competitive_intelligence",
        "patent_search",
    ]
}

fn academic_research() -> Domain {
    Domain {
        name: "academic_research".to_string(),
        description: "Search and verify academic papers across multiple scholarly databases"
            .to_string(),
        sources: vec![
            SourceConfig {
                kind: "arxiv".to_string(),
                label: "arXiv".to_string(),
                config: serde_json::json!({}),
                priority: 0,
                enabled: true,
            },
            SourceConfig {
                kind: "semantic_scholar".to_string(),
                label: "Semantic Scholar".to_string(),
                config: serde_json::json!({}),
                priority: 1,
                enabled: true,
            },
            SourceConfig {
                kind: "openalex".to_string(),
                label: "OpenAlex".to_string(),
                config: serde_json::json!({}),
                priority: 2,
                enabled: true,
            },
        ],
        verification: VerificationConfig {
            strategies: vec![
                VerificationStrategy::DoiResolve,
                VerificationStrategy::MetadataMatch {
                    fields: vec![
                        "title".to_string(),
                        "authors".to_string(),
                        "year".to_string(),
                    ],
                },
                VerificationStrategy::CrossReference { min_sources: 2 },
            ],
            min_confidence: 0.7,
        },
        workflows: vec![
            WorkflowTemplate {
                name: "literature_review".to_string(),
                description: "Systematic literature review".to_string(),
                steps: vec![
                    "Define research question and scope".to_string(),
                    "Search across academic databases".to_string(),
                    "Screen and filter papers by relevance".to_string(),
                    "Extract key findings from selected papers".to_string(),
                    "Synthesize themes and identify gaps".to_string(),
                    "Write summary with verified citations".to_string(),
                ],
                approval_required: vec![4],
            },
            WorkflowTemplate {
                name: "citation_check".to_string(),
                description: "Verify accuracy of citations".to_string(),
                steps: vec![
                    "Collect all cited papers".to_string(),
                    "Verify each citation exists".to_string(),
                    "Check metadata accuracy".to_string(),
                    "Report discrepancies".to_string(),
                ],
                approval_required: vec![],
            },
        ],
        system_prompt: "You are a rigorous academic researcher. Always cite sources with DOIs when available. Distinguish between peer-reviewed and preprint content. Flag any findings that could not be verified. Use precise language and avoid overclaiming.".to_string(),
        constraints: DomainConstraints {
            required_fields: vec![
                "title".to_string(),
                "authors".to_string(),
                "year".to_string(),
                "url".to_string(),
            ],
            output_format: "markdown_table".to_string(),
            quality_filters: Default::default(),
        },
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        enabled: true,
    }
}

fn competitive_intelligence() -> Domain {
    Domain {
        name: "competitive_intelligence".to_string(),
        description: "Monitor and analyze competitor activity, products, and market positioning"
            .to_string(),
        sources: vec![
            SourceConfig {
                kind: "web_search".to_string(),
                label: "Web Search".to_string(),
                config: serde_json::json!({}),
                priority: 0,
                enabled: true,
            },
            SourceConfig {
                kind: "http_api".to_string(),
                label: "Crunchbase".to_string(),
                config: serde_json::json!({
                    "url_template": "https://api.crunchbase.com/api/v4/searches/organizations?query={{query}}&limit={{max_results}}",
                    "headers": {"X-cb-user-key": "{{env:CRUNCHBASE_API_KEY}}"},
                    "results_path": "entities",
                    "field_map": {
                        "title": "properties.name",
                        "url": "properties.web_path",
                        "snippet": "properties.short_description"
                    }
                }),
                priority: 1,
                enabled: true,
            },
        ],
        verification: VerificationConfig {
            strategies: vec![
                VerificationStrategy::ExistenceCheck,
                VerificationStrategy::ContentSpotCheck { sample_size: 3 },
            ],
            min_confidence: 0.5,
        },
        workflows: vec![WorkflowTemplate {
            name: "competitor_scan".to_string(),
            description: "Scan and analyze competitor landscape".to_string(),
            steps: vec![
                "Identify competitors and search criteria".to_string(),
                "Search for recent activity and announcements".to_string(),
                "Analyze product changes and features".to_string(),
                "Compare market positioning".to_string(),
                "Generate competitive analysis report".to_string(),
            ],
            approval_required: vec![],
        }],
        system_prompt: "You are a competitive intelligence analyst. Focus on factual, verifiable information. Clearly distinguish between confirmed facts and speculation. Include dates and sources for all claims.".to_string(),
        constraints: DomainConstraints {
            required_fields: vec!["title".to_string(), "url".to_string()],
            output_format: "markdown_table".to_string(),
            quality_filters: Default::default(),
        },
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        enabled: true,
    }
}

fn patent_search() -> Domain {
    Domain {
        name: "patent_search".to_string(),
        description: "Search and analyze patent filings across major patent databases".to_string(),
        sources: vec![
            SourceConfig {
                kind: "web_search".to_string(),
                label: "Google Patents Search".to_string(),
                config: serde_json::json!({
                    "query_prefix": "site:patents.google.com "
                }),
                priority: 0,
                enabled: true,
            },
        ],
        verification: VerificationConfig {
            strategies: vec![VerificationStrategy::ExistenceCheck],
            min_confidence: 0.5,
        },
        workflows: vec![WorkflowTemplate {
            name: "prior_art_search".to_string(),
            description: "Search for prior art related to an invention".to_string(),
            steps: vec![
                "Define invention claims and key terms".to_string(),
                "Search patent databases with multiple query strategies".to_string(),
                "Screen results for relevance to claims".to_string(),
                "Analyze closest prior art references".to_string(),
                "Write prior art summary report".to_string(),
            ],
            approval_required: vec![3],
        }],
        system_prompt: "You are a patent research analyst. Focus on identifying relevant prior art. Use precise technical terminology. Note patent numbers, filing dates, and assignees for all references.".to_string(),
        constraints: DomainConstraints {
            required_fields: vec!["title".to_string(), "url".to_string()],
            output_format: "markdown_table".to_string(),
            quality_filters: Default::default(),
        },
        created_at: "2026-01-01T00:00:00Z".to_string(),
        updated_at: "2026-01-01T00:00:00Z".to_string(),
        enabled: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn academic_research_valid() {
        let domain = academic_research();
        assert_eq!(domain.name, "academic_research");
        assert_eq!(domain.sources.len(), 3);
        assert_eq!(domain.workflows.len(), 2);
        assert_eq!(domain.verification.strategies.len(), 3);

        // Should roundtrip through JSON.
        let json = serde_json::to_string_pretty(&domain).expect("serialize");
        let _: Domain = serde_json::from_str(&json).expect("deserialize");
    }

    #[test]
    fn competitive_intelligence_valid() {
        let domain = competitive_intelligence();
        assert_eq!(domain.name, "competitive_intelligence");
        assert_eq!(domain.sources.len(), 2);
        assert_eq!(domain.workflows.len(), 1);
    }

    #[test]
    fn patent_search_valid() {
        let domain = patent_search();
        assert_eq!(domain.name, "patent_search");
        assert_eq!(domain.sources.len(), 1);
    }

    #[test]
    fn get_builtin_returns_template() {
        let domain = get_builtin("academic_research").expect("should return template");
        assert_eq!(domain.name, "academic_research");
    }

    #[test]
    fn get_builtin_rejects_unknown() {
        let err = get_builtin("nonexistent").expect_err("should fail");
        assert!(err.to_string().contains("unknown built-in template"));
    }

    #[test]
    fn builtin_names_returns_all() {
        let names = builtin_names();
        assert!(names.contains(&"academic_research"));
        assert!(names.contains(&"competitive_intelligence"));
        assert!(names.contains(&"patent_search"));
    }
}

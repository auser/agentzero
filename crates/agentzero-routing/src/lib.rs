use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// A model route entry mapping a hint to a specific provider+model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoute {
    pub hint: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub transport: Option<String>,
}

/// An embedding route entry mapping a hint to an embedding provider+model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingRoute {
    pub hint: String,
    pub provider: String,
    pub model: String,
    pub dimensions: Option<usize>,
    pub api_key: Option<String>,
}

/// A query classification rule for automatic model routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationRule {
    pub hint: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    #[serde(default)]
    pub priority: i32,
}

/// Resolved route for a model request.
#[derive(Debug, Clone)]
pub struct ResolvedRoute {
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub transport: Option<String>,
    pub matched_hint: String,
}

/// Resolved route for an embedding request.
#[derive(Debug, Clone)]
pub struct ResolvedEmbeddingRoute {
    pub provider: String,
    pub model: String,
    pub dimensions: Option<usize>,
    pub api_key: Option<String>,
    pub matched_hint: String,
}

/// The model router resolves hints and classifies queries to select routes.
#[derive(Debug, Clone, Default)]
pub struct ModelRouter {
    pub model_routes: Vec<ModelRoute>,
    pub embedding_routes: Vec<EmbeddingRoute>,
    pub classification_rules: Vec<ClassificationRule>,
    pub classification_enabled: bool,
}

impl ModelRouter {
    /// Resolve a model route by explicit hint name.
    pub fn resolve_hint(&self, hint: &str) -> Option<ResolvedRoute> {
        self.model_routes
            .iter()
            .find(|r| r.hint.eq_ignore_ascii_case(hint))
            .map(|r| ResolvedRoute {
                provider: r.provider.clone(),
                model: r.model.clone(),
                max_tokens: r.max_tokens,
                api_key: r.api_key.clone(),
                transport: r.transport.clone(),
                matched_hint: r.hint.clone(),
            })
    }

    /// Resolve an embedding route by explicit hint name.
    pub fn resolve_embedding_hint(&self, hint: &str) -> Option<ResolvedEmbeddingRoute> {
        self.embedding_routes
            .iter()
            .find(|r| r.hint.eq_ignore_ascii_case(hint))
            .map(|r| ResolvedEmbeddingRoute {
                provider: r.provider.clone(),
                model: r.model.clone(),
                dimensions: r.dimensions,
                api_key: r.api_key.clone(),
                matched_hint: r.hint.clone(),
            })
    }

    /// Classify a query and return the best matching hint.
    pub fn classify_query(&self, query: &str) -> Option<String> {
        if !self.classification_enabled || self.classification_rules.is_empty() {
            return None;
        }

        let query_lower = query.to_lowercase();
        let query_len = query.len();

        let mut best_hint: Option<&str> = None;
        let mut best_priority = i32::MIN;

        for rule in &self.classification_rules {
            // Length filters.
            if let Some(min) = rule.min_length {
                if query_len < min {
                    continue;
                }
            }
            if let Some(max) = rule.max_length {
                if query_len > max {
                    continue;
                }
            }

            // Keyword match (any keyword present).
            let keyword_match = rule.keywords.is_empty()
                || rule
                    .keywords
                    .iter()
                    .any(|kw| query_lower.contains(&kw.to_lowercase()));

            // Pattern match (any regex matches).
            let pattern_match = rule.patterns.is_empty()
                || rule
                    .patterns
                    .iter()
                    .any(|p| Regex::new(p).map(|re| re.is_match(query)).unwrap_or(false));

            if keyword_match && pattern_match && rule.priority > best_priority {
                best_priority = rule.priority;
                best_hint = Some(&rule.hint);
            }
        }

        if let Some(hint) = best_hint {
            debug!(hint, "query classified");
        }

        best_hint.map(String::from)
    }

    /// Classify a query and resolve to a model route in one step.
    pub fn route_query(&self, query: &str) -> Option<ResolvedRoute> {
        self.classify_query(query)
            .and_then(|hint| self.resolve_hint(&hint))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> ModelRouter {
        ModelRouter {
            model_routes: vec![
                ModelRoute {
                    hint: "reasoning".into(),
                    provider: "openrouter".into(),
                    model: "anthropic/claude-opus-4-6".into(),
                    max_tokens: Some(8192),
                    api_key: None,
                    transport: None,
                },
                ModelRoute {
                    hint: "fast".into(),
                    provider: "openrouter".into(),
                    model: "anthropic/claude-haiku-4-5".into(),
                    max_tokens: None,
                    api_key: None,
                    transport: None,
                },
                ModelRoute {
                    hint: "code".into(),
                    provider: "openrouter".into(),
                    model: "anthropic/claude-sonnet-4-6".into(),
                    max_tokens: Some(16384),
                    api_key: None,
                    transport: None,
                },
            ],
            embedding_routes: vec![EmbeddingRoute {
                hint: "default".into(),
                provider: "openai".into(),
                model: "text-embedding-3-small".into(),
                dimensions: Some(1536),
                api_key: None,
            }],
            classification_rules: vec![
                ClassificationRule {
                    hint: "reasoning".into(),
                    keywords: vec!["explain".into(), "why".into(), "analyze".into()],
                    patterns: Vec::new(),
                    min_length: Some(50),
                    max_length: None,
                    priority: 10,
                },
                ClassificationRule {
                    hint: "fast".into(),
                    keywords: vec![],
                    patterns: Vec::new(),
                    min_length: None,
                    max_length: Some(20),
                    priority: 5,
                },
                ClassificationRule {
                    hint: "code".into(),
                    keywords: vec![
                        "implement".into(),
                        "function".into(),
                        "code".into(),
                        "fix".into(),
                    ],
                    patterns: Vec::new(),
                    min_length: None,
                    max_length: None,
                    priority: 8,
                },
            ],
            classification_enabled: true,
        }
    }

    #[test]
    fn resolve_hint_finds_matching_route() {
        let r = router();
        let route = r.resolve_hint("fast").unwrap();
        assert_eq!(route.model, "anthropic/claude-haiku-4-5");
    }

    #[test]
    fn resolve_hint_returns_none_for_unknown() {
        let r = router();
        assert!(r.resolve_hint("nonexistent").is_none());
    }

    #[test]
    fn resolve_embedding_hint() {
        let r = router();
        let route = r.resolve_embedding_hint("default").unwrap();
        assert_eq!(route.model, "text-embedding-3-small");
        assert_eq!(route.dimensions, Some(1536));
    }

    #[test]
    fn classify_query_by_keywords() {
        let r = router();
        let hint = r
            .classify_query("please implement a function to parse JSON")
            .unwrap();
        assert_eq!(hint, "code");
    }

    #[test]
    fn classify_query_by_length() {
        let r = router();
        let hint = r.classify_query("hello").unwrap();
        assert_eq!(hint, "fast");
    }

    #[test]
    fn classify_query_priority_wins() {
        let r = router();
        // Long query with "explain" and "implement" → reasoning wins (priority 10 > code 8).
        let hint = r
            .classify_query(
                "explain why this function fails and implement a fix for the memory leak issue",
            )
            .unwrap();
        assert_eq!(hint, "reasoning");
    }

    #[test]
    fn classify_disabled_returns_none() {
        let mut r = router();
        r.classification_enabled = false;
        assert!(r.classify_query("explain this").is_none());
    }

    #[test]
    fn route_query_resolves_end_to_end() {
        let r = router();
        let route = r
            .route_query("implement a function to sort arrays")
            .unwrap();
        assert_eq!(route.model, "anthropic/claude-sonnet-4-6");
        assert_eq!(route.matched_hint, "code");
    }
}

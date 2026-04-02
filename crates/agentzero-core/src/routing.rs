use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Privacy level for model routes — controls which routes are eligible
/// based on the active privacy mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyLevel {
    /// Only eligible when running locally (local_only mode).
    Local,
    /// Only eligible when cloud access is allowed.
    Cloud,
    /// Eligible in any privacy mode (default).
    #[default]
    Either,
}

/// A model route entry mapping a hint to a specific provider+model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRoute {
    pub hint: String,
    pub provider: String,
    pub model: String,
    pub max_tokens: Option<usize>,
    pub api_key: Option<String>,
    pub transport: Option<String>,
    /// Privacy level controlling route eligibility by privacy mode.
    #[serde(default)]
    pub privacy_level: PrivacyLevel,
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

    /// Classify by complexity tier and resolve to a matching route.
    ///
    /// Uses the complexity scorer to determine Simple/Medium/Complex,
    /// then looks for routes with hints matching the tier name.
    /// Falls back to rule-based classification if no complexity route matches.
    pub fn route_by_complexity(
        &self,
        query: &str,
        config: &crate::complexity::ComplexityConfig,
    ) -> Option<ResolvedRoute> {
        let score = crate::complexity::score(query, config);
        let tier_hint = match score.tier {
            crate::complexity::ComplexityTier::Simple => "simple",
            crate::complexity::ComplexityTier::Medium => "medium",
            crate::complexity::ComplexityTier::Complex => "complex",
        };
        debug!(
            tier = tier_hint,
            composite = score.composite,
            "complexity classification"
        );
        // Try to resolve a route matching the tier name.
        if let Some(route) = self.resolve_hint(tier_hint) {
            return Some(route);
        }
        // Fallback to rule-based classification.
        self.route_query(query)
    }

    /// Resolve a hint with privacy filtering.
    ///
    /// - `"local_only"`: only `Local` routes
    /// - `"private"`: prefer `Local`, fall through to `Cloud`
    /// - `"off"` / other: all routes (current behavior)
    pub fn resolve_hint_with_privacy(
        &self,
        hint: &str,
        privacy_mode: &str,
    ) -> Option<ResolvedRoute> {
        let candidates: Vec<&ModelRoute> = self
            .model_routes
            .iter()
            .filter(|r| r.hint.eq_ignore_ascii_case(hint))
            .collect();

        match privacy_mode {
            "local_only" => candidates
                .iter()
                .find(|r| r.privacy_level == PrivacyLevel::Local)
                .map(|r| self.route_to_resolved(r)),
            "private" => {
                // Prefer local, fall through to cloud/either.
                candidates
                    .iter()
                    .find(|r| r.privacy_level == PrivacyLevel::Local)
                    .or_else(|| {
                        candidates
                            .iter()
                            .find(|r| r.privacy_level != PrivacyLevel::Local)
                    })
                    .map(|r| self.route_to_resolved(r))
            }
            _ => candidates.first().map(|r| self.route_to_resolved(r)),
        }
    }

    /// Classify a query and resolve with privacy filtering.
    pub fn route_query_with_privacy(
        &self,
        query: &str,
        privacy_mode: &str,
    ) -> Option<ResolvedRoute> {
        self.classify_query(query)
            .and_then(|hint| self.resolve_hint_with_privacy(&hint, privacy_mode))
    }

    fn route_to_resolved(&self, r: &ModelRoute) -> ResolvedRoute {
        ResolvedRoute {
            provider: r.provider.clone(),
            model: r.model.clone(),
            max_tokens: r.max_tokens,
            api_key: r.api_key.clone(),
            transport: r.transport.clone(),
            matched_hint: r.hint.clone(),
        }
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
                    privacy_level: PrivacyLevel::Either,
                },
                ModelRoute {
                    hint: "fast".into(),
                    provider: "openrouter".into(),
                    model: "anthropic/claude-haiku-4-5".into(),
                    max_tokens: None,
                    api_key: None,
                    transport: None,
                    privacy_level: PrivacyLevel::Either,
                },
                ModelRoute {
                    hint: "code".into(),
                    provider: "openrouter".into(),
                    model: "anthropic/claude-sonnet-4-6".into(),
                    max_tokens: Some(16384),
                    api_key: None,
                    transport: None,
                    privacy_level: PrivacyLevel::Either,
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

    // --- Privacy-aware routing tests ---

    fn privacy_router() -> ModelRouter {
        ModelRouter {
            model_routes: vec![
                ModelRoute {
                    hint: "fast".into(),
                    provider: "ollama".into(),
                    model: "llama3.2".into(),
                    max_tokens: None,
                    api_key: None,
                    transport: None,
                    privacy_level: PrivacyLevel::Local,
                },
                ModelRoute {
                    hint: "fast".into(),
                    provider: "anthropic".into(),
                    model: "claude-haiku-4-5".into(),
                    max_tokens: None,
                    api_key: None,
                    transport: None,
                    privacy_level: PrivacyLevel::Cloud,
                },
                ModelRoute {
                    hint: "reasoning".into(),
                    provider: "openai".into(),
                    model: "o1".into(),
                    max_tokens: Some(8192),
                    api_key: None,
                    transport: None,
                    privacy_level: PrivacyLevel::Either,
                },
            ],
            embedding_routes: vec![],
            classification_rules: vec![],
            classification_enabled: false,
        }
    }

    #[test]
    fn private_mode_prefers_local_route() {
        let r = privacy_router();
        let route = r
            .resolve_hint_with_privacy("fast", "private")
            .expect("should resolve");
        assert_eq!(route.provider, "ollama", "private should prefer local");
    }

    #[test]
    fn private_mode_falls_through_to_cloud() {
        let r = privacy_router();
        // "reasoning" has no Local route, only Either — should still resolve.
        let route = r
            .resolve_hint_with_privacy("reasoning", "private")
            .expect("should fall through");
        assert_eq!(route.provider, "openai");
    }

    #[test]
    fn local_only_blocks_cloud_routes() {
        let r = privacy_router();
        let route = r.resolve_hint_with_privacy("fast", "local_only");
        assert_eq!(
            route.as_ref().map(|r| r.provider.as_str()),
            Some("ollama"),
            "local_only should only return local routes"
        );
    }

    #[test]
    fn local_only_returns_none_for_cloud_only() {
        let r = ModelRouter {
            model_routes: vec![ModelRoute {
                hint: "cloud-only".into(),
                provider: "anthropic".into(),
                model: "claude-sonnet-4-6".into(),
                max_tokens: None,
                api_key: None,
                transport: None,
                privacy_level: PrivacyLevel::Cloud,
            }],
            ..Default::default()
        };
        assert!(
            r.resolve_hint_with_privacy("cloud-only", "local_only")
                .is_none(),
            "local_only should not resolve cloud-only routes"
        );
    }

    #[test]
    fn off_mode_allows_all_routes() {
        let r = privacy_router();
        let route = r
            .resolve_hint_with_privacy("fast", "off")
            .expect("should resolve");
        // "off" mode returns the first matching route (local in this case).
        assert!(!route.provider.is_empty());
    }

    #[test]
    fn privacy_level_defaults_to_either() {
        let r = router(); // Uses the existing test router (privacy_level = Either)
                          // "off" mode accepts all privacy levels including Either.
        let route = r
            .resolve_hint_with_privacy("fast", "off")
            .expect("Either routes should be available in off mode");
        assert_eq!(route.provider, "openrouter");
        // "private" also accepts Either (falls through from Local).
        let route2 = r
            .resolve_hint_with_privacy("fast", "private")
            .expect("Either routes should be available in private mode");
        assert_eq!(route2.provider, "openrouter");
    }
}

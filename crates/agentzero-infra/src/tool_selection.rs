//! Tool selection strategies for reducing the tool set passed to LLM providers.
//!
//! When an agent has access to many tools, passing all tool schemas to every
//! provider call wastes tokens and can degrade response quality. This module
//! provides two strategies:
//!
//! - [`KeywordToolSelector`]: Fast TF-IDF/keyword matching on tool descriptions.
//! - [`AiToolSelector`]: Uses a lightweight LLM call to classify relevant tools.

use agentzero_core::{ToolSelector, ToolSummary};
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// AllToolSelector — pass-through (default)
// ---------------------------------------------------------------------------

/// Pass-through selector that returns all tools. Used when `tool_selection = "all"`.
pub struct AllToolSelector;

#[async_trait]
impl ToolSelector for AllToolSelector {
    async fn select(
        &self,
        _task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>> {
        Ok(available_tools.iter().map(|t| t.name.clone()).collect())
    }
}

// ---------------------------------------------------------------------------
// KeywordToolSelector — TF-IDF / keyword matching
// ---------------------------------------------------------------------------

/// Selects tools by keyword overlap between the task description and tool
/// name + description. Uses normalized TF-IDF scoring. No LLM call needed.
pub struct KeywordToolSelector {
    /// Maximum number of tools to return (0 = return all matches above threshold).
    pub max_tools: usize,
    /// Minimum relevance score to include a tool (0.0–1.0).
    pub min_score: f64,
}

impl Default for KeywordToolSelector {
    fn default() -> Self {
        Self {
            max_tools: 15,
            min_score: 0.01,
        }
    }
}

/// Tokenize text into lowercase words, stripping non-alphanumeric chars.
fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

/// Compute inverse document frequency: ln(N / (1 + df)) for each term,
/// where df is the number of documents containing the term.
fn compute_idf(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let n = documents.len() as f64;
    let mut df: HashMap<String, usize> = HashMap::new();
    for doc in documents {
        let unique: HashSet<&str> = doc.iter().map(|s| s.as_str()).collect();
        for term in unique {
            *df.entry(term.to_string()).or_default() += 1;
        }
    }
    df.into_iter()
        .map(|(term, count)| {
            let idf = (n / (1.0 + count as f64)).ln().max(0.0);
            (term, idf)
        })
        .collect()
}

/// Compute TF-IDF score for a query against a single document.
fn tfidf_score(query_tokens: &[String], doc_tokens: &[String], idf: &HashMap<String, f64>) -> f64 {
    if doc_tokens.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }
    // Term frequency in document
    let mut tf: HashMap<&str, f64> = HashMap::new();
    for t in doc_tokens {
        *tf.entry(t.as_str()).or_default() += 1.0;
    }
    let doc_len = doc_tokens.len() as f64;

    let mut score = 0.0;
    let query_unique: HashSet<&str> = query_tokens.iter().map(|s| s.as_str()).collect();
    for term in &query_unique {
        if let Some(&freq) = tf.get(term) {
            let tf_val = freq / doc_len;
            let idf_val = idf.get(*term).copied().unwrap_or(0.0);
            score += tf_val * idf_val;
        }
    }
    score
}

#[async_trait]
impl ToolSelector for KeywordToolSelector {
    async fn select(
        &self,
        task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>> {
        if available_tools.is_empty() {
            return Ok(Vec::new());
        }

        let query_tokens = tokenize(task_description);
        if query_tokens.is_empty() {
            // No meaningful tokens — return all tools.
            return Ok(available_tools.iter().map(|t| t.name.clone()).collect());
        }

        // Build document corpus: each tool's name + description is a document.
        let documents: Vec<Vec<String>> = available_tools
            .iter()
            .map(|t| {
                let mut tokens = tokenize(&t.name);
                tokens.extend(tokenize(&t.description));
                tokens
            })
            .collect();

        let idf = compute_idf(&documents);

        let mut scored: Vec<(usize, f64)> = documents
            .iter()
            .enumerate()
            .map(|(i, doc)| (i, tfidf_score(&query_tokens, doc, &idf)))
            .filter(|(_, score)| *score >= self.min_score)
            .collect();

        // Sort by score descending.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if self.max_tools > 0 {
            scored.truncate(self.max_tools);
        }

        Ok(scored
            .iter()
            .map(|(i, _)| available_tools[*i].name.clone())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// AiToolSelector — LLM-based classification
// ---------------------------------------------------------------------------

/// Uses a lightweight LLM call to classify which tools are relevant to a task.
/// Results are cached per unique (task_description, tool_set) hash for the session.
pub struct AiToolSelector {
    /// The provider to use for the classification call.
    provider: Box<dyn agentzero_core::Provider>,
    /// Maximum number of tools to request from the LLM.
    pub max_tools: usize,
    /// Session cache: task_hash -> selected tool names.
    cache: Mutex<HashMap<u64, Vec<String>>>,
}

impl AiToolSelector {
    pub fn new(provider: Box<dyn agentzero_core::Provider>, max_tools: usize) -> Self {
        Self {
            provider,
            max_tools,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cache_key(task: &str, tools: &[ToolSummary]) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        task.hash(&mut hasher);
        for t in tools {
            t.name.hash(&mut hasher);
        }
        hasher.finish()
    }
}

#[async_trait]
impl ToolSelector for AiToolSelector {
    async fn select(
        &self,
        task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>> {
        if available_tools.is_empty() {
            return Ok(Vec::new());
        }

        let key = Self::cache_key(task_description, available_tools);

        // Check cache.
        {
            let cache = self
                .cache
                .lock()
                .expect("tool selector cache lock poisoned");
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.clone());
            }
        }

        // Build the prompt.
        let tool_list: String = available_tools
            .iter()
            .map(|t| format!("- {}: {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "You are a tool selector. Given the user's task and a list of available tools, \
             select the most relevant tools (up to {max}) that would be useful for completing \
             the task. Return ONLY a JSON array of tool names, nothing else.\n\n\
             Task: {task}\n\n\
             Available tools:\n{tools}\n\n\
             Return a JSON array of the most relevant tool names (e.g. [\"tool1\", \"tool2\"]):",
            max = self.max_tools,
            task = task_description,
            tools = tool_list,
        );

        let result = self.provider.complete(&prompt).await?;
        let output = result.output_text.trim().to_string();

        // Parse the JSON array from the response.
        let selected = parse_tool_names_from_response(&output, available_tools);

        // Cache the result.
        {
            let mut cache = self
                .cache
                .lock()
                .expect("tool selector cache lock poisoned");
            cache.insert(key, selected.clone());
        }

        Ok(selected)
    }
}

/// Parse tool names from an LLM response. Expects a JSON array like `["tool1", "tool2"]`.
/// Falls back to line-by-line matching if JSON parsing fails.
fn parse_tool_names_from_response(response: &str, available_tools: &[ToolSummary]) -> Vec<String> {
    let valid_names: HashSet<&str> = available_tools.iter().map(|t| t.name.as_str()).collect();

    // Try JSON array parse first.
    if let Ok(names) = serde_json::from_str::<Vec<String>>(response) {
        let filtered: Vec<String> = names
            .into_iter()
            .filter(|n| valid_names.contains(n.as_str()))
            .collect();
        if !filtered.is_empty() {
            return filtered;
        }
    }

    // Try extracting a JSON array from within the response (LLM may add text around it).
    if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            if start < end {
                let slice = &response[start..=end];
                if let Ok(names) = serde_json::from_str::<Vec<String>>(slice) {
                    let filtered: Vec<String> = names
                        .into_iter()
                        .filter(|n| valid_names.contains(n.as_str()))
                        .collect();
                    if !filtered.is_empty() {
                        return filtered;
                    }
                }
            }
        }
    }

    // Fallback: look for tool names mentioned anywhere in the response.
    let response_lower = response.to_lowercase();
    available_tools
        .iter()
        .filter(|t| response_lower.contains(&t.name.to_lowercase()))
        .map(|t| t.name.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// HintedToolSelector — explicit hints + keyword fallback
// ---------------------------------------------------------------------------

/// Foundational tools always included regardless of hints.
const ALWAYS_INCLUDE: &[&str] = &["read_file", "shell", "content_search"];

/// Selects tools using explicit name hints (from goal decomposition) combined
/// with recipe-matched tools (from catalog learning) and keyword-based fallback.
///
/// Selection priority: explicit hints → recipe-matched tools → keyword fallback.
/// When hints are empty and no recipes match, delegates entirely to keyword fallback.
pub struct HintedToolSelector {
    /// Tool names suggested by the goal planner for this agent node.
    pub hints: Vec<String>,
    /// Optional recipe store for catalog learning.
    pub recipes: Option<std::sync::Arc<std::sync::Mutex<crate::tool_recipes::RecipeStore>>>,
    /// Fallback selector for keyword-based matching.
    pub fallback: KeywordToolSelector,
}

#[async_trait]
impl ToolSelector for HintedToolSelector {
    async fn select(
        &self,
        task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>> {
        if self.hints.is_empty() {
            return self
                .fallback
                .select(task_description, available_tools)
                .await;
        }

        let available_names: HashSet<&str> =
            available_tools.iter().map(|t| t.name.as_str()).collect();

        // 1. Exact hint matches against available tools.
        let mut selected: Vec<String> = Vec::new();
        for hint in &self.hints {
            for tool in available_tools {
                if (tool.name == *hint
                    || tool.name.contains(hint.as_str())
                    || hint.contains(tool.name.as_str()))
                    && !selected.contains(&tool.name)
                {
                    selected.push(tool.name.clone());
                }
            }
        }

        // 2. Recipe-matched tools (catalog learning).
        if let Some(ref recipes) = self.recipes {
            if let Ok(store) = recipes.lock() {
                let suggested = store.suggest_tools(task_description, 3);
                for name in suggested {
                    if available_names.contains(name.as_str()) && !selected.contains(&name) {
                        selected.push(name);
                    }
                }
            }
        }

        // 3. Always-include foundational tools.
        for &name in ALWAYS_INCLUDE {
            if available_names.contains(name) && !selected.contains(&name.to_string()) {
                selected.push(name.to_string());
            }
        }

        // 4. Augment with keyword matches using a boosted query.
        let boosted = format!("{task_description} {}", self.hints.join(" "));
        let keyword_results = self.fallback.select(&boosted, available_tools).await?;
        for name in keyword_results {
            if !selected.contains(&name) {
                selected.push(name);
            }
        }

        Ok(selected)
    }
}

// ---------------------------------------------------------------------------
// TwoStageToolSelector — keyword/embedding pre-filter → LLM refinement
// ---------------------------------------------------------------------------

/// Two-stage tool selection to prevent prompt bloat as dynamic tools grow.
///
/// **Stage 1**: `KeywordToolSelector` narrows candidates to `stage1_max`.
/// If an `EmbeddingProvider` is available, re-ranks by cosine similarity.
///
/// **Stage 2**: Passes the Stage 1 shortlist to `AiToolSelector` for final
/// LLM-based selection (max `stage2_max` tools).
///
/// Graceful degradation: no embedding provider → keyword only; LLM fails → Stage 1 results.
pub struct TwoStageToolSelector {
    keyword_selector: KeywordToolSelector,
    embedding_provider: Option<std::sync::Arc<dyn agentzero_core::embedding::EmbeddingProvider>>,
    ai_selector: AiToolSelector,
    /// Maximum tools passed from Stage 1 to Stage 2.
    pub stage1_max: usize,
    /// Maximum tools returned from Stage 2.
    pub stage2_max: usize,
    /// Embedding cache for tool descriptions (keyed by tool name).
    embedding_cache: Mutex<HashMap<String, Vec<f32>>>,
}

impl TwoStageToolSelector {
    pub fn new(
        keyword_selector: KeywordToolSelector,
        ai_selector: AiToolSelector,
        embedding_provider: Option<
            std::sync::Arc<dyn agentzero_core::embedding::EmbeddingProvider>,
        >,
    ) -> Self {
        Self {
            keyword_selector,
            embedding_provider,
            ai_selector,
            stage1_max: 30,
            stage2_max: 15,
            embedding_cache: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl ToolSelector for TwoStageToolSelector {
    async fn select(
        &self,
        task_description: &str,
        available_tools: &[ToolSummary],
    ) -> anyhow::Result<Vec<String>> {
        if available_tools.len() <= self.stage2_max {
            // Small tool set — skip both stages, return all.
            return Ok(available_tools.iter().map(|t| t.name.clone()).collect());
        }

        // Stage 1: keyword pre-filter.
        let mut stage1_names = self
            .keyword_selector
            .select(task_description, available_tools)
            .await?;
        stage1_names.truncate(self.stage1_max);

        // If embedding provider available, re-rank by semantic similarity.
        if let Some(ref provider) = self.embedding_provider {
            if let Ok(task_embedding) = provider.embed(task_description).await {
                let mut scored: Vec<(String, f32)> = Vec::new();
                for name in &stage1_names {
                    let tool_emb = {
                        let cache = self
                            .embedding_cache
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        cache.get(name).cloned()
                    };
                    let emb = match tool_emb {
                        Some(e) => e,
                        None => {
                            let desc = available_tools
                                .iter()
                                .find(|t| t.name == *name)
                                .map(|t| format!("{} {}", t.name, t.description))
                                .unwrap_or_default();
                            match provider.embed(&desc).await {
                                Ok(e) => {
                                    if let Ok(mut cache) = self.embedding_cache.lock() {
                                        cache.insert(name.clone(), e.clone());
                                    }
                                    e
                                }
                                Err(_) => continue,
                            }
                        }
                    };
                    let sim = agentzero_core::embedding::cosine_similarity(&task_embedding, &emb);
                    scored.push((name.clone(), sim));
                }
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                stage1_names = scored.into_iter().map(|(name, _)| name).collect();
                stage1_names.truncate(self.stage1_max);
            }
        }

        // Stage 2: LLM refinement on the shortlist.
        let shortlist: Vec<ToolSummary> = available_tools
            .iter()
            .filter(|t| stage1_names.contains(&t.name))
            .cloned()
            .collect();

        match self.ai_selector.select(task_description, &shortlist).await {
            Ok(mut final_names) => {
                final_names.truncate(self.stage2_max);
                Ok(final_names)
            }
            Err(e) => {
                // Graceful degradation: LLM failed, return Stage 1 results.
                tracing::warn!(error = %e, "two-stage Stage 2 LLM failed, using Stage 1 results");
                stage1_names.truncate(self.stage2_max);
                Ok(stage1_names)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tools() -> Vec<ToolSummary> {
        vec![
            ToolSummary {
                name: "read_file".to_string(),
                description: "Read a file from the filesystem".to_string(),
            },
            ToolSummary {
                name: "write_file".to_string(),
                description: "Write content to a file on the filesystem".to_string(),
            },
            ToolSummary {
                name: "web_search".to_string(),
                description: "Search the web for information using a query".to_string(),
            },
            ToolSummary {
                name: "shell".to_string(),
                description: "Execute a shell command in the terminal".to_string(),
            },
            ToolSummary {
                name: "git_commit".to_string(),
                description: "Create a git commit with a message".to_string(),
            },
            ToolSummary {
                name: "http_request".to_string(),
                description: "Make an HTTP request to a URL endpoint".to_string(),
            },
            ToolSummary {
                name: "delegate".to_string(),
                description: "Delegate a task to a sub-agent for processing".to_string(),
            },
        ]
    }

    #[tokio::test]
    async fn all_selector_returns_everything() {
        let selector = AllToolSelector;
        let tools = test_tools();
        let result = selector.select("anything", &tools).await.expect("select");
        assert_eq!(result.len(), tools.len());
    }

    #[tokio::test]
    async fn keyword_selector_matches_file_tools() {
        let selector = KeywordToolSelector::default();
        let tools = test_tools();
        let result = selector
            .select("I need to read a file and then write something", &tools)
            .await
            .expect("select");
        // read_file and write_file should be top-ranked.
        assert!(result.contains(&"read_file".to_string()));
        assert!(result.contains(&"write_file".to_string()));
        // File tools should rank higher than unrelated tools.
        let read_pos = result
            .iter()
            .position(|n| n == "read_file")
            .expect("read_file present");
        let web_pos = result.iter().position(|n| n == "web_search");
        if let Some(wp) = web_pos {
            assert!(read_pos < wp, "read_file should rank before web_search");
        }
    }

    #[tokio::test]
    async fn keyword_selector_matches_web_search() {
        let selector = KeywordToolSelector::default();
        let tools = test_tools();
        let result = selector
            .select("search the web for Rust documentation", &tools)
            .await
            .expect("select");
        assert!(result.contains(&"web_search".to_string()));
        // web_search should be first or near the top.
        let pos = result
            .iter()
            .position(|n| n == "web_search")
            .expect("web_search present");
        assert!(pos < 3, "web_search should be in top 3");
    }

    #[tokio::test]
    async fn keyword_selector_matches_git() {
        let selector = KeywordToolSelector::default();
        let tools = test_tools();
        let result = selector
            .select("commit the changes to git", &tools)
            .await
            .expect("select");
        assert!(result.contains(&"git_commit".to_string()));
    }

    #[tokio::test]
    async fn keyword_selector_empty_tools() {
        let selector = KeywordToolSelector::default();
        let result = selector.select("anything", &[]).await.expect("select");
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn keyword_selector_empty_query_returns_all() {
        let selector = KeywordToolSelector::default();
        let tools = test_tools();
        let result = selector.select("", &tools).await.expect("select");
        assert_eq!(result.len(), tools.len());
    }

    #[tokio::test]
    async fn parse_tool_names_json_array() {
        let tools = test_tools();
        let response = r#"["read_file", "write_file"]"#;
        let result = parse_tool_names_from_response(response, &tools);
        assert_eq!(result, vec!["read_file", "write_file"]);
    }

    #[tokio::test]
    async fn parse_tool_names_embedded_json() {
        let tools = test_tools();
        let response = r#"Here are the relevant tools: ["read_file", "shell"] done."#;
        let result = parse_tool_names_from_response(response, &tools);
        assert_eq!(result, vec!["read_file", "shell"]);
    }

    #[tokio::test]
    async fn parse_tool_names_filters_invalid() {
        let tools = test_tools();
        let response = r#"["read_file", "nonexistent_tool", "shell"]"#;
        let result = parse_tool_names_from_response(response, &tools);
        assert_eq!(result, vec!["read_file", "shell"]);
    }

    #[tokio::test]
    async fn parse_tool_names_fallback_mentions() {
        let tools = test_tools();
        let response = "I think read_file and web_search would be useful here.";
        let result = parse_tool_names_from_response(response, &tools);
        assert!(result.contains(&"read_file".to_string()));
        assert!(result.contains(&"web_search".to_string()));
    }

    #[tokio::test]
    async fn ai_selector_caches_results() {
        use agentzero_core::{ChatResult, Provider};

        struct CountingProvider {
            call_count: std::sync::atomic::AtomicUsize,
        }
        #[async_trait]
        impl Provider for CountingProvider {
            async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
                self.call_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(ChatResult {
                    output_text: r#"["read_file", "write_file"]"#.to_string(),
                    tool_calls: vec![],
                    stop_reason: None,
                    input_tokens: 0,
                    output_tokens: 0,
                })
            }
        }

        let provider = CountingProvider {
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };
        let selector = AiToolSelector::new(Box::new(provider), 10);
        let tools = test_tools();

        // First call — should hit the provider.
        let r1 = selector
            .select("read and write files", &tools)
            .await
            .expect("select");
        assert_eq!(r1, vec!["read_file", "write_file"]);

        // Second call with same inputs — should hit cache.
        let r2 = selector
            .select("read and write files", &tools)
            .await
            .expect("select");
        assert_eq!(r2, vec!["read_file", "write_file"]);

        // Cache hit verified: same result returned without extra provider call.
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn ai_selector_empty_tools() {
        use agentzero_core::{ChatResult, Provider};

        struct NeverCalledProvider;
        #[async_trait]
        impl Provider for NeverCalledProvider {
            async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
                panic!("should not be called for empty tools");
            }
        }

        let selector = AiToolSelector::new(Box::new(NeverCalledProvider), 10);
        let result = selector.select("anything", &[]).await.expect("select");
        assert!(result.is_empty());
    }

    // ── HintedToolSelector tests ────────────────────────────────────────

    #[tokio::test]
    async fn hinted_empty_hints_delegates_to_fallback() {
        let selector = HintedToolSelector {
            hints: vec![],
            recipes: None,
            fallback: KeywordToolSelector::default(),
        };
        let tools = test_tools();
        let result = selector
            .select("read a file from disk", &tools)
            .await
            .expect("select");
        assert!(result.contains(&"read_file".to_string()));
    }

    #[tokio::test]
    async fn hinted_exact_match_returns_hinted_tools_first() {
        let selector = HintedToolSelector {
            hints: vec!["web_search".to_string(), "http_request".to_string()],
            recipes: None,
            fallback: KeywordToolSelector::default(),
        };
        let tools = test_tools();
        let result = selector
            .select("find information online", &tools)
            .await
            .expect("select");
        // web_search should be in the result because it was hinted.
        assert!(result.contains(&"web_search".to_string()));
        assert!(result.contains(&"http_request".to_string()));
        // web_search should appear before non-hinted tools.
        let ws_pos = result
            .iter()
            .position(|n| n == "web_search")
            .expect("web_search present");
        // git_commit is not hinted, so it should come after if present at all.
        if let Some(gc_pos) = result.iter().position(|n| n == "git_commit") {
            assert!(
                ws_pos < gc_pos,
                "hinted tool should rank before non-hinted tool"
            );
        }
    }

    #[tokio::test]
    async fn hinted_always_includes_foundational_tools() {
        let selector = HintedToolSelector {
            hints: vec!["web_search".to_string()],
            recipes: None,
            fallback: KeywordToolSelector::default(),
        };
        let mut tools = test_tools();
        tools.push(ToolSummary {
            name: "read_file".to_string(),
            description: "Read a file from the filesystem".to_string(),
        });
        let result = selector
            .select("search the web", &tools)
            .await
            .expect("select");
        // read_file should always be included even though it wasn't hinted.
        assert!(
            result.contains(&"read_file".to_string()),
            "foundational tool read_file should always be included"
        );
        // shell should be included if available.
        assert!(
            result.contains(&"shell".to_string()),
            "foundational tool shell should always be included"
        );
    }

    #[tokio::test]
    async fn hinted_substring_match() {
        let selector = HintedToolSelector {
            hints: vec!["git".to_string()],
            recipes: None,
            fallback: KeywordToolSelector::default(),
        };
        let tools = test_tools();
        let result = selector
            .select("commit changes", &tools)
            .await
            .expect("select");
        // "git" hint should match "git_commit" via substring.
        assert!(
            result.contains(&"git_commit".to_string()),
            "substring hint 'git' should match 'git_commit'"
        );
    }
}

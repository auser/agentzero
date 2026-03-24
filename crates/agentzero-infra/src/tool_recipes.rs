//! Tool catalog learning — record successful tool combos and boost them
//! on matching future goals.
//!
//! After a successful agent or swarm run, the system records which tools
//! were used for what kind of goal. On future goals, the [`RecipeStore`]
//! matches against stored recipes via TF-IDF and returns tool suggestions
//! that get boosted by the [`HintedToolSelector`].
//!
//! Persistence: encrypted JSON at `.agentzero/tool-recipes.json`.

use agentzero_storage::EncryptedJsonStore;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// ── Types ────────────────────────────────────────────────────────────────────

/// A recorded tool usage pattern.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRecipe {
    /// Unique identifier.
    pub id: String,
    /// Summary of the goal that was accomplished.
    pub goal_summary: String,
    /// Pre-tokenized keywords for TF-IDF matching.
    pub goal_keywords: Vec<String>,
    /// Tool names that were used.
    pub tools_used: Vec<String>,
    /// Whether the run succeeded.
    pub success: bool,
    /// Unix timestamp.
    pub timestamp: u64,
    /// How many times this recipe has been reused.
    pub use_count: u32,
}

/// Wrapper for encrypted persistence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RecipeData {
    recipes: Vec<ToolRecipe>,
}

// ── RecipeStore ──────────────────────────────────────────────────────────────

const RECIPES_FILE: &str = "tool-recipes.json";
const MAX_RECIPES: usize = 200;

/// Persistent store for tool usage recipes.
pub struct RecipeStore {
    recipes: Vec<ToolRecipe>,
    store: EncryptedJsonStore,
    next_id: u64,
}

impl RecipeStore {
    /// Open or create the recipe store in the given data directory.
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        let store = EncryptedJsonStore::in_config_dir(data_dir, RECIPES_FILE)?;
        let data: RecipeData = store.load_or_default()?;
        let next_id = data
            .recipes
            .iter()
            .filter_map(|r| {
                r.id.strip_prefix("recipe-")
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .max()
            .unwrap_or(0)
            + 1;
        Ok(Self {
            recipes: data.recipes,
            store,
            next_id,
        })
    }

    /// Record a new recipe after a successful (or failed) run.
    pub fn record(
        &mut self,
        goal: &str,
        tools_used: &[String],
        success: bool,
    ) -> anyhow::Result<()> {
        // Don't record empty tool sets.
        if tools_used.is_empty() {
            return Ok(());
        }

        let id = format!("recipe-{}", self.next_id);
        self.next_id += 1;

        let recipe = ToolRecipe {
            id,
            goal_summary: goal.to_string(),
            goal_keywords: tokenize(goal),
            tools_used: tools_used.to_vec(),
            success,
            timestamp: now_secs(),
            use_count: 0,
        };

        self.recipes.push(recipe);

        // Prune old failed recipes to stay under limit.
        if self.recipes.len() > MAX_RECIPES {
            // Remove oldest failed recipes first.
            self.recipes.sort_by(|a, b| {
                let a_priority = if a.success { 1 } else { 0 };
                let b_priority = if b.success { 1 } else { 0 };
                b_priority
                    .cmp(&a_priority)
                    .then(b.use_count.cmp(&a.use_count))
                    .then(b.timestamp.cmp(&a.timestamp))
            });
            self.recipes.truncate(MAX_RECIPES);
        }

        self.persist()
    }

    /// Find recipes matching the given goal, ranked by relevance.
    pub fn find_matching(&self, goal: &str, top_k: usize) -> Vec<&ToolRecipe> {
        let query_tokens = tokenize(goal);
        if query_tokens.is_empty() {
            return vec![];
        }

        let query_set: HashSet<&str> = query_tokens.iter().map(|s| s.as_str()).collect();

        let mut scored: Vec<(usize, f64)> = self
            .recipes
            .iter()
            .enumerate()
            .filter(|(_, r)| r.success) // Only match successful recipes.
            .map(|(i, r)| {
                let doc_set: HashSet<&str> = r.goal_keywords.iter().map(|s| s.as_str()).collect();
                // Jaccard-like overlap: shared terms / total unique terms.
                let intersection = query_set.intersection(&doc_set).count() as f64;
                let union = query_set.union(&doc_set).count() as f64;
                let score = if union > 0.0 {
                    intersection / union
                } else {
                    0.0
                };
                // Boost by use_count (logarithmic).
                let boost = 1.0 + (r.use_count as f64).ln_1p() * 0.1;
                (i, score * boost)
            })
            .filter(|(_, score)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);

        scored.iter().map(|(i, _)| &self.recipes[*i]).collect()
    }

    /// Get tools suggested by matching recipes for a given goal.
    pub fn suggest_tools(&self, goal: &str, top_k: usize) -> Vec<String> {
        let matches = self.find_matching(goal, top_k);
        let mut seen = HashSet::new();
        let mut tools = Vec::new();
        for recipe in matches {
            for tool in &recipe.tools_used {
                if seen.insert(tool.clone()) {
                    tools.push(tool.clone());
                }
            }
        }
        tools
    }

    /// Increment the use_count on a recipe (called when a recipe's tools are reused).
    pub fn mark_reused(&mut self, recipe_id: &str) -> anyhow::Result<()> {
        if let Some(r) = self.recipes.iter_mut().find(|r| r.id == recipe_id) {
            r.use_count += 1;
            self.persist()?;
        }
        Ok(())
    }

    /// List all recipes.
    pub fn list(&self) -> &[ToolRecipe] {
        &self.recipes
    }

    /// Clear all recipes.
    pub fn clear(&mut self) -> anyhow::Result<()> {
        self.recipes.clear();
        self.persist()
    }

    fn persist(&self) -> anyhow::Result<()> {
        let data = RecipeData {
            recipes: self.recipes.clone(),
        };
        self.store.save(&data)
    }
}

// ── Tokenization ─────────────────────────────────────────────────────────────

fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 1)
        .map(|w| w.to_lowercase())
        .collect()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("agentzero-recipes-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn record_and_find_matching() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store
            .record(
                "summarize this video",
                &[
                    "shell".to_string(),
                    "web_fetch".to_string(),
                    "whisper_transcribe".to_string(),
                ],
                true,
            )
            .expect("record");

        let matches = store.find_matching("transcribe this podcast", 5);
        assert!(!matches.is_empty(), "should match on 'transcribe'");
        assert!(matches[0]
            .tools_used
            .contains(&"whisper_transcribe".to_string()));
    }

    #[test]
    fn suggest_tools_returns_unique_tools() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store
            .record(
                "download and process video",
                &["shell".to_string(), "web_fetch".to_string()],
                true,
            )
            .expect("record 1");

        store
            .record(
                "download and convert audio",
                &["shell".to_string(), "http_request".to_string()],
                true,
            )
            .expect("record 2");

        let tools = store.suggest_tools("download a file", 5);
        // shell should appear only once even though it's in both recipes.
        let shell_count = tools.iter().filter(|t| t.as_str() == "shell").count();
        assert_eq!(shell_count, 1, "shell should appear only once");
        assert!(tools.contains(&"web_fetch".to_string()));
    }

    #[test]
    fn failed_recipes_not_matched() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store
            .record("failed task", &["bad_tool".to_string()], false)
            .expect("record failure");

        let matches = store.find_matching("failed task", 5);
        assert!(matches.is_empty(), "failed recipes should not match");
    }

    #[test]
    fn persists_across_reopen() {
        let dir = test_data_dir();

        {
            let mut store = RecipeStore::open(&dir).expect("open");
            store
                .record("test task", &["shell".to_string()], true)
                .expect("record");
        }

        {
            let store = RecipeStore::open(&dir).expect("reopen");
            assert_eq!(store.list().len(), 1);
            assert_eq!(store.list()[0].goal_summary, "test task");
        }
    }

    #[test]
    fn mark_reused_increments_count() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store
            .record("reusable task", &["shell".to_string()], true)
            .expect("record");

        let id = store.list()[0].id.clone();
        store.mark_reused(&id).expect("mark");
        assert_eq!(store.list()[0].use_count, 1);
    }

    #[test]
    fn clear_removes_all() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store
            .record("task", &["shell".to_string()], true)
            .expect("record");
        assert_eq!(store.list().len(), 1);

        store.clear().expect("clear");
        assert!(store.list().is_empty());
    }

    #[test]
    fn empty_tools_not_recorded() {
        let dir = test_data_dir();
        let mut store = RecipeStore::open(&dir).expect("open");

        store.record("empty", &[], true).expect("record");
        assert!(
            store.list().is_empty(),
            "empty tool sets should not be recorded"
        );
    }

    #[test]
    fn tokenize_splits_correctly() {
        let tokens = tokenize("summarize this video file");
        assert_eq!(tokens, vec!["summarize", "this", "video", "file"]);
    }
}

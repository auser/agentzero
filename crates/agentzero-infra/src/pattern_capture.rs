//! AUTO-LEARN — capture novel multi-tool combinations as reusable Composite dynamic tools.
//!
//! After a successful agent run, [`PatternCapture`] checks whether the tool
//! combination was novel (not already covered by an existing recipe with Jaccard
//! similarity above 0.8). If novel and using 3+ unique tools, it creates a Composite
//! `DynamicToolDef` that chains those tools in execution order, making the
//! pattern immediately reusable in future runs.

use crate::tool_recipes::RecipeStore;
use crate::tools::dynamic_tool::{
    CompositeStep, DynamicToolDef, DynamicToolRegistry, DynamicToolStrategy,
};
use agentzero_core::ToolExecutionRecord;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

/// Minimum unique successful tools required for pattern capture.
const MIN_UNIQUE_TOOLS: usize = 3;
/// Jaccard similarity threshold — if an existing recipe exceeds this, the combo is not novel.
const NOVELTY_JACCARD_THRESHOLD: f64 = 0.8;

/// Captures novel tool usage patterns as reusable Composite dynamic tools.
pub struct PatternCapture {
    registry: Arc<DynamicToolRegistry>,
    recipe_store: Arc<Mutex<RecipeStore>>,
}

impl PatternCapture {
    pub fn new(registry: Arc<DynamicToolRegistry>, recipe_store: Arc<Mutex<RecipeStore>>) -> Self {
        Self {
            registry,
            recipe_store,
        }
    }

    /// Analyze a completed run and capture the tool combination if novel.
    /// Returns the name of the created composite tool, or `None` if not novel.
    pub async fn capture_if_novel(
        &self,
        goal: &str,
        tool_executions: &[ToolExecutionRecord],
    ) -> anyhow::Result<Option<String>> {
        // Extract unique successful tool names in execution order.
        let mut seen = HashSet::new();
        let mut ordered_tools: Vec<String> = Vec::new();
        for record in tool_executions {
            if record.success && seen.insert(record.tool_name.clone()) {
                ordered_tools.push(record.tool_name.clone());
            }
        }

        if ordered_tools.len() < MIN_UNIQUE_TOOLS {
            return Ok(None);
        }

        // Check novelty: compare against existing recipes.
        let tool_set: HashSet<&str> = ordered_tools.iter().map(|s| s.as_str()).collect();
        let is_novel = {
            let store = self
                .recipe_store
                .lock()
                .map_err(|e| anyhow::anyhow!("recipe store lock poisoned: {e}"))?;
            let matches = store.find_matching(goal, 5);
            !matches.iter().any(|recipe| {
                let recipe_set: HashSet<&str> =
                    recipe.tools_used.iter().map(|s| s.as_str()).collect();
                jaccard(&tool_set, &recipe_set) >= NOVELTY_JACCARD_THRESHOLD
            })
        };

        if !is_novel {
            return Ok(None);
        }

        // Create composite tool from the execution order.
        let first_keyword = goal
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| w.len() > 2)
            .unwrap_or("task")
            .to_lowercase();
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let tool_name = format!("auto_{}_{}", first_keyword, ts % 100000);

        let steps: Vec<CompositeStep> = ordered_tools
            .iter()
            .map(|name| CompositeStep {
                tool_name: name.clone(),
                input_override: None,
            })
            .collect();

        let description = format!("Auto-captured pipeline: {}", ordered_tools.join(" → "));

        let def = DynamicToolDef {
            name: tool_name.clone(),
            description,
            strategy: DynamicToolStrategy::Composite { steps },
            input_schema: None,
            created_at: ts,
            total_invocations: 0,
            total_successes: 0,
            total_failures: 0,
            last_error: None,
            generation: 0,
            parent_name: None,
            user_rated: false,
            creator_capability_set: None,
        };

        self.registry.register(def).await?;
        info!(tool = %tool_name, tools = ?ordered_tools, "auto-captured novel tool pattern");

        // Also record as a recipe.
        if let Ok(mut store) = self.recipe_store.lock() {
            if let Err(e) = store.record(goal, &ordered_tools, true) {
                warn!(error = %e, "failed to record auto-captured recipe");
            }
        }

        Ok(Some(tool_name))
    }
}

/// Jaccard similarity between two string sets.
fn jaccard(a: &HashSet<&str>, b: &HashSet<&str>) -> f64 {
    let intersection = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jaccard_identical_sets() {
        let a: HashSet<&str> = ["shell", "read_file", "web_fetch"].into_iter().collect();
        assert!((jaccard(&a, &a) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_disjoint_sets() {
        let a: HashSet<&str> = ["shell", "read_file"].into_iter().collect();
        let b: HashSet<&str> = ["web_fetch", "image_gen"].into_iter().collect();
        assert!((jaccard(&a, &b)).abs() < f64::EPSILON);
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: HashSet<&str> = ["shell", "read_file", "web_fetch"].into_iter().collect();
        let b: HashSet<&str> = ["shell", "read_file", "image_gen"].into_iter().collect();
        // intersection=2, union=4, jaccard=0.5
        assert!((jaccard(&a, &b) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn too_few_tools_not_captured() {
        let records = [
            ToolExecutionRecord {
                tool_name: "shell".to_string(),
                success: true,
                error: None,
                latency_ms: 100,
                timestamp: 0,
            },
            ToolExecutionRecord {
                tool_name: "read_file".to_string(),
                success: true,
                error: None,
                latency_ms: 50,
                timestamp: 0,
            },
        ];
        // Only 2 unique tools — below MIN_UNIQUE_TOOLS threshold.
        let mut seen = HashSet::new();
        let ordered: Vec<_> = records
            .iter()
            .filter(|r| r.success && seen.insert(r.tool_name.clone()))
            .map(|r| r.tool_name.clone())
            .collect();
        assert!(ordered.len() < MIN_UNIQUE_TOOLS);
    }
}

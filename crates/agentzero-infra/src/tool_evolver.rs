//! Tool evolution engine — AUTO-FIX for failing tools, AUTO-IMPROVE for successful ones.
//!
//! The [`ToolEvolver`] scans dynamic tools after each agent run and:
//! - **AUTO-FIX**: Repairs tools with >60% failure rate via LLM-based strategy correction
//! - **AUTO-IMPROVE**: Optimizes tools with >80% success rate via LLM-based strategy enhancement
//!
//! Anti-loop protections: one evolution per tool per session, generation caps,
//! cooldown periods, and per-session evolution limits.

use crate::tools::dynamic_tool::{DynamicToolDef, DynamicToolRegistry, DynamicToolStrategy};
use agentzero_core::Provider;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::warn;

/// Failure rate threshold above which auto-fix is triggered.
const FIX_FAILURE_RATE_THRESHOLD: f64 = 0.60;
/// Minimum invocations before auto-fix eligibility.
const FIX_MIN_INVOCATIONS: u32 = 5;
/// Maximum generation for auto-fix (prevent infinite repair loops).
const FIX_MAX_GENERATION: u32 = 5;
/// Success rate threshold above which auto-improve is triggered.
const IMPROVE_SUCCESS_RATE_THRESHOLD: f64 = 0.80;
/// Minimum invocations before auto-improve eligibility.
const IMPROVE_MIN_INVOCATIONS: u32 = 10;
/// Maximum generation for auto-improve.
const IMPROVE_MAX_GENERATION: u32 = 3;
/// Maximum total evolutions per session.
const MAX_EVOLUTIONS_PER_SESSION: usize = 5;

/// Placeholder — full implementation in Phase B.
pub struct ToolEvolver {
    provider: Arc<dyn Provider>,
    registry: Arc<DynamicToolRegistry>,
    session_evolutions: Mutex<HashSet<String>>,
}

impl ToolEvolver {
    pub fn new(provider: Arc<dyn Provider>, registry: Arc<DynamicToolRegistry>) -> Self {
        Self {
            provider,
            registry,
            session_evolutions: Mutex::new(HashSet::new()),
        }
    }

    /// Check if a failing tool qualifies for auto-fix and attempt repair.
    pub async fn maybe_fix(&self, tool_name: &str) -> anyhow::Result<bool> {
        let def = match self.registry.get_def(tool_name).await {
            Some(d) => d,
            None => return Ok(false),
        };

        if def.user_rated || def.total_invocations < FIX_MIN_INVOCATIONS {
            return Ok(false);
        }
        let failure_rate = 1.0 - def.success_rate();
        if failure_rate < FIX_FAILURE_RATE_THRESHOLD || def.generation >= FIX_MAX_GENERATION {
            return Ok(false);
        }

        let mut evolved = self.session_evolutions.lock().await;
        if evolved.len() >= MAX_EVOLUTIONS_PER_SESSION || evolved.contains(tool_name) {
            return Ok(false);
        }
        evolved.insert(tool_name.to_string());
        drop(evolved);

        let errors = def.last_error.as_deref().unwrap_or("(no error details)");
        match self.fix(&def, errors).await {
            Ok(new_def) => {
                self.registry.register(new_def).await?;
                Ok(true)
            }
            Err(e) => {
                warn!(tool = tool_name, error = %e, "auto-fix failed");
                Ok(false)
            }
        }
    }

    /// Scan for high-quality tools and produce improved variants.
    pub async fn evolve_candidates(&self) -> anyhow::Result<Vec<String>> {
        let defs = self.registry.list().await;
        let mut improved = Vec::new();

        for def in &defs {
            if def.total_invocations < IMPROVE_MIN_INVOCATIONS
                || def.success_rate() < IMPROVE_SUCCESS_RATE_THRESHOLD
                || def.generation >= IMPROVE_MAX_GENERATION
            {
                continue;
            }

            // 24h cooldown: skip if any child was created recently.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let has_recent_child = defs
                .iter()
                .any(|d| d.parent_name.as_deref() == Some(&def.name) && now - d.created_at < 86400);
            if has_recent_child {
                continue;
            }

            let mut evolved = self.session_evolutions.lock().await;
            if evolved.len() >= MAX_EVOLUTIONS_PER_SESSION || evolved.contains(&def.name) {
                continue;
            }
            evolved.insert(def.name.clone());
            drop(evolved);

            match self.improve(def).await {
                Ok(new_def) => {
                    let name = new_def.name.clone();
                    self.registry.register(new_def).await?;
                    improved.push(name);
                }
                Err(e) => {
                    warn!(tool = %def.name, error = %e, "auto-improve failed");
                }
            }
        }

        Ok(improved)
    }

    async fn fix(&self, def: &DynamicToolDef, errors: &str) -> anyhow::Result<DynamicToolDef> {
        let strategy_json = serde_json::to_string_pretty(&def.strategy)
            .map_err(|e| anyhow::anyhow!("failed to serialize strategy: {e}"))?;

        let prompt = TOOL_FIX_PROMPT
            .replace("{{name}}", &def.name)
            .replace("{{description}}", &def.description)
            .replace("{{strategy_json}}", &strategy_json)
            .replace("{{errors}}", errors);

        let result = self.provider.complete(&prompt).await?;
        let new_strategy = parse_strategy_from_response(result.output_text.trim())?;

        Ok(DynamicToolDef {
            name: def.name.clone(),
            description: def.description.clone(),
            strategy: new_strategy,
            input_schema: def.input_schema.clone(),
            created_at: def.created_at,
            total_invocations: 0,
            total_successes: 0,
            total_failures: 0,
            last_error: None,
            generation: def.generation + 1,
            parent_name: Some(def.name.clone()),
            user_rated: false,
        })
    }

    async fn improve(&self, def: &DynamicToolDef) -> anyhow::Result<DynamicToolDef> {
        let strategy_json = serde_json::to_string_pretty(&def.strategy)
            .map_err(|e| anyhow::anyhow!("failed to serialize strategy: {e}"))?;

        let prompt = TOOL_IMPROVE_PROMPT
            .replace("{{name}}", &def.name)
            .replace("{{description}}", &def.description)
            .replace("{{strategy_json}}", &strategy_json)
            .replace(
                "{{success_rate}}",
                &format!("{:.0}%", def.success_rate() * 100.0),
            )
            .replace("{{invocations}}", &def.total_invocations.to_string());

        let result = self.provider.complete(&prompt).await?;
        let new_strategy = parse_strategy_from_response(result.output_text.trim())?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(DynamicToolDef {
            name: format!("{}_v{}", def.name, def.generation + 1),
            description: def.description.clone(),
            strategy: new_strategy,
            input_schema: def.input_schema.clone(),
            created_at: now,
            total_invocations: 0,
            total_successes: 0,
            total_failures: 0,
            last_error: None,
            generation: def.generation + 1,
            parent_name: Some(def.name.clone()),
            user_rated: false,
        })
    }
}

const TOOL_FIX_PROMPT: &str = r#"You are a tool repair assistant. A dynamic tool has been failing repeatedly. Given the tool definition and recent error messages, produce a corrected strategy JSON.

Current tool:
- Name: {{name}}
- Description: {{description}}
- Strategy: {{strategy_json}}

Recent errors:
{{errors}}

Output a corrected strategy JSON object (same format as above). Fix the issue based on the errors.
Rules:
- Keep the same strategy type unless errors clearly indicate a different type
- Output ONLY the JSON strategy object, no markdown fences or explanation"#;

const TOOL_IMPROVE_PROMPT: &str = r#"You are a tool optimization assistant. A dynamic tool has been performing well. Analyze it and produce an improved strategy.

Current tool:
- Name: {{name}}
- Description: {{description}}
- Strategy: {{strategy_json}}
- Success rate: {{success_rate}}
- Total invocations: {{invocations}}

Produce an optimized strategy JSON that improves performance, adds error handling, or specializes the tool.
Rules:
- Keep the same strategy type
- Output ONLY the JSON strategy object, no markdown fences or explanation"#;

fn parse_strategy_from_response(response: &str) -> anyhow::Result<DynamicToolStrategy> {
    let trimmed = response.trim();

    // Try ```json ... ``` block.
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    // Try ``` ... ``` block.
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            if let Ok(v) = serde_json::from_str(after[..end].trim()) {
                return Ok(v);
            }
        }
    }

    // Try raw JSON.
    if let Ok(v) = serde_json::from_str(trimmed) {
        return Ok(v);
    }

    // Try extracting first { ... } block.
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if let Ok(v) = serde_json::from_str(&trimmed[start..=end]) {
                return Ok(v);
            }
        }
    }

    anyhow::bail!("failed to parse strategy from LLM response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_strategy_raw_json() {
        let json = r#"{"type": "shell", "command_template": "echo hello"}"#;
        let result = parse_strategy_from_response(json).expect("parse");
        assert!(matches!(result, DynamicToolStrategy::Shell { .. }));
    }

    #[test]
    fn parse_strategy_fenced() {
        let json = "```json\n{\"type\": \"shell\", \"command_template\": \"ls -la\"}\n```";
        let result = parse_strategy_from_response(json).expect("parse");
        assert!(matches!(result, DynamicToolStrategy::Shell { .. }));
    }

    #[test]
    fn parse_strategy_embedded() {
        let json = "Here is the fix:\n{\"type\": \"http\", \"url\": \"https://api.example.com\", \"method\": \"GET\", \"headers\": {}}\nDone.";
        let result = parse_strategy_from_response(json).expect("parse");
        assert!(matches!(result, DynamicToolStrategy::Http { .. }));
    }

    #[test]
    fn fix_eligibility() {
        let def = DynamicToolDef {
            name: "test".to_string(),
            description: "test".to_string(),
            strategy: DynamicToolStrategy::Shell {
                command_template: "echo x".to_string(),
            },
            input_schema: None,
            created_at: 0,
            total_invocations: 10,
            total_successes: 2,
            total_failures: 8,
            last_error: Some("not found".to_string()),
            generation: 0,
            parent_name: None,
            user_rated: false,
        };
        assert!(def.total_invocations >= FIX_MIN_INVOCATIONS);
        assert!((1.0 - def.success_rate()) >= FIX_FAILURE_RATE_THRESHOLD);
        assert!(def.generation < FIX_MAX_GENERATION);
    }

    #[test]
    fn improve_eligibility() {
        let def = DynamicToolDef {
            name: "test".to_string(),
            description: "test".to_string(),
            strategy: DynamicToolStrategy::Shell {
                command_template: "echo x".to_string(),
            },
            input_schema: None,
            created_at: 0,
            total_invocations: 15,
            total_successes: 14,
            total_failures: 1,
            last_error: None,
            generation: 0,
            parent_name: None,
            user_rated: false,
        };
        assert!(def.total_invocations >= IMPROVE_MIN_INVOCATIONS);
        assert!(def.success_rate() >= IMPROVE_SUCCESS_RATE_THRESHOLD);
        assert!(def.generation < IMPROVE_MAX_GENERATION);
    }
}

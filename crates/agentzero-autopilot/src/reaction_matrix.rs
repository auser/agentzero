use chrono::Utc;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::types::{AutopilotEvent, ReactionRule};

/// JSON-configurable probabilistic inter-agent reaction system.
///
/// When agent A emits an event matching pattern X, agent B has probability P
/// of proposing action Y. Adds non-deterministic dynamics mimicking team
/// interactions.
#[derive(Debug)]
pub struct ReactionMatrix {
    rules: Vec<ReactionRule>,
    /// Tracks last-fired times per (source_agent, event_pattern, target_agent) key.
    cooldowns: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

impl ReactionMatrix {
    pub fn new(rules: Vec<ReactionRule>) -> Self {
        info!(count = rules.len(), "loaded reaction matrix rules");
        Self {
            rules,
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load reaction matrix from a JSON file.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let rules: Vec<ReactionRule> = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("invalid reaction matrix JSON: {e}"))?;
        Ok(Self::new(rules))
    }

    /// Load from a file path.
    pub async fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| anyhow::anyhow!("failed to read reaction matrix file '{path}': {e}"))?;
        Self::from_json(&content)
    }

    /// Evaluate an event and return actions to fire based on probability and cooldown.
    pub async fn evaluate(&self, event: &AutopilotEvent) -> Vec<ReactionAction> {
        let cooldowns = self.cooldowns.read().await;
        let mut actions = Vec::new();
        let mut rng = rand::thread_rng();

        for rule in &self.rules {
            // Check source agent match
            if rule.source_agent != "*" && rule.source_agent != event.source_agent {
                continue;
            }

            // Check event pattern match (simple string match or wildcard)
            if !event_matches_pattern(&event.event_type, &rule.event_pattern) {
                continue;
            }

            // Check cooldown
            let cooldown_key = format!(
                "{}:{}:{}",
                rule.source_agent, rule.event_pattern, rule.target_agent
            );
            let is_cooled = cooldowns
                .get(&cooldown_key)
                .map(|last| {
                    let elapsed = Utc::now() - *last;
                    elapsed.num_seconds() >= rule.cooldown_secs as i64
                })
                .unwrap_or(true);

            if !is_cooled {
                debug!(
                    source = %rule.source_agent,
                    target = %rule.target_agent,
                    pattern = %rule.event_pattern,
                    "reaction skipped (cooldown)"
                );
                continue;
            }

            // Roll probability
            let roll: f64 = rng.gen();
            if roll > rule.probability {
                debug!(
                    source = %rule.source_agent,
                    target = %rule.target_agent,
                    probability = rule.probability,
                    roll,
                    "reaction skipped (probability)"
                );
                continue;
            }

            actions.push(ReactionAction {
                cooldown_key,
                target_agent: rule.target_agent.clone(),
                action: rule.action.clone(),
                source_event: event.event_type.clone(),
            });
        }

        actions
    }

    /// Mark a reaction as fired, updating its cooldown timer.
    pub async fn mark_fired(&self, cooldown_key: &str) {
        let mut cooldowns = self.cooldowns.write().await;
        cooldowns.insert(cooldown_key.to_string(), Utc::now());
    }

    /// Return the number of loaded rules.
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// An action that should be taken as a result of a reaction.
#[derive(Debug, Clone)]
pub struct ReactionAction {
    pub cooldown_key: String,
    pub target_agent: String,
    pub action: String,
    pub source_event: String,
}

/// Check if an event type matches a pattern.
/// Supports exact match and simple wildcard (`*` matches anything).
fn event_matches_pattern(event_type: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return event_type.starts_with(prefix);
    }
    event_type == pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_rules() -> Vec<ReactionRule> {
        vec![
            ReactionRule {
                source_agent: "writer".to_string(),
                event_pattern: "mission.completed".to_string(),
                target_agent: "social_media".to_string(),
                action: "propose_social_post".to_string(),
                probability: 1.0, // Always fire for deterministic tests
                cooldown_secs: 0,
                last_fired_at: None,
            },
            ReactionRule {
                source_agent: "*".to_string(),
                event_pattern: "content.*".to_string(),
                target_agent: "analyst".to_string(),
                action: "analyze_performance".to_string(),
                probability: 1.0,
                cooldown_secs: 0,
                last_fired_at: None,
            },
        ]
    }

    #[tokio::test]
    async fn exact_match_fires() {
        let matrix = ReactionMatrix::new(make_rules());
        let event = AutopilotEvent::new("mission.completed", "writer", serde_json::json!({}));
        let actions = matrix.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target_agent, "social_media");
    }

    #[tokio::test]
    async fn wildcard_source_matches_any_agent() {
        let matrix = ReactionMatrix::new(make_rules());
        let event = AutopilotEvent::new("content.published", "anyone", serde_json::json!({}));
        let actions = matrix.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].target_agent, "analyst");
    }

    #[tokio::test]
    async fn wildcard_pattern_matches_prefix() {
        let matrix = ReactionMatrix::new(make_rules());
        let event = AutopilotEvent::new("content.drafted", "writer", serde_json::json!({}));
        let actions = matrix.evaluate(&event).await;
        // Should match the "content.*" rule
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action, "analyze_performance");
    }

    #[tokio::test]
    async fn non_matching_event_no_action() {
        let matrix = ReactionMatrix::new(make_rules());
        let event = AutopilotEvent::new("system.shutdown", "ops", serde_json::json!({}));
        let actions = matrix.evaluate(&event).await;
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn cooldown_blocks_rapid_reaction() {
        let rules = vec![ReactionRule {
            source_agent: "writer".to_string(),
            event_pattern: "test.event".to_string(),
            target_agent: "social_media".to_string(),
            action: "post".to_string(),
            probability: 1.0,
            cooldown_secs: 3600,
            last_fired_at: None,
        }];
        let matrix = ReactionMatrix::new(rules);
        let event = AutopilotEvent::new("test.event", "writer", serde_json::json!({}));

        // First should fire
        let actions = matrix.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
        matrix.mark_fired(&actions[0].cooldown_key).await;

        // Second should be blocked by cooldown
        let actions = matrix.evaluate(&event).await;
        assert!(actions.is_empty());
    }

    #[test]
    fn from_json_valid() {
        let json = r#"[{
            "source_agent": "writer",
            "event_pattern": "mission.completed",
            "target_agent": "social_media",
            "action": "post",
            "probability": 0.9,
            "cooldown_secs": 600
        }]"#;
        let matrix = ReactionMatrix::from_json(json).expect("valid json");
        assert_eq!(matrix.rule_count(), 1);
    }

    #[test]
    fn from_json_invalid() {
        let result = ReactionMatrix::from_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn event_pattern_matching() {
        assert!(event_matches_pattern(
            "mission.completed",
            "mission.completed"
        ));
        assert!(event_matches_pattern("anything", "*"));
        assert!(event_matches_pattern("content.published", "content.*"));
        assert!(event_matches_pattern("content.drafted", "content.*"));
        assert!(!event_matches_pattern("mission.completed", "content.*"));
        assert!(!event_matches_pattern(
            "mission.completed",
            "mission.failed"
        ));
    }
}

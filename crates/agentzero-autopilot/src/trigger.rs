use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::config::TriggerRuleConfig;
use crate::types::{AutopilotEvent, TriggerAction, TriggerCondition, TriggerRule};

/// Evaluates trigger rules against incoming events and fires actions.
#[derive(Debug)]
pub struct TriggerEngine {
    rules: Arc<RwLock<Vec<TriggerRule>>>,
    /// Tracks last-fired times per rule ID for cooldown enforcement.
    cooldowns: Arc<RwLock<HashMap<String, chrono::DateTime<Utc>>>>,
}

impl TriggerEngine {
    pub fn new() -> Self {
        Self {
            rules: Arc::new(RwLock::new(Vec::new())),
            cooldowns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load trigger rules from config.
    pub async fn load_from_config(&self, configs: &[TriggerRuleConfig]) {
        let mut rules = self.rules.write().await;
        rules.clear();
        for (i, cfg) in configs.iter().enumerate() {
            rules.push(TriggerRule {
                id: format!("trigger-{i}"),
                name: cfg.name.clone(),
                condition: cfg.condition.clone(),
                action: cfg.action.clone(),
                cooldown_secs: cfg.cooldown_secs,
                last_fired_at: None,
                enabled: cfg.enabled,
            });
        }
        info!(count = rules.len(), "loaded trigger rules");
    }

    /// Evaluate an event against all trigger rules and return actions to fire.
    pub async fn evaluate(&self, event: &AutopilotEvent) -> Vec<(String, TriggerAction)> {
        let rules = self.rules.read().await;
        let cooldowns = self.cooldowns.read().await;
        let mut actions = Vec::new();

        for rule in rules.iter() {
            if !rule.enabled {
                continue;
            }

            // Check condition match
            let matches = match &rule.condition {
                TriggerCondition::EventMatch { event_type } => event.event_type == *event_type,
                TriggerCondition::Cron { .. } => {
                    // Cron triggers are handled by the cron scheduler, not event evaluation.
                    false
                }
                TriggerCondition::MetricThreshold { metric, threshold } => {
                    // Check if the event payload contains the metric at or above threshold.
                    event
                        .payload
                        .get(metric)
                        .and_then(|v| v.as_f64())
                        .map(|v| v >= *threshold)
                        .unwrap_or(false)
                }
            };

            if !matches {
                continue;
            }

            // Check cooldown
            let is_cooled = cooldowns
                .get(&rule.id)
                .map(|last| {
                    let elapsed = Utc::now() - *last;
                    elapsed.num_seconds() >= rule.cooldown_secs as i64
                })
                .unwrap_or(true);

            if !is_cooled {
                debug!(
                    rule_id = %rule.id,
                    rule_name = %rule.name,
                    "trigger skipped (cooldown)"
                );
                continue;
            }

            actions.push((rule.id.clone(), rule.action.clone()));
        }

        actions
    }

    /// Mark a trigger as fired, updating its cooldown timer.
    pub async fn mark_fired(&self, rule_id: &str) {
        let mut cooldowns = self.cooldowns.write().await;
        cooldowns.insert(rule_id.to_string(), Utc::now());
    }

    /// List all trigger rules.
    pub async fn list_rules(&self) -> Vec<TriggerRule> {
        self.rules.read().await.clone()
    }

    /// Enable or disable a trigger rule by ID.
    pub async fn toggle_rule(&self, rule_id: &str, enabled: bool) -> bool {
        let mut rules = self.rules.write().await;
        if let Some(rule) = rules.iter_mut().find(|r| r.id == rule_id) {
            rule.enabled = enabled;
            info!(rule_id, enabled, "trigger rule toggled");
            true
        } else {
            warn!(rule_id, "trigger rule not found");
            false
        }
    }
}

impl Default for TriggerEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TriggerAction;

    fn make_event(event_type: &str) -> AutopilotEvent {
        AutopilotEvent::new(event_type, "test-agent", serde_json::json!({}))
    }

    #[tokio::test]
    async fn event_match_trigger_fires() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "on-mission-complete".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "mission.completed".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "write follow-up".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        let event = make_event("mission.completed");
        let actions = engine.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
    }

    #[tokio::test]
    async fn non_matching_event_does_not_fire() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "on-mission-complete".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "mission.completed".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "write follow-up".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        let event = make_event("proposal.created");
        let actions = engine.evaluate(&event).await;
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn cooldown_prevents_rapid_firing() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "test".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "test.event".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "do thing".to_string(),
            },
            cooldown_secs: 3600,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        let event = make_event("test.event");

        // First evaluation should fire
        let actions = engine.evaluate(&event).await;
        assert_eq!(actions.len(), 1);

        // Mark as fired
        engine.mark_fired(&actions[0].0).await;

        // Second evaluation should NOT fire (cooldown)
        let actions = engine.evaluate(&event).await;
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn disabled_rule_does_not_fire() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "disabled".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "test.event".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "do thing".to_string(),
            },
            cooldown_secs: 0,
            enabled: false,
        }];
        engine.load_from_config(&configs).await;

        let event = make_event("test.event");
        let actions = engine.evaluate(&event).await;
        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn toggle_rule() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "togglable".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "test.event".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "do thing".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        // Disable
        let toggled = engine.toggle_rule("trigger-0", false).await;
        assert!(toggled);

        let event = make_event("test.event");
        let actions = engine.evaluate(&event).await;
        assert!(actions.is_empty());

        // Re-enable
        engine.toggle_rule("trigger-0", true).await;
        let actions = engine.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
    }

    #[tokio::test]
    async fn metric_threshold_trigger() {
        let engine = TriggerEngine::new();
        let configs = vec![TriggerRuleConfig {
            name: "high-engagement".to_string(),
            condition: TriggerCondition::MetricThreshold {
                metric: "engagement_score".to_string(),
                threshold: 0.8,
            },
            action: TriggerAction::ProposeTask {
                agent: "social_media".to_string(),
                prompt: "amplify content".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        // Below threshold
        let event = AutopilotEvent::new(
            "content.metrics",
            "analyst",
            serde_json::json!({"engagement_score": 0.5}),
        );
        let actions = engine.evaluate(&event).await;
        assert!(actions.is_empty());

        // Above threshold
        let event = AutopilotEvent::new(
            "content.metrics",
            "analyst",
            serde_json::json!({"engagement_score": 0.9}),
        );
        let actions = engine.evaluate(&event).await;
        assert_eq!(actions.len(), 1);
    }
}

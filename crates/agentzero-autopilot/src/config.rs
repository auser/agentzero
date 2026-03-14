use serde::{Deserialize, Serialize};

use crate::types::{TriggerAction, TriggerCondition};

/// Configuration for trigger rules defined in TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRuleConfig {
    pub name: String,
    pub condition: TriggerCondition,
    pub action: TriggerAction,
    #[serde(default = "default_cooldown")]
    pub cooldown_secs: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_cooldown() -> u64 {
    3600
}

fn default_enabled() -> bool {
    true
}

/// Top-level autopilot configuration, deserialized from `[autopilot]` in TOML.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AutopilotConfig {
    pub enabled: bool,

    /// Supabase project URL (e.g. `https://xxx.supabase.co`).
    pub supabase_url: String,

    /// Supabase service-role key for full access.
    #[serde(default)]
    pub supabase_service_role_key: String,

    /// Maximum daily spend in cents before cap gate rejects proposals.
    #[serde(default = "default_max_daily_spend")]
    pub max_daily_spend_cents: u64,

    /// Maximum number of concurrently running missions.
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_missions: usize,

    /// Maximum proposals any single agent can create per hour.
    #[serde(default = "default_max_proposals_per_hour")]
    pub max_proposals_per_hour: usize,

    /// Maximum missions any single agent can have per day.
    #[serde(default = "default_max_missions_per_agent")]
    pub max_missions_per_agent_per_day: usize,

    /// Minutes before a mission without a heartbeat is marked stale.
    #[serde(default = "default_stale_threshold")]
    pub stale_threshold_minutes: u32,

    /// Path to a JSON file containing reaction matrix rules.
    pub reaction_matrix_path: Option<String>,

    /// Trigger rules defined inline in TOML.
    #[serde(default)]
    pub triggers: Vec<TriggerRuleConfig>,
}

fn default_max_daily_spend() -> u64 {
    500
}

fn default_max_concurrent() -> usize {
    5
}

fn default_max_proposals_per_hour() -> usize {
    20
}

fn default_max_missions_per_agent() -> usize {
    10
}

fn default_stale_threshold() -> u32 {
    30
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_disabled() {
        let cfg = AutopilotConfig::default();
        assert!(!cfg.enabled);
    }

    #[test]
    fn serde_defaults_are_applied() {
        let cfg: AutopilotConfig = serde_json::from_str("{}").expect("empty obj");
        assert!(!cfg.enabled);
        assert_eq!(cfg.max_daily_spend_cents, 500);
        assert_eq!(cfg.max_concurrent_missions, 5);
        assert_eq!(cfg.stale_threshold_minutes, 30);
    }

    #[test]
    fn config_serde_roundtrip() {
        let cfg = AutopilotConfig {
            enabled: true,
            supabase_url: "https://test.supabase.co".to_string(),
            supabase_service_role_key: "key123".to_string(),
            max_daily_spend_cents: 1000,
            max_concurrent_missions: 10,
            max_proposals_per_hour: 50,
            max_missions_per_agent_per_day: 20,
            stale_threshold_minutes: 15,
            reaction_matrix_path: Some("reactions.json".to_string()),
            triggers: vec![],
        };
        let json = serde_json::to_string(&cfg).expect("serialize");
        let cfg2: AutopilotConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(cfg2.enabled);
        assert_eq!(cfg2.supabase_url, "https://test.supabase.co");
        assert_eq!(cfg2.max_daily_spend_cents, 1000);
    }
}

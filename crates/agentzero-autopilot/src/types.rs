use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Proposal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalType {
    ContentIdea,
    TaskRequest,
    ResourceRequest,
    SystemChange,
}

impl fmt::Display for ProposalType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContentIdea => write!(f, "content_idea"),
            Self::TaskRequest => write!(f, "task_request"),
            Self::ResourceRequest => write!(f, "resource_request"),
            Self::SystemChange => write!(f, "system_change"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Pending,
    Approved,
    Rejected,
    Executed,
}

impl fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Approved => write!(f, "approved"),
            Self::Rejected => write!(f, "rejected"),
            Self::Executed => write!(f, "executed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub agent_id: String,
    pub title: String,
    pub description: String,
    pub proposal_type: ProposalType,
    pub priority: Priority,
    pub estimated_cost_microdollars: u64,
    pub status: ProposalStatus,
    pub cap_gate_result: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Proposal {
    pub fn new(
        agent_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
        proposal_type: ProposalType,
        priority: Priority,
        estimated_cost_microdollars: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent_id: agent_id.into(),
            title: title.into(),
            description: description.into(),
            proposal_type,
            priority,
            estimated_cost_microdollars,
            status: ProposalStatus::Pending,
            cap_gate_result: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: now,
            updated_at: now,
        }
    }
}

// ---------------------------------------------------------------------------
// Mission
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Stalled,
}

impl fmt::Display for MissionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Stalled => write!(f, "stalled"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Skipped,
}

impl fmt::Display for StepStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::InProgress => write!(f, "in_progress"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
            Self::Skipped => write!(f, "skipped"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionStep {
    pub step_index: usize,
    pub description: String,
    pub agent_id: String,
    pub status: StepStatus,
    pub result: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mission {
    pub id: String,
    pub proposal_id: String,
    pub title: String,
    pub steps: Vec<MissionStep>,
    pub status: MissionStatus,
    pub assigned_agent: String,
    pub heartbeat_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub result: Option<serde_json::Value>,
}

impl Mission {
    pub fn from_proposal(proposal: &Proposal, steps: Vec<MissionStep>) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            proposal_id: proposal.id.clone(),
            title: proposal.title.clone(),
            steps,
            status: MissionStatus::Pending,
            assigned_agent: proposal.agent_id.clone(),
            heartbeat_at: now,
            deadline: None,
            created_at: now,
            updated_at: now,
            result: None,
        }
    }

    /// Returns true if the mission has reached a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            MissionStatus::Completed | MissionStatus::Failed
        )
    }

    /// Returns true if the mission heartbeat is older than the given threshold.
    pub fn is_stale(&self, threshold_minutes: i64) -> bool {
        let elapsed = Utc::now() - self.heartbeat_at;
        elapsed.num_minutes() >= threshold_minutes
    }
}

// ---------------------------------------------------------------------------
// AutopilotEvent
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotEvent {
    pub id: String,
    pub event_type: String,
    pub source_agent: String,
    pub payload: serde_json::Value,
    pub correlation_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl AutopilotEvent {
    pub fn new(
        event_type: impl Into<String>,
        source_agent: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            event_type: event_type.into(),
            source_agent: source_agent.into(),
            payload,
            correlation_id: None,
            created_at: Utc::now(),
        }
    }

    pub fn with_correlation(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Trigger
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerCondition {
    EventMatch { event_type: String },
    Cron { schedule: String },
    MetricThreshold { metric: String, threshold: f64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerAction {
    ProposeTask { agent: String, prompt: String },
    NotifyAgent { agent: String, message: String },
    RunPipeline { pipeline: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerRule {
    pub id: String,
    pub name: String,
    pub condition: TriggerCondition,
    pub action: TriggerAction,
    pub cooldown_secs: u64,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub enabled: bool,
}

impl TriggerRule {
    /// Returns true if enough time has elapsed since the last firing.
    pub fn is_cooled_down(&self) -> bool {
        match self.last_fired_at {
            None => true,
            Some(last) => {
                let elapsed = Utc::now() - last;
                elapsed.num_seconds() >= self.cooldown_secs as i64
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Reaction
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionRule {
    pub source_agent: String,
    pub event_pattern: String,
    pub target_agent: String,
    pub action: String,
    pub probability: f64,
    pub cooldown_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_fired_at: Option<DateTime<Utc>>,
}

impl ReactionRule {
    /// Returns true if enough time has elapsed since the last firing.
    pub fn is_cooled_down(&self) -> bool {
        match self.last_fired_at {
            None => true,
            Some(last) => {
                let elapsed = Utc::now() - last;
                elapsed.num_seconds() >= self.cooldown_secs as i64
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

    #[test]
    fn proposal_serde_roundtrip() {
        let p = Proposal::new(
            "editor",
            "Write blog post",
            "A post about AI agents",
            ProposalType::ContentIdea,
            Priority::High,
            5000,
        );
        let json = serde_json::to_string(&p).expect("serialize");
        let p2: Proposal = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(p.id, p2.id);
        assert_eq!(p.title, p2.title);
        assert_eq!(p.proposal_type, p2.proposal_type);
        assert_eq!(p.priority, p2.priority);
        assert_eq!(p.status, ProposalStatus::Pending);
    }

    #[test]
    fn mission_from_proposal() {
        let p = Proposal::new(
            "writer",
            "Test Mission",
            "desc",
            ProposalType::TaskRequest,
            Priority::Medium,
            1000,
        );
        let steps = vec![MissionStep {
            step_index: 0,
            description: "Step 1".to_string(),
            agent_id: "writer".to_string(),
            status: StepStatus::Pending,
            result: None,
            started_at: None,
            completed_at: None,
        }];
        let m = Mission::from_proposal(&p, steps);
        assert_eq!(m.proposal_id, p.id);
        assert_eq!(m.status, MissionStatus::Pending);
        assert!(!m.is_terminal());
        assert_eq!(m.steps.len(), 1);
    }

    #[test]
    fn mission_stale_detection() {
        let p = Proposal::new(
            "agent",
            "t",
            "d",
            ProposalType::TaskRequest,
            Priority::Low,
            0,
        );
        let mut m = Mission::from_proposal(&p, vec![]);
        // Just created — should not be stale
        assert!(!m.is_stale(30));
        // Set heartbeat far in the past
        m.heartbeat_at = Utc::now() - chrono::Duration::minutes(60);
        assert!(m.is_stale(30));
    }

    #[test]
    fn trigger_cooldown() {
        let mut rule = TriggerRule {
            id: "t1".to_string(),
            name: "test".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "mission.completed".to_string(),
            },
            action: TriggerAction::ProposeTask {
                agent: "editor".to_string(),
                prompt: "do something".to_string(),
            },
            cooldown_secs: 3600,
            last_fired_at: None,
            enabled: true,
        };
        // Never fired — should be cooled down
        assert!(rule.is_cooled_down());
        // Just fired — should NOT be cooled down
        rule.last_fired_at = Some(Utc::now());
        assert!(!rule.is_cooled_down());
        // Fired long ago — should be cooled down
        rule.last_fired_at = Some(Utc::now() - chrono::Duration::hours(2));
        assert!(rule.is_cooled_down());
    }

    #[test]
    fn autopilot_event_with_correlation() {
        let evt = AutopilotEvent::new(
            "proposal.created",
            "editor",
            serde_json::json!({"proposal_id": "p1"}),
        )
        .with_correlation("corr-123");
        assert_eq!(evt.event_type, "proposal.created");
        assert_eq!(evt.correlation_id.as_deref(), Some("corr-123"));
    }

    #[test]
    fn display_impls() {
        assert_eq!(ProposalType::ContentIdea.to_string(), "content_idea");
        assert_eq!(Priority::Critical.to_string(), "critical");
        assert_eq!(ProposalStatus::Approved.to_string(), "approved");
        assert_eq!(MissionStatus::InProgress.to_string(), "in_progress");
        assert_eq!(StepStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn reaction_rule_cooldown() {
        let mut rule = ReactionRule {
            source_agent: "writer".to_string(),
            event_pattern: "mission.completed".to_string(),
            target_agent: "social_media".to_string(),
            action: "propose_social_post".to_string(),
            probability: 0.9,
            cooldown_secs: 600,
            last_fired_at: None,
        };
        assert!(rule.is_cooled_down());
        rule.last_fired_at = Some(Utc::now());
        assert!(!rule.is_cooled_down());
    }

    #[test]
    fn trigger_condition_serde() {
        let cond = TriggerCondition::Cron {
            schedule: "0 */6 * * *".to_string(),
        };
        let json = serde_json::to_string(&cond).expect("serialize");
        let cond2: TriggerCondition = serde_json::from_str(&json).expect("deserialize");
        match cond2 {
            TriggerCondition::Cron { schedule } => {
                assert_eq!(schedule, "0 */6 * * *");
            }
            _ => panic!("expected Cron variant"),
        }
    }
}

//! Main orchestration loop for autonomous agent operation.
//!
//! The [`AutopilotLoop`] polls for approved proposals, converts them to missions,
//! advances mission steps, fires triggers, evaluates reactions, and enforces
//! budget limits via the [`CapGate`].

use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::cap_gate::{CapGate, CapGateResult};
use crate::config::AutopilotConfig;
use crate::reaction_matrix::ReactionMatrix;
use crate::trigger::TriggerEngine;
use crate::types::{AutopilotEvent, Mission, MissionStatus, MissionStep, Proposal, StepStatus};

/// Default tick interval in seconds when not specified by config.
const DEFAULT_TICK_INTERVAL_SECS: u64 = 10;

/// Internal snapshot of resource usage used for offline cap-gate checks.
#[derive(Debug, Clone, Default)]
struct ResourceSnapshot {
    daily_spend_microdollars: u64,
    concurrent_missions: usize,
}

/// The main orchestration loop for autonomous operation.
///
/// It:
/// 1. Polls for approved proposals
/// 2. Creates missions from approved proposals
/// 3. Dispatches mission steps to agents
/// 4. Tracks step completion and updates mission status
/// 5. Feeds results into the [`TriggerEngine`] and [`ReactionMatrix`]
/// 6. Respects [`CapGate`] budget limits
pub struct AutopilotLoop {
    config: AutopilotConfig,
    cap_gate: CapGate,
    trigger_engine: Arc<TriggerEngine>,
    reaction_matrix: Arc<RwLock<Option<ReactionMatrix>>>,
    /// Proposals that have been approved and are awaiting conversion to missions.
    approved_proposals: Arc<RwLock<Vec<Proposal>>>,
    /// Active missions being tracked by the loop.
    active_missions: Arc<RwLock<Vec<Mission>>>,
    /// Current resource usage snapshot for offline cap-gate checks.
    resource_snapshot: Arc<RwLock<ResourceSnapshot>>,
    /// Timestamp of the last successful tick.
    last_heartbeat: Arc<RwLock<DateTime<Utc>>>,
    /// Tick interval in seconds.
    tick_interval_secs: u64,
    /// Total number of completed ticks (useful for testing and metrics).
    tick_count: Arc<RwLock<u64>>,
}

impl AutopilotLoop {
    /// Create a new autopilot loop with the given configuration.
    pub fn new(config: AutopilotConfig) -> Self {
        let cap_gate = CapGate::from_config(&config);
        Self {
            config,
            cap_gate,
            trigger_engine: Arc::new(TriggerEngine::new()),
            reaction_matrix: Arc::new(RwLock::new(None)),
            approved_proposals: Arc::new(RwLock::new(Vec::new())),
            active_missions: Arc::new(RwLock::new(Vec::new())),
            resource_snapshot: Arc::new(RwLock::new(ResourceSnapshot::default())),
            last_heartbeat: Arc::new(RwLock::new(Utc::now())),
            tick_interval_secs: DEFAULT_TICK_INTERVAL_SECS,
            tick_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Override the default tick interval.
    pub fn with_tick_interval(mut self, secs: u64) -> Self {
        self.tick_interval_secs = secs;
        self
    }

    /// Set the reaction matrix for inter-agent reactions.
    pub async fn set_reaction_matrix(&self, matrix: ReactionMatrix) {
        let mut rm = self.reaction_matrix.write().await;
        *rm = Some(matrix);
    }

    /// Submit an approved proposal for the loop to process.
    pub async fn submit_approved_proposal(&self, proposal: Proposal) {
        let mut proposals = self.approved_proposals.write().await;
        proposals.push(proposal);
    }

    /// Update the resource snapshot used for offline cap-gate checks.
    pub async fn update_resource_snapshot(
        &self,
        daily_spend_microdollars: u64,
        concurrent_missions: usize,
    ) {
        let mut snapshot = self.resource_snapshot.write().await;
        snapshot.daily_spend_microdollars = daily_spend_microdollars;
        snapshot.concurrent_missions = concurrent_missions;
    }

    /// Get the trigger engine for external configuration.
    pub fn trigger_engine(&self) -> &Arc<TriggerEngine> {
        &self.trigger_engine
    }

    /// Get the current tick count.
    pub async fn tick_count(&self) -> u64 {
        *self.tick_count.read().await
    }

    /// Get the last heartbeat timestamp.
    pub async fn last_heartbeat(&self) -> DateTime<Utc> {
        *self.last_heartbeat.read().await
    }

    /// Get a snapshot of active missions.
    pub async fn active_missions(&self) -> Vec<Mission> {
        self.active_missions.read().await.clone()
    }

    /// Main loop that runs until the shutdown signal is received.
    ///
    /// Uses `tokio::select!` for clean shutdown: each tick the loop checks
    /// the watch channel and exits gracefully when `true` is observed.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        info!(
            tick_interval_secs = self.tick_interval_secs,
            enabled = self.config.enabled,
            "autopilot loop starting"
        );

        if !self.config.enabled {
            warn!("autopilot is disabled in config — loop will idle until shutdown");
            // Wait for shutdown even when disabled.
            let _ = shutdown.changed().await;
            info!("autopilot loop shut down (was disabled)");
            return;
        }

        // Load trigger rules from config.
        self.trigger_engine
            .load_from_config(&self.config.triggers)
            .await;

        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(self.tick_interval_secs));

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("autopilot loop received shutdown signal");
                        break;
                    }
                }
                _ = interval.tick() => {
                    if let Err(e) = self.tick().await {
                        error!(error = %e, "autopilot tick failed");
                    }
                }
            }
        }

        info!("autopilot loop exited cleanly");
    }

    /// Execute a single tick of the autopilot loop.
    ///
    /// This is also exposed for testing — callers can drive the loop one tick
    /// at a time without running the full async loop.
    pub async fn tick(&self) -> anyhow::Result<()> {
        let tick_num = {
            let mut count = self.tick_count.write().await;
            *count += 1;
            *count
        };

        debug!(tick = tick_num, "autopilot tick");

        // 1. Check cap gate before processing anything.
        let snapshot = self.resource_snapshot.read().await.clone();
        let at_capacity = snapshot.concurrent_missions >= self.cap_gate.max_concurrent_missions;

        if at_capacity {
            debug!(
                concurrent = snapshot.concurrent_missions,
                max = self.cap_gate.max_concurrent_missions,
                "at mission capacity — skipping proposal processing"
            );
        }

        // 2. Process approved proposals (convert to missions) if not at capacity.
        if !at_capacity {
            self.process_proposals().await;
        }

        // 3. Advance active mission steps.
        self.advance_missions().await;

        // 4. Fire triggers for completed missions.
        self.fire_triggers().await;

        // 5. Update heartbeat.
        {
            let mut hb = self.last_heartbeat.write().await;
            *hb = Utc::now();
        }

        Ok(())
    }

    /// Convert approved proposals to missions, respecting cap-gate limits.
    async fn process_proposals(&self) {
        let mut proposals = self.approved_proposals.write().await;
        if proposals.is_empty() {
            return;
        }

        let mut missions = self.active_missions.write().await;
        let mut resource = self.resource_snapshot.write().await;

        // Drain proposals that pass the cap gate.
        let mut remaining = Vec::new();
        for proposal in proposals.drain(..) {
            let result = self.cap_gate.check_offline(
                &proposal,
                resource.daily_spend_microdollars,
                resource.concurrent_missions,
            );

            match result {
                CapGateResult::Approved => {
                    info!(
                        proposal_id = %proposal.id,
                        title = %proposal.title,
                        "converting approved proposal to mission"
                    );

                    // Create a single-step mission as a skeleton.
                    // The Coordinator will expand this into real steps when wired in.
                    let step = MissionStep {
                        step_index: 0,
                        description: proposal.description.clone(),
                        agent_id: proposal.agent_id.clone(),
                        status: StepStatus::Pending,
                        result: None,
                        started_at: None,
                        completed_at: None,
                    };
                    let mission = Mission::from_proposal(&proposal, vec![step]);

                    // Update resource tracking.
                    resource.daily_spend_microdollars += proposal.estimated_cost_microdollars;
                    resource.concurrent_missions += 1;

                    missions.push(mission);
                }
                CapGateResult::Rejected { reason } => {
                    warn!(
                        proposal_id = %proposal.id,
                        reason = %reason,
                        "proposal rejected by cap gate — will retry next tick"
                    );
                    remaining.push(proposal);
                }
            }
        }

        *proposals = remaining;
    }

    /// Advance active missions by checking step status and updating mission state.
    ///
    /// In the skeleton implementation, this marks missions with all-completed steps
    /// as completed, and detects stale missions.
    async fn advance_missions(&self) {
        let mut missions = self.active_missions.write().await;
        let stale_threshold = i64::from(self.config.stale_threshold_minutes);

        for mission in missions.iter_mut() {
            if mission.is_terminal() {
                continue;
            }

            // Check for stale missions.
            if mission.is_stale(stale_threshold) {
                warn!(
                    mission_id = %mission.id,
                    title = %mission.title,
                    "mission is stale — marking as stalled"
                );
                mission.status = MissionStatus::Stalled;
                mission.updated_at = Utc::now();
                continue;
            }

            // Check if all steps are completed.
            let all_complete = !mission.steps.is_empty()
                && mission
                    .steps
                    .iter()
                    .all(|s| s.status == StepStatus::Completed || s.status == StepStatus::Skipped);

            let any_failed = mission.steps.iter().any(|s| s.status == StepStatus::Failed);

            if any_failed {
                info!(
                    mission_id = %mission.id,
                    title = %mission.title,
                    "mission has failed steps — marking as failed"
                );
                mission.status = MissionStatus::Failed;
                mission.updated_at = Utc::now();
            } else if all_complete {
                info!(
                    mission_id = %mission.id,
                    title = %mission.title,
                    "all mission steps complete — marking as completed"
                );
                mission.status = MissionStatus::Completed;
                mission.updated_at = Utc::now();
            } else if mission.status == MissionStatus::Pending {
                // Move from Pending to InProgress once we start processing.
                mission.status = MissionStatus::InProgress;
                mission.updated_at = Utc::now();
            }
        }
    }

    /// Emit events for completed/failed missions and evaluate triggers + reactions.
    async fn fire_triggers(&self) {
        // Collect events from terminal missions while holding only a read lock.
        let events: Vec<AutopilotEvent> = {
            let missions = self.active_missions.read().await;
            missions
                .iter()
                .filter_map(|mission| {
                    let event_type = match mission.status {
                        MissionStatus::Completed => "mission.completed",
                        MissionStatus::Failed => "mission.failed",
                        MissionStatus::Stalled => "mission.stalled",
                        _ => return None,
                    };
                    Some(
                        AutopilotEvent::new(
                            event_type,
                            &mission.assigned_agent,
                            serde_json::json!({
                                "mission_id": mission.id,
                                "title": mission.title,
                            }),
                        )
                        .with_correlation(mission.id.clone()),
                    )
                })
                .collect()
        };
        // Read lock on active_missions is dropped here.

        // Process events against triggers and reaction matrix.
        for event in &events {
            let trigger_actions = self.trigger_engine.evaluate(event).await;
            for (rule_id, action) in &trigger_actions {
                info!(
                    rule_id = %rule_id,
                    action = ?action,
                    "trigger fired for mission event"
                );
                self.trigger_engine.mark_fired(rule_id).await;
            }

            let rm = self.reaction_matrix.read().await;
            if let Some(matrix) = rm.as_ref() {
                let reaction_actions = matrix.evaluate(event).await;
                for ra in &reaction_actions {
                    info!(
                        target_agent = %ra.target_agent,
                        action = %ra.action,
                        source_event = %ra.source_event,
                        "reaction fired for mission event"
                    );
                    matrix.mark_fired(&ra.cooldown_key).await;
                }
            }
        }

        // Clean up terminal missions from the active list and update resource snapshot.
        let mut missions = self.active_missions.write().await;
        let terminal_count = missions
            .iter()
            .filter(|m| m.is_terminal() || m.status == MissionStatus::Stalled)
            .count();

        if terminal_count > 0 {
            missions.retain(|m| !m.is_terminal() && m.status != MissionStatus::Stalled);

            let mut resource = self.resource_snapshot.write().await;
            resource.concurrent_missions = missions.len();

            debug!(
                removed = terminal_count,
                remaining = missions.len(),
                "cleaned up terminal missions"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Priority, ProposalStatus, ProposalType};

    fn test_config() -> AutopilotConfig {
        AutopilotConfig {
            enabled: true,
            max_daily_spend_cents: 100,
            max_concurrent_missions: 3,
            max_proposals_per_hour: 10,
            max_missions_per_agent_per_day: 5,
            stale_threshold_minutes: 30,
            ..Default::default()
        }
    }

    fn test_proposal(title: &str, cost_microdollars: u64) -> Proposal {
        let mut p = Proposal::new(
            "test-agent",
            title,
            "test description",
            ProposalType::TaskRequest,
            Priority::Medium,
            cost_microdollars,
        );
        p.status = ProposalStatus::Approved;
        p
    }

    #[test]
    fn creates_with_default_config() {
        let config = AutopilotConfig::default();
        let ap = AutopilotLoop::new(config.clone());
        assert_eq!(ap.tick_interval_secs, DEFAULT_TICK_INTERVAL_SECS);
        assert_eq!(
            ap.cap_gate.max_daily_spend_microdollars,
            config.max_daily_spend_cents * 10_000
        );
        assert_eq!(
            ap.cap_gate.max_concurrent_missions,
            config.max_concurrent_missions
        );
    }

    #[tokio::test]
    async fn shuts_down_cleanly_on_signal() {
        let config = test_config();
        let ap = AutopilotLoop::new(config);
        let (tx, rx) = tokio::sync::watch::channel(false);

        let handle = tokio::spawn(async move {
            ap.run(rx).await;
        });

        // Give the loop a moment to start, then signal shutdown.
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        tx.send(true).expect("send shutdown signal");

        // The loop should exit within a reasonable time.
        let result = tokio::time::timeout(tokio::time::Duration::from_secs(5), handle).await;
        assert!(result.is_ok(), "loop should shut down within 5 seconds");
        assert!(
            result.expect("timeout already checked").is_ok(),
            "loop task should not panic"
        );
    }

    #[tokio::test]
    async fn cap_gate_blocks_when_budget_exceeded() {
        let config = AutopilotConfig {
            enabled: true,
            max_daily_spend_cents: 1, // $0.01 = 10_000 microdollars
            max_concurrent_missions: 5,
            ..Default::default()
        };
        let ap = AutopilotLoop::new(config);

        // Set resource snapshot to already be at the spending limit.
        ap.update_resource_snapshot(10_000, 0).await;

        // Submit a proposal that would exceed the budget.
        let proposal = test_proposal("expensive task", 1);
        ap.submit_approved_proposal(proposal).await;

        // Run one tick.
        ap.tick().await.expect("tick should succeed");

        // The proposal should NOT have been converted to a mission (rejected by cap gate).
        let missions = ap.active_missions().await;
        assert!(
            missions.is_empty(),
            "no mission should be created when budget is exceeded"
        );

        // The proposal should still be in the queue for retry.
        let proposals = ap.approved_proposals.read().await;
        assert_eq!(
            proposals.len(),
            1,
            "rejected proposal should remain in queue"
        );
    }

    #[tokio::test]
    async fn proposal_converts_to_mission_within_budget() {
        let config = test_config();
        let ap = AutopilotLoop::new(config);

        let proposal = test_proposal("write blog post", 50_000);
        ap.submit_approved_proposal(proposal).await;

        ap.tick().await.expect("tick should succeed");

        let missions = ap.active_missions().await;
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].title, "write blog post");
        assert_eq!(missions[0].status, MissionStatus::InProgress);
        assert_eq!(missions[0].steps.len(), 1);
    }

    #[tokio::test]
    async fn cap_gate_blocks_at_mission_capacity() {
        let config = AutopilotConfig {
            enabled: true,
            max_daily_spend_cents: 10_000,
            max_concurrent_missions: 2,
            ..Default::default()
        };
        let ap = AutopilotLoop::new(config);

        // Set concurrent missions to the limit.
        ap.update_resource_snapshot(0, 2).await;

        let proposal = test_proposal("task", 1000);
        ap.submit_approved_proposal(proposal).await;

        ap.tick().await.expect("tick should succeed");

        let missions = ap.active_missions().await;
        assert!(
            missions.is_empty(),
            "no mission should be created when at mission capacity"
        );
    }

    #[tokio::test]
    async fn tick_count_increments() {
        let config = test_config();
        let ap = AutopilotLoop::new(config);

        assert_eq!(ap.tick_count().await, 0);
        ap.tick().await.expect("tick 1");
        assert_eq!(ap.tick_count().await, 1);
        ap.tick().await.expect("tick 2");
        assert_eq!(ap.tick_count().await, 2);
    }

    #[tokio::test]
    async fn heartbeat_updates_on_tick() {
        let config = test_config();
        let ap = AutopilotLoop::new(config);

        let before = ap.last_heartbeat().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        ap.tick().await.expect("tick");
        let after = ap.last_heartbeat().await;

        assert!(after >= before, "heartbeat should advance after tick");
    }

    #[tokio::test]
    async fn completed_mission_is_cleaned_up() {
        let config = test_config();
        let ap = AutopilotLoop::new(config);

        // Submit and convert a proposal.
        let proposal = test_proposal("quick task", 1000);
        ap.submit_approved_proposal(proposal).await;
        ap.tick().await.expect("tick 1 — converts proposal");

        // Mark the step as completed.
        {
            let mut missions = ap.active_missions.write().await;
            assert_eq!(missions.len(), 1);
            missions[0].steps[0].status = StepStatus::Completed;
            missions[0].steps[0].completed_at = Some(Utc::now());
        }

        // Next tick should detect completion and clean up.
        ap.tick().await.expect("tick 2 — completes mission");

        // After trigger processing, terminal missions are removed.
        let missions = ap.active_missions().await;
        assert!(
            missions.is_empty(),
            "completed mission should be cleaned up"
        );
    }

    #[tokio::test]
    async fn stale_mission_is_marked_stalled() {
        let config = AutopilotConfig {
            enabled: true,
            stale_threshold_minutes: 1,
            max_daily_spend_cents: 10_000,
            max_concurrent_missions: 5,
            ..Default::default()
        };
        let ap = AutopilotLoop::new(config);

        let proposal = test_proposal("slow task", 1000);
        ap.submit_approved_proposal(proposal).await;
        ap.tick().await.expect("tick 1");

        // Simulate a stale heartbeat by moving it far into the past.
        {
            let mut missions = ap.active_missions.write().await;
            missions[0].heartbeat_at = Utc::now() - chrono::Duration::minutes(10);
        }

        ap.tick().await.expect("tick 2 — detect stale");

        // After stale detection + cleanup, the mission should be removed.
        let missions = ap.active_missions().await;
        assert!(
            missions.is_empty(),
            "stalled mission should be cleaned up after trigger firing"
        );
    }
}

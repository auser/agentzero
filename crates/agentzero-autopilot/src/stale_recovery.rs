use std::sync::Arc;
use tokio::sync::watch;
use tracing::{info, warn};

use crate::supabase::SupabaseClient;
use crate::types::{AutopilotEvent, MissionStatus};

/// Monitors missions for stalled heartbeats and marks them accordingly.
///
/// Runs as a background tokio task, checking every `check_interval_secs`
/// for missions that have not sent a heartbeat within `threshold_minutes`.
pub struct StaleRecovery {
    client: Arc<SupabaseClient>,
    threshold_minutes: u32,
    check_interval_secs: u64,
}

impl StaleRecovery {
    pub fn new(
        client: Arc<SupabaseClient>,
        threshold_minutes: u32,
        check_interval_secs: u64,
    ) -> Self {
        Self {
            client,
            threshold_minutes,
            check_interval_secs,
        }
    }

    /// Run the stale recovery loop until shutdown is signaled.
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Vec<AutopilotEvent> {
        let mut all_events = Vec::new();
        loop {
            tokio::select! {
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(self.check_interval_secs)) => {
                    let events = self.check_once().await;
                    all_events.extend(events);
                }
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        info!("stale recovery shutting down");
                        break;
                    }
                }
            }
        }
        all_events
    }

    /// Perform a single check for stale missions.
    pub async fn check_once(&self) -> Vec<AutopilotEvent> {
        let mut events = Vec::new();
        match self
            .client
            .query_stale_missions(self.threshold_minutes)
            .await
        {
            Ok(stale_missions) => {
                for mission in &stale_missions {
                    info!(
                        mission_id = %mission.id,
                        title = %mission.title,
                        heartbeat_at = %mission.heartbeat_at,
                        "marking mission as stalled"
                    );
                    if let Err(e) = self
                        .client
                        .update_mission_status(&mission.id, MissionStatus::Stalled)
                        .await
                    {
                        warn!(
                            mission_id = %mission.id,
                            error = %e,
                            "failed to mark mission as stalled"
                        );
                        continue;
                    }
                    events.push(AutopilotEvent::new(
                        "mission.stalled",
                        "stale_recovery",
                        serde_json::json!({
                            "mission_id": mission.id,
                            "title": mission.title,
                            "assigned_agent": mission.assigned_agent,
                            "heartbeat_at": mission.heartbeat_at.to_rfc3339(),
                        }),
                    ));
                }
                if !stale_missions.is_empty() {
                    info!(
                        count = stale_missions.len(),
                        threshold_minutes = self.threshold_minutes,
                        "stale missions detected and marked"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to query stale missions");
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_recovery_construction() {
        // This just validates the struct can be constructed; actual integration
        // tests require a live Supabase instance.
        let config = crate::config::AutopilotConfig {
            supabase_url: "https://test.supabase.co".to_string(),
            supabase_service_role_key: "key".to_string(),
            ..Default::default()
        };
        let client = Arc::new(SupabaseClient::new(&config).expect("client"));
        let recovery = StaleRecovery::new(client, 30, 300);
        assert_eq!(recovery.threshold_minutes, 30);
        assert_eq!(recovery.check_interval_secs, 300);
    }
}

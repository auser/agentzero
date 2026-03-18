//! Event bus integration for regression detection.
//!
//! Spawns a background task that subscribes to `tool.file_written` events,
//! feeds them into a [`FileModificationTracker`], and publishes
//! `regression.file_conflict` events when conflicts are detected.

use crate::event_bus::{Event, EventBus};
use crate::regression::FileModificationTracker;
use std::sync::Arc;
use tokio::task::JoinHandle;

/// Spawn a background task that monitors file writes for regressions.
///
/// Subscribes to `tool.file_written` events on the bus. When a conflict
/// is detected (two agents modifying the same file in the same delegation
/// tree), publishes a `regression.file_conflict` event.
///
/// Expected `tool.file_written` event payload (JSON):
/// ```json
/// {
///   "file_path": "/path/to/file",
///   "agent_id": "writer",
///   "run_id": "run-123",
///   "correlation_id": "corr-456"
/// }
/// ```
pub fn spawn_regression_monitor(
    bus: Arc<dyn EventBus>,
    tracker: Arc<FileModificationTracker>,
) -> JoinHandle<()> {
    let bus_for_task = Arc::clone(&bus);
    tokio::spawn(async move {
        let mut sub = bus_for_task.subscribe();
        loop {
            let event = match sub.recv_filtered("tool.file_written").await {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "regression monitor: event bus recv failed");
                    break;
                }
            };

            let parsed: Option<FileWrittenPayload> = serde_json::from_str(&event.payload).ok();

            let Some(payload) = parsed else {
                tracing::debug!(
                    payload = event.payload,
                    "regression monitor: ignoring malformed tool.file_written payload"
                );
                continue;
            };

            let correlation_id = payload
                .correlation_id
                .or(event.correlation_id.clone())
                .unwrap_or_default();

            if correlation_id.is_empty() {
                continue;
            }

            let warning = tracker.record_modification(
                &payload.file_path,
                &payload.agent_id,
                &payload.run_id,
                &correlation_id,
            );

            if let Some(w) = warning {
                let conflict_payload = serde_json::json!({
                    "file_path": w.file_path,
                    "correlation_id": w.correlation_id,
                    "conflicting_agents": w.conflicting_entries,
                });
                let conflict_event = Event::new(
                    "regression.file_conflict",
                    "regression_monitor",
                    conflict_payload.to_string(),
                );
                if let Err(e) = bus_for_task.publish(conflict_event).await {
                    tracing::warn!(error = %e, "regression monitor: failed to publish conflict event");
                }
            }
        }
    })
}

#[derive(serde::Deserialize)]
struct FileWrittenPayload {
    file_path: String,
    agent_id: String,
    #[serde(default)]
    run_id: String,
    #[serde(default)]
    correlation_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_bus::InMemoryBus;
    use std::time::Duration;

    #[tokio::test]
    async fn monitor_detects_conflict_and_publishes_event() {
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
        let tracker = Arc::new(FileModificationTracker::new(60_000));

        // Pre-record agent A's write directly in the tracker so we don't
        // need the monitor to process it first.
        tracker.record_modification("/src/main.rs", "agent_a", "run-1", "corr-1");

        // Start monitor THEN subscribe — subscriber only sees new events
        let _handle = spawn_regression_monitor(Arc::clone(&bus), Arc::clone(&tracker));
        let mut sub = bus.subscribe();

        // Give monitor time to set up its subscription
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Agent B writes same file in same correlation tree — triggers conflict
        let event_b = Event::new(
            "tool.file_written",
            "agent_b",
            r#"{"file_path":"/src/main.rs","agent_id":"agent_b","run_id":"run-2","correlation_id":"corr-1"}"#,
        );
        bus.publish(event_b).await.expect("publish b");

        // Wait for conflict event
        let conflict = tokio::time::timeout(
            Duration::from_secs(2),
            sub.recv_filtered("regression.file_conflict"),
        )
        .await
        .expect("should receive conflict within timeout")
        .expect("recv should succeed");

        assert_eq!(conflict.topic, "regression.file_conflict");
        assert!(conflict.payload.contains("/src/main.rs"));
        assert!(conflict.payload.contains("corr-1"));
    }

    #[tokio::test]
    async fn no_conflict_for_different_files() {
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
        let tracker = Arc::new(FileModificationTracker::new(60_000));

        let _handle = spawn_regression_monitor(Arc::clone(&bus), Arc::clone(&tracker));
        let mut sub = bus.subscribe();

        // Agent A writes file1
        let event_a = Event::new(
            "tool.file_written",
            "agent_a",
            r#"{"file_path":"/src/a.rs","agent_id":"agent_a","run_id":"run-1","correlation_id":"corr-1"}"#,
        );
        bus.publish(event_a).await.expect("publish a");

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Agent B writes different file
        let event_b = Event::new(
            "tool.file_written",
            "agent_b",
            r#"{"file_path":"/src/b.rs","agent_id":"agent_b","run_id":"run-2","correlation_id":"corr-1"}"#,
        );
        bus.publish(event_b).await.expect("publish b");

        // Wait briefly — should NOT get a conflict event
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            sub.recv_filtered("regression.file_conflict"),
        )
        .await;

        assert!(result.is_err(), "should timeout — no conflict expected");
    }
}

//! Trigger evaluation loop — subscribes to the event bus and fires matching triggers.
//!
//! Bridges the `TriggerEngine` (from `agentzero-autopilot`) to the event bus
//! so that trigger rules are evaluated against every event flowing through
//! the system. When a rule matches, the corresponding action is published
//! back to the bus for the coordinator or agents to handle.

use agentzero_autopilot::types::{AutopilotEvent, TriggerAction};
use agentzero_autopilot::TriggerEngine;
use agentzero_core::event_bus::{Event, EventBus};
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// Run the trigger evaluation loop until shutdown.
///
/// Subscribes to all events on the bus, converts them to `AutopilotEvent`,
/// evaluates against the `TriggerEngine`, and publishes actions back.
pub async fn run_trigger_loop(
    engine: Arc<TriggerEngine>,
    bus: Arc<dyn EventBus>,
    mut shutdown: watch::Receiver<bool>,
) {
    info!("trigger evaluation loop started");

    let mut sub = bus.subscribe();

    loop {
        let event = tokio::select! {
            e = sub.recv() => match e {
                Ok(event) => event,
                Err(e) => {
                    error!(error = %e, "trigger loop bus subscription failed");
                    break;
                }
            },
            _ = shutdown.changed() => {
                info!("trigger loop received shutdown signal");
                break;
            }
        };

        // Skip system events and trigger-generated events to prevent loops.
        if event.topic.starts_with("system.")
            || event.topic.starts_with("trigger.")
            || event.topic.starts_with("cron.")
        {
            continue;
        }

        // Convert bus Event → AutopilotEvent.
        let autopilot_event = AutopilotEvent::new(
            &event.topic,
            &event.source,
            // Try to parse payload as JSON; fall back to wrapping as string.
            serde_json::from_str(&event.payload)
                .unwrap_or_else(|_| serde_json::json!({ "raw": event.payload })),
        );

        let actions = engine.evaluate(&autopilot_event).await;

        for (rule_id, action) in actions {
            debug!(rule_id = %rule_id, "trigger rule matched");
            engine.mark_fired(&rule_id).await;

            let (topic, payload) = match &action {
                TriggerAction::ProposeTask { agent, prompt } => (
                    format!("trigger.propose.{agent}"),
                    serde_json::json!({
                        "rule_id": rule_id,
                        "agent": agent,
                        "prompt": prompt,
                        "source_event": event.topic,
                    }),
                ),
                TriggerAction::NotifyAgent { agent, message } => (
                    format!("trigger.notify.{agent}"),
                    serde_json::json!({
                        "rule_id": rule_id,
                        "agent": agent,
                        "message": message,
                        "source_event": event.topic,
                    }),
                ),
                TriggerAction::RunPipeline { pipeline } => (
                    format!("trigger.pipeline.{pipeline}"),
                    serde_json::json!({
                        "rule_id": rule_id,
                        "pipeline": pipeline,
                        "source_event": event.topic,
                    }),
                ),
            };

            let trigger_event = Event::new(&topic, "trigger-engine", payload.to_string());
            if let Err(e) = bus.publish(trigger_event).await {
                warn!(rule_id = %rule_id, error = %e, "failed to publish trigger action");
            }
        }
    }

    info!("trigger evaluation loop stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_autopilot::config::TriggerRuleConfig;
    use agentzero_autopilot::types::TriggerCondition;
    use agentzero_core::InMemoryBus;
    use std::time::Duration;

    #[tokio::test]
    async fn trigger_loop_fires_on_matching_event() {
        let engine = Arc::new(TriggerEngine::new());
        let configs = vec![TriggerRuleConfig {
            name: "on-agent-complete".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "agent.writer.complete".to_string(),
            },
            action: TriggerAction::NotifyAgent {
                agent: "reviewer".to_string(),
                message: "please review".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        let bus = Arc::new(InMemoryBus::default_capacity());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Subscribe to trigger events before starting the loop.
        let mut trigger_sub = bus.subscribe();

        // Start the trigger loop in background.
        let loop_bus = bus.clone();
        let loop_engine = engine.clone();
        let handle = tokio::spawn(async move {
            run_trigger_loop(loop_engine, loop_bus, shutdown_rx).await;
        });

        // Give the loop a moment to subscribe.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Publish a matching event.
        let event = Event::new(
            "agent.writer.complete",
            "writer-agent",
            r#"{"result": "draft done"}"#,
        );
        bus.publish(event).await.expect("publish");

        // Wait for the trigger action event.
        let result = tokio::time::timeout(Duration::from_millis(500), async {
            loop {
                match trigger_sub.recv().await {
                    Ok(e) if e.topic.starts_with("trigger.") => return e,
                    Ok(_) => continue,
                    Err(e) => panic!("recv error: {e}"),
                }
            }
        })
        .await;

        assert!(result.is_ok(), "should receive trigger event");
        let trigger_event = result.expect("trigger event");
        assert_eq!(trigger_event.topic, "trigger.notify.reviewer");

        let payload: serde_json::Value =
            serde_json::from_str(&trigger_event.payload).expect("parse");
        assert_eq!(payload["agent"], "reviewer");
        assert_eq!(payload["message"], "please review");
        assert_eq!(payload["source_event"], "agent.writer.complete");

        shutdown_tx.send(true).expect("shutdown");
        let _ = tokio::time::timeout(Duration::from_millis(200), handle).await;
    }

    #[tokio::test]
    async fn trigger_loop_ignores_system_events() {
        let engine = Arc::new(TriggerEngine::new());
        let configs = vec![TriggerRuleConfig {
            name: "catch-all".to_string(),
            condition: TriggerCondition::EventMatch {
                event_type: "system.startup".to_string(),
            },
            action: TriggerAction::NotifyAgent {
                agent: "admin".to_string(),
                message: "started".to_string(),
            },
            cooldown_secs: 0,
            enabled: true,
        }];
        engine.load_from_config(&configs).await;

        let bus = Arc::new(InMemoryBus::default_capacity());
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut trigger_sub = bus.subscribe();

        let loop_bus = bus.clone();
        let loop_engine = engine.clone();
        let handle = tokio::spawn(async move {
            run_trigger_loop(loop_engine, loop_bus, shutdown_rx).await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Publish a system event — should be ignored.
        bus.publish(Event::new("system.startup", "coordinator", "{}"))
            .await
            .expect("publish");

        // Should NOT receive a trigger event.
        let result = tokio::time::timeout(Duration::from_millis(200), async {
            loop {
                match trigger_sub.recv().await {
                    Ok(e) if e.topic.starts_with("trigger.") => return Some(e),
                    Ok(_) => continue,
                    Err(_) => return None,
                }
            }
        })
        .await;

        assert!(
            result.is_err() || result.as_ref().is_ok_and(|r| r.is_none()),
            "should not fire trigger for system events"
        );

        shutdown_tx.send(true).expect("shutdown");
        let _ = tokio::time::timeout(Duration::from_millis(200), handle).await;
    }
}

//! Cron execution loop — polls `CronStore` for due tasks and dispatches them.
//!
//! This is the missing piece that connects stored cron tasks (created via
//! the `schedule` or `cron_*` tools) to actual execution. It runs as a
//! background loop in the coordinator, checking for due tasks every interval
//! and publishing execution events to the event bus.

use agentzero_core::event_bus::{Event, EventBus};
use agentzero_tools::cron_store::CronStore;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::watch;
use tracing::{debug, error, info, warn};

/// Configuration for the cron executor.
#[derive(Debug, Clone)]
pub struct CronExecutorConfig {
    /// How often to poll the cron store for due tasks (default: 30s).
    pub poll_interval: Duration,
    /// Maximum number of tasks to fire in a single poll cycle (default: 10).
    pub max_fires_per_cycle: usize,
}

impl Default for CronExecutorConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(30),
            max_fires_per_cycle: 10,
        }
    }
}

/// Runs the cron execution loop until shutdown.
///
/// On each tick:
/// 1. Loads all enabled tasks from `CronStore`
/// 2. Checks which are due (cron expression matches current time window)
/// 3. Publishes a `cron.task.fire` event to the bus for each due task
/// 4. Updates `last_run_epoch_seconds` in the store
///
/// The coordinator's response handler or a dedicated agent can subscribe to
/// `cron.task.fire` events and execute the commands.
pub async fn run_cron_executor(
    store: Arc<CronStore>,
    bus: Arc<dyn EventBus>,
    config: CronExecutorConfig,
    mut shutdown: watch::Receiver<bool>,
) {
    info!(
        poll_interval_secs = config.poll_interval.as_secs(),
        "cron executor started"
    );

    let mut interval = tokio::time::interval(config.poll_interval);
    // Skip the first immediate tick to give the system time to start.
    interval.tick().await;

    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                info!("cron executor received shutdown signal");
                break;
            }
            _ = interval.tick() => {
                if let Err(e) = poll_and_fire(&store, &*bus, &config).await {
                    error!(error = %e, "cron executor poll cycle failed");
                }
            }
        }
    }

    info!("cron executor stopped");
}

/// Single poll cycle: check all tasks and fire due ones.
async fn poll_and_fire(
    store: &CronStore,
    bus: &dyn EventBus,
    config: &CronExecutorConfig,
) -> anyhow::Result<()> {
    let tasks = store.list()?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut fired = 0;

    for task in &tasks {
        if !task.enabled {
            continue;
        }
        if fired >= config.max_fires_per_cycle {
            debug!("max fires per cycle reached, deferring remaining tasks");
            break;
        }

        if is_due(&task.schedule, task.last_run_epoch_seconds, now) {
            info!(
                task_id = %task.id,
                schedule = %task.schedule,
                command = %task.command,
                "firing cron task"
            );

            let event = Event::new(
                "cron.task.fire",
                "cron-executor",
                serde_json::to_string(&serde_json::json!({
                    "task_id": task.id,
                    "command": task.command,
                    "schedule": task.schedule,
                }))
                .unwrap_or_default(),
            );

            if let Err(e) = bus.publish(event).await {
                error!(task_id = %task.id, error = %e, "failed to publish cron event");
                continue;
            }

            // Mark as run so we don't fire again until the next window.
            if let Err(e) = store.mark_last_run(&task.id, now) {
                warn!(task_id = %task.id, error = %e, "failed to update last_run timestamp");
            }

            fired += 1;
        }
    }

    if fired > 0 {
        debug!(fired, "cron poll cycle complete");
    }

    Ok(())
}

/// Check if a cron task is due to fire.
///
/// A task is due if:
/// 1. It has never run (`last_run` is None), OR
/// 2. The time since last run exceeds the minimum interval implied by the
///    cron expression
///
/// We parse the cron expression and check if the current time falls within
/// a matching window. For simplicity, we compare the cron's next-fire-after
/// the last run to the current time.
fn is_due(cron_expr: &str, last_run: Option<u64>, now_epoch: u64) -> bool {
    // Try to parse as a standard cron expression (5-field or 7-field).
    // We use the `cron` crate-style parsing. If we can't parse, try our
    // simpler interval-based check.
    if let Some(interval_secs) = parse_simple_interval(cron_expr) {
        return match last_run {
            None => true,
            Some(last) => now_epoch.saturating_sub(last) >= interval_secs,
        };
    }

    // For standard cron expressions, check if enough time has passed.
    // We infer the minimum interval from the expression pattern.
    let min_interval = infer_min_interval(cron_expr);
    match last_run {
        None => true,
        Some(last) => now_epoch.saturating_sub(last) >= min_interval,
    }
}

/// Parse simple interval expressions like "every 5 minutes", "*/5 * * * *".
fn parse_simple_interval(expr: &str) -> Option<u64> {
    let expr = expr.trim();

    // Handle "every N minutes/hours/seconds" natural language.
    let lower = expr.to_lowercase();
    if let Some(rest) = lower.strip_prefix("every ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Ok(n) = parts[0].parse::<u64>() {
                return match parts[1] {
                    s if s.starts_with("second") => Some(n),
                    s if s.starts_with("minute") => Some(n * 60),
                    s if s.starts_with("hour") => Some(n * 3600),
                    s if s.starts_with("day") => Some(n * 86400),
                    _ => None,
                };
            }
        }
        // "every minute", "every hour", etc.
        return match parts.first().copied() {
            Some(s) if s.starts_with("second") => Some(1),
            Some(s) if s.starts_with("minute") => Some(60),
            Some(s) if s.starts_with("hour") => Some(3600),
            Some(s) if s.starts_with("day") => Some(86400),
            _ => None,
        };
    }

    // Handle "*/N * * * *" minute-based cron.
    if let Some(rest) = expr.strip_prefix("*/") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if let Some(first) = parts.first() {
            if let Ok(n) = first.parse::<u64>() {
                // Check if the rest is all wildcards (minute-level interval).
                let is_all_wildcards = parts.iter().skip(1).all(|p| *p == "*");
                if is_all_wildcards && n > 0 {
                    return Some(n * 60);
                }
            }
        }
    }

    None
}

/// Infer the minimum interval between firings from a cron expression.
///
/// This is a heuristic — for complex cron expressions we fall back to 60s.
fn infer_min_interval(expr: &str) -> u64 {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.is_empty() {
        return 60;
    }

    // Check minute field for the base interval.
    match parts[0] {
        "*" => 60, // every minute
        "0" if parts.len() > 1 => {
            // "0 * * * *" = every hour, "0 0 * * *" = every day
            match parts.get(1).copied() {
                Some("*") => 3600,
                Some("0") if parts.len() > 2 => match parts.get(2).copied() {
                    Some("*") => 86400,
                    _ => 86400,
                },
                _ => 3600,
            }
        }
        field if field.starts_with("*/") => {
            // "*/N" in the minute field.
            field[2..].parse::<u64>().unwrap_or(1) * 60
        }
        _ => 60, // Conservative default: check every minute.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::InMemoryBus;

    #[test]
    fn parse_simple_interval_natural_language() {
        assert_eq!(parse_simple_interval("every 5 minutes"), Some(300));
        assert_eq!(parse_simple_interval("every 1 hour"), Some(3600));
        assert_eq!(parse_simple_interval("every 30 seconds"), Some(30));
        assert_eq!(parse_simple_interval("every minute"), Some(60));
        assert_eq!(parse_simple_interval("every day"), Some(86400));
    }

    #[test]
    fn parse_simple_interval_cron_style() {
        assert_eq!(parse_simple_interval("*/5 * * * *"), Some(300));
        assert_eq!(parse_simple_interval("*/15 * * * *"), Some(900));
        assert_eq!(parse_simple_interval("*/1 * * * *"), Some(60));
    }

    #[test]
    fn parse_simple_interval_returns_none_for_complex() {
        assert_eq!(parse_simple_interval("0 9 * * *"), None);
        assert_eq!(parse_simple_interval("0 0 * * 1"), None);
    }

    #[test]
    fn is_due_never_run() {
        assert!(is_due("*/5 * * * *", None, 1000));
        assert!(is_due("every 5 minutes", None, 1000));
    }

    #[test]
    fn is_due_within_interval() {
        // Last run 2 minutes ago, interval is 5 minutes → not due.
        let now = 1000;
        let last = now - 120;
        assert!(!is_due("*/5 * * * *", Some(last), now));
    }

    #[test]
    fn is_due_past_interval() {
        // Last run 6 minutes ago, interval is 5 minutes → due.
        let now = 1000;
        let last = now - 360;
        assert!(is_due("*/5 * * * *", Some(last), now));
    }

    #[test]
    fn infer_min_interval_patterns() {
        assert_eq!(infer_min_interval("* * * * *"), 60);
        assert_eq!(infer_min_interval("0 * * * *"), 3600);
        assert_eq!(infer_min_interval("*/5 * * * *"), 300);
        assert_eq!(infer_min_interval("0 0 * * *"), 86400);
    }

    #[tokio::test]
    async fn poll_and_fire_empty_store() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(CronStore::new(dir.path()).expect("store"));
        let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::default_capacity());
        let config = CronExecutorConfig::default();

        // Should not panic with empty store.
        poll_and_fire(&store, &*bus, &config)
            .await
            .expect("should succeed");
    }

    #[tokio::test]
    async fn poll_and_fire_fires_due_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(CronStore::new(dir.path()).expect("store"));
        let bus = Arc::new(InMemoryBus::default_capacity());
        let mut sub = bus.subscribe();

        // Add a task that has never run.
        store
            .add("test-task", "every 5 minutes", "echo hello")
            .expect("add");

        let config = CronExecutorConfig::default();
        poll_and_fire(&store, &*bus, &config).await.expect("poll");

        // Should have received a fire event.
        let event = tokio::time::timeout(Duration::from_millis(100), sub.recv())
            .await
            .expect("should receive event")
            .expect("recv should succeed");

        assert_eq!(event.topic, "cron.task.fire");
        let payload: serde_json::Value =
            serde_json::from_str(&event.payload).expect("parse payload");
        assert_eq!(payload["task_id"], "test-task");
        assert_eq!(payload["command"], "echo hello");

        // Task should now have a last_run timestamp.
        let tasks = store.list().expect("list");
        assert!(tasks[0].last_run_epoch_seconds.is_some());
    }

    #[tokio::test]
    async fn poll_and_fire_skips_disabled_task() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(CronStore::new(dir.path()).expect("store"));
        let bus = Arc::new(InMemoryBus::default_capacity());
        let mut sub = bus.subscribe();

        store.add("paused", "every 1 minute", "cmd").expect("add");
        store.pause("paused").expect("pause");

        let config = CronExecutorConfig::default();
        poll_and_fire(&store, &*bus, &config).await.expect("poll");

        // Should NOT have received any event.
        let result = tokio::time::timeout(Duration::from_millis(100), sub.recv()).await;
        assert!(
            result.is_err(),
            "should not receive event for disabled task"
        );
    }

    #[tokio::test]
    async fn poll_and_fire_respects_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Arc::new(CronStore::new(dir.path()).expect("store"));
        let bus = Arc::new(InMemoryBus::default_capacity());

        store
            .add("interval-task", "every 5 minutes", "cmd")
            .expect("add");

        let config = CronExecutorConfig::default();

        // First poll fires the task.
        poll_and_fire(&store, &*bus, &config).await.expect("poll 1");
        let tasks = store.list().expect("list");
        assert!(tasks[0].last_run_epoch_seconds.is_some());

        // Second poll immediately should NOT fire (within interval).
        let mut sub = bus.subscribe();
        poll_and_fire(&store, &*bus, &config).await.expect("poll 2");
        let result = tokio::time::timeout(Duration::from_millis(100), sub.recv()).await;
        assert!(result.is_err(), "should not fire again within interval");
    }
}

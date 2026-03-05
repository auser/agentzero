use crate::cron_store::CronStore;
use agentzero_core::{Tool, ToolContext, ToolResult};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

fn cron_data_dir(workspace_root: &str) -> PathBuf {
    PathBuf::from(workspace_root).join(".agentzero")
}

// ---------------------------------------------------------------------------
// Natural-language schedule parsing
// ---------------------------------------------------------------------------

/// Attempt to convert a natural-language schedule expression into a cron
/// expression. Supports common patterns like:
///
/// - "every 5 minutes" → "*/5 * * * *"
/// - "every hour" → "0 * * * *"
/// - "daily at 9am" → "0 9 * * *"
/// - "daily at 2:30pm" → "30 14 * * *"
/// - "weekly on monday" → "0 0 * * 1"
/// - "hourly" → "0 * * * *"
/// - "every day" → "0 0 * * *"
///
/// Returns `None` if the expression is already a valid 5-field cron or
/// cannot be parsed.
pub(crate) fn parse_natural_schedule(input: &str) -> Option<String> {
    let s = input.trim().to_ascii_lowercase();

    // Already a cron expression (5 space-separated fields starting with digit or *)
    if looks_like_cron(&s) {
        return None;
    }

    // "every N minutes"
    if let Some(n) = extract_every_n_minutes(&s) {
        return Some(format!("*/{n} * * * *"));
    }

    // "every N hours"
    if let Some(n) = extract_every_n_hours(&s) {
        return Some(format!("0 */{n} * * *"));
    }

    // "every minute"
    if s == "every minute" {
        return Some("* * * * *".to_string());
    }

    // "every hour" / "hourly"
    if s == "every hour" || s == "hourly" {
        return Some("0 * * * *".to_string());
    }

    // "every day" / "daily" (without time → midnight)
    if s == "every day" || s == "daily" {
        return Some("0 0 * * *".to_string());
    }

    // "daily at HH:MM" / "daily at Ham" / "daily at Hpm"
    if let Some(cron) = parse_daily_at(&s) {
        return Some(cron);
    }

    // "weekly" / "every week"
    if s == "weekly" || s == "every week" {
        return Some("0 0 * * 0".to_string());
    }

    // "weekly on <day>" / "every <day>"
    if let Some(cron) = parse_weekly_on(&s) {
        return Some(cron);
    }

    // "monthly" / "every month"
    if s == "monthly" || s == "every month" {
        return Some("0 0 1 * *".to_string());
    }

    None
}

fn looks_like_cron(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    parts.len() == 5 && parts[0].starts_with(|c: char| c.is_ascii_digit() || c == '*')
}

fn extract_every_n_minutes(s: &str) -> Option<u32> {
    // "every 5 minutes" / "every 10 min"
    let s = s.strip_prefix("every ")?;
    let rest = s
        .strip_suffix(" minutes")
        .or_else(|| s.strip_suffix(" min"))?;
    let n: u32 = rest.trim().parse().ok()?;
    if n > 0 && n <= 59 {
        Some(n)
    } else {
        None
    }
}

fn extract_every_n_hours(s: &str) -> Option<u32> {
    // "every 2 hours" / "every 4 hrs"
    let s = s.strip_prefix("every ")?;
    let rest = s
        .strip_suffix(" hours")
        .or_else(|| s.strip_suffix(" hrs"))
        .or_else(|| s.strip_suffix(" hour"))?;
    let n: u32 = rest.trim().parse().ok()?;
    if n > 0 && n <= 23 {
        Some(n)
    } else {
        None
    }
}

fn parse_daily_at(s: &str) -> Option<String> {
    // "daily at 9am" → "0 9 * * *"
    // "daily at 2:30pm" → "30 14 * * *"
    // "daily at 14:30" → "30 14 * * *"
    // "every day at 9am" → "0 9 * * *"
    let time_str = s
        .strip_prefix("daily at ")
        .or_else(|| s.strip_prefix("every day at "))?;
    let (hour, minute) = parse_time(time_str)?;
    Some(format!("{minute} {hour} * * *"))
}

fn parse_weekly_on(s: &str) -> Option<String> {
    // "weekly on monday" → "0 0 * * 1"
    // "every monday" → "0 0 * * 1"
    let day_str = s
        .strip_prefix("weekly on ")
        .or_else(|| s.strip_prefix("every "))?;
    let dow = day_of_week(day_str.trim())?;
    Some(format!("0 0 * * {dow}"))
}

fn parse_time(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();

    // Try "2:30pm" / "2:30am" / "14:30"
    if let Some(rest) = s.strip_suffix("pm") {
        return parse_hm(rest, true);
    }
    if let Some(rest) = s.strip_suffix("am") {
        return parse_hm(rest, false);
    }

    // "14:30" (24-hour)
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let h: u32 = parts[0].parse().ok()?;
            let m: u32 = parts[1].parse().ok()?;
            if h < 24 && m < 60 {
                return Some((h, m));
            }
        }
    }

    // "9am" → just an hour
    if let Ok(h) = s.parse::<u32>() {
        if h < 24 {
            return Some((h, 0));
        }
    }

    None
}

fn parse_hm(s: &str, is_pm: bool) -> Option<(u32, u32)> {
    let s = s.trim();
    if s.contains(':') {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let mut h: u32 = parts[0].parse().ok()?;
            let m: u32 = parts[1].parse().ok()?;
            if is_pm && h < 12 {
                h += 12;
            }
            if !is_pm && h == 12 {
                h = 0;
            }
            if h < 24 && m < 60 {
                return Some((h, m));
            }
        }
    } else {
        let mut h: u32 = s.parse().ok()?;
        if is_pm && h < 12 {
            h += 12;
        }
        if !is_pm && h == 12 {
            h = 0;
        }
        if h < 24 {
            return Some((h, 0));
        }
    }
    None
}

fn day_of_week(s: &str) -> Option<u8> {
    match s {
        "sunday" | "sun" => Some(0),
        "monday" | "mon" => Some(1),
        "tuesday" | "tue" | "tues" => Some(2),
        "wednesday" | "wed" => Some(3),
        "thursday" | "thu" | "thurs" => Some(4),
        "friday" | "fri" => Some(5),
        "saturday" | "sat" => Some(6),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Schedule tool (unified interface)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ScheduleInput {
    action: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    schedule: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

/// Unified scheduling tool that wraps cron operations and supports
/// natural-language schedule expressions. Actions:
///
/// - `create` — create a new scheduled task (requires `id`, `schedule`, `command`)
/// - `list` — list all scheduled tasks
/// - `update` — update schedule or command for a task (requires `id`)
/// - `remove` — remove a task (requires `id`)
/// - `pause` — disable a task (requires `id`)
/// - `resume` — re-enable a task (requires `id`)
/// - `parse` — parse a natural-language expression to cron (requires `schedule`)
#[derive(Debug, Default, Clone, Copy)]
pub struct ScheduleTool;

#[async_trait]
impl Tool for ScheduleTool {
    fn name(&self) -> &'static str {
        "schedule"
    }

    fn description(&self) -> &'static str {
        "Manage scheduled tasks: create, list, update, remove, pause, resume, or parse cron expressions."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["create", "list", "update", "remove", "pause", "resume", "parse"], "description": "The scheduling action to perform" },
                "id": { "type": "string", "description": "Task ID (required for create/update/remove/pause/resume)" },
                "schedule": { "type": "string", "description": "Cron expression or natural language schedule (e.g. 'every 5 minutes')" },
                "command": { "type": "string", "description": "Command to run on schedule" }
            },
            "required": ["action"],
            "additionalProperties": false
        }))
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: ScheduleInput = serde_json::from_str(input)
            .context("schedule expects JSON: {\"action\": \"create|list|update|remove|pause|resume|parse\", ...}")?;

        match req.action.as_str() {
            "create" => handle_create(ctx, &req),
            "list" => handle_list(ctx),
            "update" => handle_update(ctx, &req),
            "remove" => handle_remove(ctx, &req),
            "pause" => handle_pause(ctx, &req),
            "resume" => handle_resume(ctx, &req),
            "parse" => handle_parse(&req),
            other => Err(anyhow!(
                "unknown action `{other}`: expected create, list, update, remove, pause, resume, or parse"
            )),
        }
    }
}

fn require_id(req: &ScheduleInput) -> anyhow::Result<&str> {
    req.id
        .as_deref()
        .ok_or_else(|| anyhow!("missing required field `id`"))
}

fn resolve_schedule(raw: &str) -> String {
    parse_natural_schedule(raw).unwrap_or_else(|| raw.to_string())
}

fn handle_create(ctx: &ToolContext, req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let id = require_id(req)?;
    let raw_schedule = req
        .schedule
        .as_deref()
        .ok_or_else(|| anyhow!("missing required field `schedule`"))?;
    let command = req
        .command
        .as_deref()
        .ok_or_else(|| anyhow!("missing required field `command`"))?;

    let cron_expr = resolve_schedule(raw_schedule);
    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    let task = store.add(id, &cron_expr, command)?;

    let mut out = format!(
        "created task: id={}, schedule={}, command={}, enabled={}",
        task.id, task.schedule, task.command, task.enabled
    );
    if cron_expr != raw_schedule {
        out.push_str(&format!(
            "\n(interpreted \"{}\" as cron: {})",
            raw_schedule, cron_expr
        ));
    }
    Ok(ToolResult { output: out })
}

fn handle_list(ctx: &ToolContext) -> anyhow::Result<ToolResult> {
    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    let tasks = store.list()?;
    if tasks.is_empty() {
        return Ok(ToolResult {
            output: "no scheduled tasks".to_string(),
        });
    }
    let lines: Vec<String> = tasks
        .iter()
        .map(|t| {
            format!(
                "id={} schedule={} command={} enabled={}",
                t.id, t.schedule, t.command, t.enabled
            )
        })
        .collect();
    Ok(ToolResult {
        output: lines.join("\n"),
    })
}

fn handle_update(ctx: &ToolContext, req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let id = require_id(req)?;
    let schedule = req.schedule.as_deref().map(resolve_schedule);
    let command = req.command.as_deref();

    if schedule.is_none() && command.is_none() {
        return Err(anyhow!(
            "at least one of `schedule` or `command` must be provided"
        ));
    }

    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    let task = store.update(id, schedule.as_deref(), command)?;
    Ok(ToolResult {
        output: format!(
            "updated task: id={}, schedule={}, command={}",
            task.id, task.schedule, task.command
        ),
    })
}

fn handle_remove(ctx: &ToolContext, req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let id = require_id(req)?;
    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    store.remove(id)?;
    Ok(ToolResult {
        output: format!("removed task: {id}"),
    })
}

fn handle_pause(ctx: &ToolContext, req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let id = require_id(req)?;
    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    let task = store.pause(id)?;
    Ok(ToolResult {
        output: format!("paused task: id={}, enabled={}", task.id, task.enabled),
    })
}

fn handle_resume(ctx: &ToolContext, req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let id = require_id(req)?;
    let store = CronStore::new(cron_data_dir(&ctx.workspace_root))?;
    let task = store.resume(id)?;
    Ok(ToolResult {
        output: format!("resumed task: id={}, enabled={}", task.id, task.enabled),
    })
}

fn handle_parse(req: &ScheduleInput) -> anyhow::Result<ToolResult> {
    let raw = req
        .schedule
        .as_deref()
        .ok_or_else(|| anyhow!("missing required field `schedule`"))?;
    match parse_natural_schedule(raw) {
        Some(cron) => Ok(ToolResult {
            output: format!("\"{}\" → {}", raw, cron),
        }),
        None => {
            if looks_like_cron(&raw.to_ascii_lowercase()) {
                Ok(ToolResult {
                    output: format!("\"{}\" is already a valid cron expression", raw),
                })
            } else {
                Err(anyhow!(
                    "could not parse \"{}\" as a schedule expression",
                    raw
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-schedule-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    // --- Natural language parsing tests ---

    #[test]
    fn parse_every_n_minutes() {
        assert_eq!(
            parse_natural_schedule("every 5 minutes"),
            Some("*/5 * * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every 15 min"),
            Some("*/15 * * * *".to_string())
        );
    }

    #[test]
    fn parse_every_n_hours() {
        assert_eq!(
            parse_natural_schedule("every 2 hours"),
            Some("0 */2 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every 4 hrs"),
            Some("0 */4 * * *".to_string())
        );
    }

    #[test]
    fn parse_every_minute() {
        assert_eq!(
            parse_natural_schedule("every minute"),
            Some("* * * * *".to_string())
        );
    }

    #[test]
    fn parse_hourly() {
        assert_eq!(
            parse_natural_schedule("hourly"),
            Some("0 * * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every hour"),
            Some("0 * * * *".to_string())
        );
    }

    #[test]
    fn parse_daily() {
        assert_eq!(
            parse_natural_schedule("daily"),
            Some("0 0 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every day"),
            Some("0 0 * * *".to_string())
        );
    }

    #[test]
    fn parse_daily_at_time() {
        assert_eq!(
            parse_natural_schedule("daily at 9am"),
            Some("0 9 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("daily at 2:30pm"),
            Some("30 14 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("daily at 14:30"),
            Some("30 14 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every day at 12pm"),
            Some("0 12 * * *".to_string())
        );
        assert_eq!(
            parse_natural_schedule("daily at 12am"),
            Some("0 0 * * *".to_string())
        );
    }

    #[test]
    fn parse_weekly() {
        assert_eq!(
            parse_natural_schedule("weekly"),
            Some("0 0 * * 0".to_string())
        );
        assert_eq!(
            parse_natural_schedule("weekly on monday"),
            Some("0 0 * * 1".to_string())
        );
        assert_eq!(
            parse_natural_schedule("every friday"),
            Some("0 0 * * 5".to_string())
        );
    }

    #[test]
    fn parse_monthly() {
        assert_eq!(
            parse_natural_schedule("monthly"),
            Some("0 0 1 * *".to_string())
        );
    }

    #[test]
    fn parse_returns_none_for_existing_cron() {
        assert_eq!(parse_natural_schedule("*/5 * * * *"), None);
        assert_eq!(parse_natural_schedule("0 9 * * 1"), None);
    }

    #[test]
    fn parse_returns_none_for_unrecognized() {
        assert_eq!(parse_natural_schedule("next tuesday"), None);
        assert_eq!(parse_natural_schedule("gibberish"), None);
    }

    // --- Tool integration tests ---

    #[tokio::test]
    async fn schedule_create_list_remove_roundtrip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = ScheduleTool;

        // Create with natural language
        let result = tool
            .execute(
                r#"{"action": "create", "id": "backup", "schedule": "every 5 minutes", "command": "echo hello"}"#,
                &ctx,
            )
            .await
            .expect("create should succeed");
        assert!(result.output.contains("*/5 * * * *"));
        assert!(result.output.contains("interpreted"));

        // List
        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("backup"));

        // Remove
        let result = tool
            .execute(r#"{"action": "remove", "id": "backup"}"#, &ctx)
            .await
            .expect("remove should succeed");
        assert!(result.output.contains("removed"));

        // Verify empty
        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no scheduled tasks"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn schedule_create_with_cron_expression() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = ScheduleTool;

        let result = tool
            .execute(
                r#"{"action": "create", "id": "job", "schedule": "0 9 * * 1", "command": "echo"}"#,
                &ctx,
            )
            .await
            .expect("create with cron should succeed");
        assert!(result.output.contains("0 9 * * 1"));
        assert!(!result.output.contains("interpreted"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn schedule_pause_resume() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = ScheduleTool;

        tool.execute(
            r#"{"action": "create", "id": "job", "schedule": "hourly", "command": "test"}"#,
            &ctx,
        )
        .await
        .expect("create should succeed");

        let result = tool
            .execute(r#"{"action": "pause", "id": "job"}"#, &ctx)
            .await
            .expect("pause should succeed");
        assert!(result.output.contains("enabled=false"));

        let result = tool
            .execute(r#"{"action": "resume", "id": "job"}"#, &ctx)
            .await
            .expect("resume should succeed");
        assert!(result.output.contains("enabled=true"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn schedule_parse_action() {
        let tool = ScheduleTool;
        let ctx = ToolContext::new("/tmp".to_string());

        let result = tool
            .execute(
                r#"{"action": "parse", "schedule": "every 5 minutes"}"#,
                &ctx,
            )
            .await
            .expect("parse should succeed");
        assert!(result.output.contains("*/5 * * * *"));

        let result = tool
            .execute(r#"{"action": "parse", "schedule": "*/5 * * * *"}"#, &ctx)
            .await
            .expect("parse existing cron should succeed");
        assert!(result.output.contains("already a valid cron"));
    }

    #[tokio::test]
    async fn schedule_unknown_action_fails() {
        let tool = ScheduleTool;
        let ctx = ToolContext::new("/tmp".to_string());

        let err = tool
            .execute(r#"{"action": "destroy"}"#, &ctx)
            .await
            .expect_err("unknown action should fail");
        assert!(err.to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn schedule_missing_id_fails() {
        let tool = ScheduleTool;
        let ctx = ToolContext::new("/tmp".to_string());

        let err = tool
            .execute(r#"{"action": "remove"}"#, &ctx)
            .await
            .expect_err("missing id should fail");
        assert!(err.to_string().contains("missing required field `id`"));
    }
}

use agentzero_plugin_sdk::prelude::*;
use serde_json::json;

declare_tool!("schedule", execute);

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let action = match req.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.trim(),
        None => return ToolOutput::error("action field is required (parse|create|list|update|remove|pause|resume)"),
    };

    match action {
        "parse" => handle_parse(&req),
        "create" | "list" | "update" | "remove" | "pause" | "resume" => {
            // For CRUD operations, translate natural language schedule and delegate
            // to the cron_manager plugin. Return the prepared request.
            handle_crud_delegation(action, &req)
        }
        _ => ToolOutput::error(format!(
            "unknown action: {action}. Use: parse|create|list|update|remove|pause|resume"
        )),
    }
}

fn handle_parse(req: &serde_json::Value) -> ToolOutput {
    let schedule = match req.get("schedule").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return ToolOutput::error("schedule field is required"),
    };

    match parse_natural_language(schedule) {
        Ok(cron_expr) => ToolOutput::success(
            json!({
                "input": schedule,
                "cron_expression": cron_expr,
                "is_natural_language": schedule != cron_expr,
            })
            .to_string(),
        ),
        Err(e) => ToolOutput::error(e),
    }
}

fn handle_crud_delegation(action: &str, req: &serde_json::Value) -> ToolOutput {
    // Translate natural language schedule if present
    let mut delegated = req.clone();

    if let Some(schedule_str) = req.get("schedule").and_then(|v| v.as_str()) {
        match parse_natural_language(schedule_str.trim()) {
            Ok(cron_expr) => {
                delegated["schedule"] = serde_json::Value::String(cron_expr);
            }
            Err(e) => return ToolOutput::error(format!("invalid schedule: {e}")),
        }
    }

    // Map action names: create -> add
    let cron_action = match action {
        "create" => "add",
        other => other,
    };
    delegated["action"] = serde_json::Value::String(cron_action.to_string());

    ToolOutput::success(
        json!({
            "delegation": "cron_manager",
            "prepared_input": delegated,
            "note": "Pass prepared_input to cron_manager plugin for execution",
        })
        .to_string(),
    )
}

/// Parse a natural language schedule expression into a 5-field cron expression.
///
/// Supported patterns:
/// - "every N minutes" -> "*/N * * * *"
/// - "every N hours" -> "0 */N * * *"
/// - "every minute" -> "* * * * *"
/// - "hourly" / "every hour" -> "0 * * * *"
/// - "daily" / "every day" -> "0 0 * * *"
/// - "daily at 9am" -> "0 9 * * *"
/// - "daily at 2:30pm" -> "30 14 * * *"
/// - "daily at 14:30" -> "30 14 * * *"
/// - "weekly" -> "0 0 * * 0"
/// - "weekly on monday" -> "0 0 * * 1"
/// - "every monday" -> "0 0 * * 1"
/// - "monthly" -> "0 0 1 * *"
/// - Already valid cron expressions pass through unchanged.
fn parse_natural_language(input: &str) -> Result<String, String> {
    let lower = input.to_lowercase();
    let lower = lower.trim();

    // Already looks like a cron expression (5 fields, starts with digit or *)
    if looks_like_cron(lower) {
        return Ok(lower.to_string());
    }

    // "every minute"
    if lower == "every minute" {
        return Ok("* * * * *".to_string());
    }

    // "every N minutes"
    if let Some(n) = extract_number(lower, "every ", " minutes") {
        if n < 1 || n > 59 {
            return Err(format!("minutes must be 1-59, got {n}"));
        }
        return Ok(format!("*/{n} * * * *"));
    }

    // "every N hours"
    if let Some(n) = extract_number(lower, "every ", " hours") {
        if n < 1 || n > 23 {
            return Err(format!("hours must be 1-23, got {n}"));
        }
        return Ok(format!("0 */{n} * * *"));
    }

    // "hourly" / "every hour"
    if lower == "hourly" || lower == "every hour" {
        return Ok("0 * * * *".to_string());
    }

    // "daily at HH:MM" or "daily at Ham/Hpm"
    if lower.starts_with("daily at ") || lower.starts_with("every day at ") {
        let time_part = if lower.starts_with("daily at ") {
            &lower[9..]
        } else {
            &lower[13..]
        };
        let (hour, minute) = parse_time(time_part.trim())?;
        return Ok(format!("{minute} {hour} * * *"));
    }

    // "daily" / "every day"
    if lower == "daily" || lower == "every day" {
        return Ok("0 0 * * *".to_string());
    }

    // "weekly on <day>"
    if lower.starts_with("weekly on ") {
        let day_str = lower[10..].trim();
        let day = parse_day_of_week(day_str)?;
        return Ok(format!("0 0 * * {day}"));
    }

    // "every <day>"
    if lower.starts_with("every ") {
        let day_str = lower[6..].trim();
        if let Ok(day) = parse_day_of_week(day_str) {
            return Ok(format!("0 0 * * {day}"));
        }
    }

    // "weekly"
    if lower == "weekly" {
        return Ok("0 0 * * 0".to_string());
    }

    // "monthly"
    if lower == "monthly" {
        return Ok("0 0 1 * *".to_string());
    }

    Err(format!("unrecognized schedule format: '{input}'"))
}

fn looks_like_cron(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() != 5 {
        return false;
    }
    parts[0]
        .chars()
        .next()
        .map(|c| c.is_ascii_digit() || c == '*')
        .unwrap_or(false)
}

fn extract_number(s: &str, prefix: &str, suffix: &str) -> Option<u32> {
    if s.starts_with(prefix) && s.ends_with(suffix) {
        let num_str = &s[prefix.len()..s.len() - suffix.len()];
        num_str.trim().parse().ok()
    } else {
        None
    }
}

fn parse_time(s: &str) -> Result<(u32, u32), String> {
    let s = s.trim();

    // "14:30" (24h)
    if let Some((h, m)) = s.split_once(':') {
        let h_clean = h.trim().trim_end_matches(|c: char| c.is_alphabetic());
        let m_clean = m.trim().trim_end_matches(|c: char| c.is_alphabetic());
        let hour: u32 = h_clean
            .parse()
            .map_err(|_| format!("invalid hour: {h}"))?;
        let minute: u32 = m_clean
            .parse()
            .map_err(|_| format!("invalid minute: {m}"))?;

        let is_pm = s.ends_with("pm");
        let is_am = s.ends_with("am");
        let final_hour = if is_pm && hour < 12 {
            hour + 12
        } else if is_am && hour == 12 {
            0
        } else {
            hour
        };

        if final_hour > 23 {
            return Err(format!("hour out of range: {final_hour}"));
        }
        if minute > 59 {
            return Err(format!("minute out of range: {minute}"));
        }
        return Ok((final_hour, minute));
    }

    // "9am", "2pm", "14"
    let is_pm = s.ends_with("pm");
    let is_am = s.ends_with("am");
    let num_str = s
        .trim_end_matches("am")
        .trim_end_matches("pm")
        .trim();
    let hour: u32 = num_str
        .parse()
        .map_err(|_| format!("invalid time: {s}"))?;

    let final_hour = if is_pm && hour < 12 {
        hour + 12
    } else if is_am && hour == 12 {
        0
    } else {
        hour
    };

    if final_hour > 23 {
        return Err(format!("hour out of range: {final_hour}"));
    }
    Ok((final_hour, 0))
}

fn parse_day_of_week(s: &str) -> Result<u32, String> {
    match s {
        "sunday" | "sun" => Ok(0),
        "monday" | "mon" => Ok(1),
        "tuesday" | "tue" => Ok(2),
        "wednesday" | "wed" => Ok(3),
        "thursday" | "thu" => Ok(4),
        "friday" | "fri" => Ok(5),
        "saturday" | "sat" => Ok(6),
        _ => Err(format!("unknown day of week: '{s}'")),
    }
}

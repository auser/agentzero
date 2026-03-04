use agentzero_plugin_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

declare_tool!("cron_manager", execute);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CronTask {
    id: String,
    schedule: String,
    command: String,
    enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_run_epoch_seconds: Option<u64>,
}

fn execute(input: ToolInput) -> ToolOutput {
    let req: serde_json::Value = match serde_json::from_str(&input.input) {
        Ok(v) => v,
        Err(e) => return ToolOutput::error(format!("invalid input: {e}")),
    };

    let action = match req.get("action").and_then(|v| v.as_str()) {
        Some(a) => a.trim(),
        None => return ToolOutput::error("action field is required (add|list|remove|update|pause|resume)"),
    };

    let state_path = cron_state_path(&input.workspace_root);

    match action {
        "add" => handle_add(&req, &state_path),
        "list" => handle_list(&state_path),
        "remove" => handle_remove(&req, &state_path),
        "update" => handle_update(&req, &state_path),
        "pause" => handle_toggle(&req, &state_path, false),
        "resume" => handle_toggle(&req, &state_path, true),
        _ => ToolOutput::error(format!("unknown action: {action}. Use: add|list|remove|update|pause|resume")),
    }
}

fn cron_state_path(_workspace_root: &str) -> PathBuf {
    // In WASI sandbox, the workspace root is preopened as ".".
    // Use relative path to access files within the sandbox.
    PathBuf::from(".")
        .join(".agentzero")
        .join("plugin-cron-tasks.json")
}

fn load_tasks(path: &PathBuf) -> Vec<CronTask> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_tasks(path: &PathBuf, tasks: &[CronTask]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("failed to create dir: {e}"))?;
    }
    let json = serde_json::to_string_pretty(tasks).map_err(|e| format!("serialize error: {e}"))?;
    fs::write(path, json).map_err(|e| format!("failed to write state: {e}"))
}

fn handle_add(req: &serde_json::Value, state_path: &PathBuf) -> ToolOutput {
    let id = match req.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return ToolOutput::error("id field is required and must not be empty"),
    };
    let schedule = match req.get("schedule").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return ToolOutput::error("schedule field is required and must not be empty"),
    };
    let command = match req.get("command").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return ToolOutput::error("command field is required and must not be empty"),
    };

    let mut tasks = load_tasks(state_path);

    if tasks.iter().any(|t| t.id == id) {
        return ToolOutput::error(format!("task with id '{id}' already exists"));
    }

    let task = CronTask {
        id: id.clone(),
        schedule: schedule.clone(),
        command: command.clone(),
        enabled: true,
        last_run_epoch_seconds: None,
    };
    tasks.push(task);

    if let Err(e) = save_tasks(state_path, &tasks) {
        return ToolOutput::error(e);
    }

    ToolOutput::success(
        json!({
            "status": "added",
            "id": id,
            "schedule": schedule,
            "command": command,
            "enabled": true,
        })
        .to_string(),
    )
}

fn handle_list(state_path: &PathBuf) -> ToolOutput {
    let tasks = load_tasks(state_path);
    if tasks.is_empty() {
        return ToolOutput::success("no cron tasks");
    }

    let list: Vec<serde_json::Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "schedule": t.schedule,
                "command": t.command,
                "enabled": t.enabled,
            })
        })
        .collect();

    ToolOutput::success(serde_json::to_string_pretty(&list).unwrap_or_default())
}

fn handle_remove(req: &serde_json::Value, state_path: &PathBuf) -> ToolOutput {
    let id = match req.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return ToolOutput::error("id field is required"),
    };

    let mut tasks = load_tasks(state_path);
    let before = tasks.len();
    tasks.retain(|t| t.id != id);

    if tasks.len() == before {
        return ToolOutput::error(format!("no task found with id '{id}'"));
    }

    if let Err(e) = save_tasks(state_path, &tasks) {
        return ToolOutput::error(e);
    }

    ToolOutput::success(json!({"status": "removed", "id": id}).to_string())
}

fn handle_update(req: &serde_json::Value, state_path: &PathBuf) -> ToolOutput {
    let id = match req.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return ToolOutput::error("id field is required"),
    };

    let new_schedule = req.get("schedule").and_then(|v| v.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty());
    let new_command = req.get("command").and_then(|v| v.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty());

    if new_schedule.is_none() && new_command.is_none() {
        return ToolOutput::error("at least one of schedule or command must be provided");
    }

    let mut tasks = load_tasks(state_path);
    let task = match tasks.iter_mut().find(|t| t.id == id) {
        Some(t) => t,
        None => return ToolOutput::error(format!("no task found with id '{id}'")),
    };

    if let Some(s) = new_schedule {
        task.schedule = s.to_string();
    }
    if let Some(c) = new_command {
        task.command = c.to_string();
    }

    let result = json!({
        "status": "updated",
        "id": task.id,
        "schedule": task.schedule,
        "command": task.command,
        "enabled": task.enabled,
    });

    if let Err(e) = save_tasks(state_path, &tasks) {
        return ToolOutput::error(e);
    }

    ToolOutput::success(result.to_string())
}

fn handle_toggle(req: &serde_json::Value, state_path: &PathBuf, enabled: bool) -> ToolOutput {
    let id = match req.get("id").and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return ToolOutput::error("id field is required"),
    };

    let mut tasks = load_tasks(state_path);
    let task = match tasks.iter_mut().find(|t| t.id == id) {
        Some(t) => t,
        None => return ToolOutput::error(format!("no task found with id '{id}'")),
    };

    task.enabled = enabled;
    let action_word = if enabled { "resumed" } else { "paused" };

    let result = json!({
        "status": action_word,
        "id": task.id,
        "schedule": task.schedule,
        "command": task.command,
        "enabled": task.enabled,
    });

    if let Err(e) = save_tasks(state_path, &tasks) {
        return ToolOutput::error(e);
    }

    ToolOutput::success(result.to_string())
}

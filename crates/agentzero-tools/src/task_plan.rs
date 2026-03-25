use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaskItem {
    id: usize,
    title: String,
    status: TaskStatus,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action")]
#[serde(rename_all = "snake_case")]
enum TaskAction {
    Create { tasks: Vec<TaskCreate> },
    Add { title: String },
    Update { id: usize, status: TaskStatus },
    List,
    Delete,
}

#[derive(Debug, Deserialize)]
struct TaskCreate {
    title: String,
    #[serde(default = "default_pending")]
    status: TaskStatus,
}

fn default_pending() -> TaskStatus {
    TaskStatus::Pending
}

#[tool(
    name = "task_plan",
    description = "Manage a structured task plan: create, list, update status, or clear tasks for tracking multi-step work."
)]
pub struct TaskPlanTool {
    tasks: Mutex<Vec<TaskItem>>,
}

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct TaskPlanSchema {
    /// The task plan action to perform
    #[schema(enum_values = ["create", "add", "update", "list", "delete"])]
    action: String,
    /// Tasks to create (for create action)
    #[serde(default)]
    tasks: Option<Vec<serde_json::Value>>,
    /// Task title (for add action)
    #[serde(default)]
    title: Option<String>,
    /// Task ID (for update action)
    #[serde(default)]
    id: Option<i64>,
    /// New status (for update action)
    #[serde(default)]
    #[schema(enum_values = ["pending", "in_progress", "completed"])]
    status: Option<String>,
}

impl Default for TaskPlanTool {
    fn default() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
        }
    }
}

impl TaskPlanTool {
    fn persist(&self, workspace_root: &str) -> anyhow::Result<()> {
        let tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
        let dir = PathBuf::from(workspace_root).join(".agentzero");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("task-plan.json");
        let json = serde_json::to_string_pretty(&*tasks)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    fn load(&self, workspace_root: &str) {
        let path = PathBuf::from(workspace_root)
            .join(".agentzero")
            .join("task-plan.json");
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(items) = serde_json::from_str::<Vec<TaskItem>>(&content) {
                if let Ok(mut tasks) = self.tasks.lock() {
                    if tasks.is_empty() {
                        *tasks = items;
                    }
                }
            }
        }
    }

    fn format_tasks(tasks: &[TaskItem]) -> String {
        if tasks.is_empty() {
            return "no tasks".to_string();
        }
        tasks
            .iter()
            .map(|t| {
                let marker = match t.status {
                    TaskStatus::Pending => "[ ]",
                    TaskStatus::InProgress => "[~]",
                    TaskStatus::Completed => "[x]",
                };
                format!("{} {}. {}", marker, t.id, t.title)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[async_trait]
impl Tool for TaskPlanTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(TaskPlanSchema::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let action: TaskAction =
            serde_json::from_str(input).context("task_plan expects JSON with \"action\" field")?;

        self.load(&ctx.workspace_root);

        match action {
            TaskAction::Create { tasks: new_tasks } => {
                let mut tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                tasks.clear();
                for (i, tc) in new_tasks.into_iter().enumerate() {
                    tasks.push(TaskItem {
                        id: i + 1,
                        title: tc.title,
                        status: tc.status,
                    });
                }
                let output = Self::format_tasks(&tasks);
                drop(tasks);
                self.persist(&ctx.workspace_root)?;
                Ok(ToolResult {
                    output: format!("plan created:\n{output}"),
                })
            }

            TaskAction::Add { title } => {
                if title.trim().is_empty() {
                    return Err(anyhow!("title must not be empty"));
                }
                let mut tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                let id = tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                tasks.push(TaskItem {
                    id,
                    title: title.clone(),
                    status: TaskStatus::Pending,
                });
                drop(tasks);
                self.persist(&ctx.workspace_root)?;
                Ok(ToolResult {
                    output: format!("added task {id}: {title}"),
                })
            }

            TaskAction::Update { id, status } => {
                let mut tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                let task = tasks
                    .iter_mut()
                    .find(|t| t.id == id)
                    .ok_or_else(|| anyhow!("task {id} not found"))?;
                task.status = status;
                let output = format!("updated task {}: {}", task.id, task.title);
                drop(tasks);
                self.persist(&ctx.workspace_root)?;
                Ok(ToolResult { output })
            }

            TaskAction::List => {
                let tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                Ok(ToolResult {
                    output: Self::format_tasks(&tasks),
                })
            }

            TaskAction::Delete => {
                let mut tasks = self.tasks.lock().map_err(|_| anyhow!("lock poisoned"))?;
                tasks.clear();
                drop(tasks);
                self.persist(&ctx.workspace_root)?;
                Ok(ToolResult {
                    output: "plan deleted".to_string(),
                })
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
            "agentzero-task-plan-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn task_plan_create_list_update() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = tool
            .execute(
                r#"{"action": "create", "tasks": [{"title": "Step 1"}, {"title": "Step 2"}]}"#,
                &ctx,
            )
            .await
            .expect("create should succeed");
        assert!(result.output.contains("Step 1"));
        assert!(result.output.contains("Step 2"));

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("[ ] 1. Step 1"));

        let result = tool
            .execute(
                r#"{"action": "update", "id": 1, "status": "completed"}"#,
                &ctx,
            )
            .await
            .expect("update should succeed");
        assert!(result.output.contains("updated task 1"));

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("[x] 1. Step 1"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn task_plan_add_and_delete() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        tool.execute(r#"{"action": "add", "title": "New task"}"#, &ctx)
            .await
            .expect("add should succeed");

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("New task"));

        tool.execute(r#"{"action": "delete"}"#, &ctx)
            .await
            .expect("delete should succeed");

        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("no tasks"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn task_plan_update_nonexistent_fails() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = tool
            .execute(
                r#"{"action": "update", "id": 99, "status": "completed"}"#,
                &ctx,
            )
            .await
            .expect_err("update nonexistent should fail");
        assert!(err.to_string().contains("not found"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn task_plan_empty_create_succeeds() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let result = tool
            .execute(r#"{"action": "create", "tasks": []}"#, &ctx)
            .await
            .expect("create with empty tasks should succeed");
        // Should acknowledge creation even if no tasks.
        assert!(!result.output.is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn task_plan_status_transitions() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        tool.execute(
            r#"{"action": "create", "tasks": [{"title": "My task"}]}"#,
            &ctx,
        )
        .await
        .expect("create");

        // pending → in_progress
        tool.execute(
            r#"{"action": "update", "id": 1, "status": "in_progress"}"#,
            &ctx,
        )
        .await
        .expect("pending to in_progress");
        let list = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list");
        assert!(list.output.contains("[~] 1. My task"));

        // in_progress → completed
        tool.execute(
            r#"{"action": "update", "id": 1, "status": "completed"}"#,
            &ctx,
        )
        .await
        .expect("in_progress to completed");
        let list = tool
            .execute(r#"{"action": "list"}"#, &ctx)
            .await
            .expect("list");
        assert!(list.output.contains("[x] 1. My task"));
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn task_plan_invalid_input_returns_error() {
        let dir = temp_dir();
        let tool = TaskPlanTool::default();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());

        let err = tool
            .execute("not json at all", &ctx)
            .await
            .expect_err("invalid JSON should fail");
        assert!(!err.to_string().is_empty());
        fs::remove_dir_all(dir).ok();
    }
}

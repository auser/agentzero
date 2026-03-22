//! Background task manager for async delegation.
//!
//! Tracks background sub-agent tasks, provides lifecycle management
//! (check, list, cancel), and persists results to workspace directory.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Status of a background task.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum TaskStatus {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "completed")]
    Completed { output: String },
    #[serde(rename = "failed")]
    Failed { error: String },
    #[serde(rename = "cancelled")]
    Cancelled,
}

struct BackgroundTask {
    cancel_token: CancellationToken,
    #[allow(dead_code)]
    handle: JoinHandle<()>,
    status: Arc<Mutex<TaskStatus>>,
    agent_name: String,
    created_at: u64,
}

/// Manages background delegation tasks.
#[derive(Clone)]
pub struct TaskManager {
    tasks: Arc<Mutex<HashMap<String, BackgroundTask>>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Generate a unique task ID.
    fn generate_id() -> String {
        format!(
            "task-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        )
    }

    /// Spawn a background task. Returns the task ID immediately.
    pub async fn spawn_background<F>(&self, agent_name: String, future: F) -> String
    where
        F: std::future::Future<Output = anyhow::Result<String>> + Send + 'static,
    {
        let task_id = Self::generate_id();
        let cancel_token = CancellationToken::new();
        let status = Arc::new(Mutex::new(TaskStatus::Running));

        let status_clone = status.clone();
        let cancel_clone = cancel_token.clone();

        let handle = tokio::spawn(async move {
            tokio::select! {
                result = future => {
                    let mut s = status_clone.lock().await;
                    match result {
                        Ok(output) => *s = TaskStatus::Completed { output },
                        Err(e) => *s = TaskStatus::Failed { error: e.to_string() },
                    }
                }
                _ = cancel_clone.cancelled() => {
                    let mut s = status_clone.lock().await;
                    *s = TaskStatus::Cancelled;
                }
            }
        });

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let task = BackgroundTask {
            cancel_token,
            handle,
            status,
            agent_name,
            created_at: now,
        };

        self.tasks.lock().await.insert(task_id.clone(), task);
        task_id
    }

    /// Check the status of a background task.
    pub async fn check_result(&self, task_id: &str) -> Option<TaskStatus> {
        let tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get(task_id) {
            Some(task.status.lock().await.clone())
        } else {
            None
        }
    }

    /// List all tasks with their statuses.
    ///
    /// Returns tuples of `(task_id, agent_name, status, created_at)`.
    pub async fn list_results(&self) -> Vec<(String, String, TaskStatus, u64)> {
        let tasks = self.tasks.lock().await;
        let mut results = Vec::new();
        for (id, task) in tasks.iter() {
            let status = task.status.lock().await.clone();
            results.push((id.clone(), task.agent_name.clone(), status, task.created_at));
        }
        results
    }

    /// Cancel a specific background task.
    pub async fn cancel_task(&self, task_id: &str) -> bool {
        let tasks = self.tasks.lock().await;
        if let Some(task) = tasks.get(task_id) {
            task.cancel_token.cancel();
            true
        } else {
            false
        }
    }

    /// Cancel all background tasks (called on session teardown).
    pub async fn cancel_all(&self) {
        let tasks = self.tasks.lock().await;
        for task in tasks.values() {
            task.cancel_token.cancel();
        }
    }
}

impl Default for TaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_and_check_completed() {
        let tm = TaskManager::new();
        let task_id = tm
            .spawn_background("test-agent".to_string(), async { Ok("done".to_string()) })
            .await;

        // Give the spawned task time to complete.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let status = tm.check_result(&task_id).await.expect("task should exist");
        match status {
            TaskStatus::Completed { output } => assert_eq!(output, "done"),
            other => panic!("expected Completed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancel_task_sets_cancelled() {
        let tm = TaskManager::new();
        let task_id = tm
            .spawn_background("slow-agent".to_string(), async {
                // Simulate a long-running task.
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok("should not reach".to_string())
            })
            .await;

        // Cancel immediately.
        assert!(tm.cancel_task(&task_id).await);

        // Give the spawned task time to notice cancellation.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let status = tm.check_result(&task_id).await.expect("task should exist");
        assert!(
            matches!(status, TaskStatus::Cancelled),
            "expected Cancelled, got {status:?}"
        );
    }

    #[tokio::test]
    async fn list_results_shows_all() {
        let tm = TaskManager::new();
        let _id1 = tm
            .spawn_background("agent-a".to_string(), async { Ok("a".to_string()) })
            .await;
        let _id2 = tm
            .spawn_background("agent-b".to_string(), async { Ok("b".to_string()) })
            .await;

        // Wait for both to complete.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let results = tm.list_results().await;
        assert_eq!(results.len(), 2);

        let names: Vec<&str> = results
            .iter()
            .map(|(_, name, _, _)| name.as_str())
            .collect();
        assert!(names.contains(&"agent-a"));
        assert!(names.contains(&"agent-b"));
    }

    #[tokio::test]
    async fn check_nonexistent_returns_none() {
        let tm = TaskManager::new();
        assert!(tm.check_result("no-such-task").await.is_none());
    }

    #[tokio::test]
    async fn cancel_all_cancels_everything() {
        let tm = TaskManager::new();
        let id1 = tm
            .spawn_background("a".to_string(), async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok("nope".to_string())
            })
            .await;
        let id2 = tm
            .spawn_background("b".to_string(), async {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                Ok("nope".to_string())
            })
            .await;

        tm.cancel_all().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        for id in [&id1, &id2] {
            let status = tm.check_result(id).await.expect("task should exist");
            assert!(
                matches!(status, TaskStatus::Cancelled),
                "task {id} should be Cancelled, got {status:?}"
            );
        }
    }
}

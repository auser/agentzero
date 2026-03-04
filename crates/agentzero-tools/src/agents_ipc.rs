use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const IPC_STORE_FILE: &str = "ipc.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct IpcMessage {
    from: String,
    to: String,
    payload: String,
    created_at_epoch_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum IpcRequest {
    Send {
        from: String,
        to: String,
        payload: String,
    },
    Recv {
        to: String,
    },
    List {
        to: Option<String>,
        from: Option<String>,
        limit: Option<usize>,
    },
    Clear {
        to: Option<String>,
        from: Option<String>,
    },
}

pub struct AgentsIpcTool;

#[async_trait]
impl Tool for AgentsIpcTool {
    fn name(&self) -> &'static str {
        "agents_ipc"
    }

    fn description(&self) -> &'static str {
        "Inter-process communication between agents: send messages and receive responses."
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: IpcRequest =
            serde_json::from_str(input).context("agents_ipc input must be valid JSON request")?;
        let ipc_dir = ipc_dir(&ctx.workspace_root);
        let store = EncryptedJsonStore::in_config_dir(&ipc_dir, IPC_STORE_FILE)?;
        let mut messages: Vec<IpcMessage> = store.load_or_default()?;

        let output = match req {
            IpcRequest::Send { from, to, payload } => {
                if from.trim().is_empty() || to.trim().is_empty() {
                    return Err(anyhow!("`from` and `to` must not be empty"));
                }
                messages.push(IpcMessage {
                    from,
                    to,
                    payload,
                    created_at_epoch_secs: now_epoch_secs(),
                });
                store.save(&messages)?;
                json!({
                    "queued": messages.len(),
                    "status": "ok"
                })
            }
            IpcRequest::Recv { to } => {
                if to.trim().is_empty() {
                    return Err(anyhow!("`to` must not be empty"));
                }
                let idx = messages.iter().position(|msg| msg.to == to);
                let received = idx.map(|index| messages.remove(index));
                store.save(&messages)?;
                json!({
                    "message": received,
                    "remaining": messages.len()
                })
            }
            IpcRequest::List { to, from, limit } => {
                let iter = messages.iter().filter(|msg| {
                    to.as_ref()
                        .map(|expected| &msg.to == expected)
                        .unwrap_or(true)
                        && from
                            .as_ref()
                            .map(|expected| &msg.from == expected)
                            .unwrap_or(true)
                });
                let listed = if let Some(limit) = limit {
                    iter.take(limit).cloned().collect::<Vec<_>>()
                } else {
                    iter.cloned().collect::<Vec<_>>()
                };
                json!({
                    "messages": listed,
                    "count": listed.len()
                })
            }
            IpcRequest::Clear { to, from } => {
                let before = messages.len();
                messages.retain(|msg| {
                    let to_match = to
                        .as_ref()
                        .map(|expected| &msg.to == expected)
                        .unwrap_or(true);
                    let from_match = from
                        .as_ref()
                        .map(|expected| &msg.from == expected)
                        .unwrap_or(true);
                    !(to_match && from_match)
                });
                let removed = before.saturating_sub(messages.len());
                store.save(&messages)?;
                json!({
                    "removed": removed,
                    "remaining": messages.len()
                })
            }
        };

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)?,
        })
    }
}

fn ipc_dir(workspace_root: &str) -> PathBuf {
    Path::new(workspace_root).join(".agentzero")
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be after epoch")
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::AgentsIpcTool;
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-ipc-tool-test-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[tokio::test]
    async fn agents_ipc_send_and_recv_success_path() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = AgentsIpcTool;

        tool.execute(
            r#"{"op":"send","from":"planner","to":"worker","payload":"do task"}"#,
            &ctx,
        )
        .await
        .expect("send should succeed");

        let recv = tool
            .execute(r#"{"op":"recv","to":"worker"}"#, &ctx)
            .await
            .expect("recv should succeed");
        assert!(recv.output.contains("\"payload\": \"do task\""));

        // Verify stored data is encrypted (not readable as plain JSON)
        let ipc_file = dir.join(".agentzero").join("ipc.json");
        if ipc_file.exists() {
            let raw = fs::read_to_string(&ipc_file).unwrap_or_default();
            assert!(
                !raw.contains("\"planner\""),
                "IPC store should be encrypted, not plaintext"
            );
        }

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn agents_ipc_rejects_invalid_json_negative_path() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = AgentsIpcTool;

        let err = tool
            .execute("not-json", &ctx)
            .await
            .expect_err("invalid json should fail");
        assert!(err.to_string().contains("valid JSON"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}

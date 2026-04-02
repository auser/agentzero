use agentzero_core::event_bus::Event;
use agentzero_core::{Tool, ToolContext, ToolResult};
use agentzero_macros::{tool, ToolSchema};
use agentzero_storage::EncryptedJsonStore;
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const IPC_STORE_FILE: &str = "ipc.json";
/// Timeout for recv when using the event bus (seconds).
const BUS_RECV_TIMEOUT_SECS: u64 = 30;

#[derive(ToolSchema, Deserialize)]
#[allow(dead_code)]
struct AgentsIpcInput {
    /// The IPC operation to perform
    #[schema(enum_values = ["send", "recv", "list", "clear"])]
    op: String,
    /// Sender agent name (required for send)
    #[serde(default)]
    from: Option<String>,
    /// Recipient agent name (required for send/recv)
    #[serde(default)]
    to: Option<String>,
    /// Message payload (required for send)
    #[serde(default)]
    payload: Option<String>,
    /// Max messages to return (for list)
    #[serde(default)]
    limit: Option<usize>,
}

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

#[tool(
    name = "agents_ipc",
    description = "Inter-process communication between agents: send messages and receive responses. When an event bus is available, messages are published as events. Otherwise falls back to file-based storage."
)]
pub struct AgentsIpcTool;

#[async_trait]
impl Tool for AgentsIpcTool {
    fn name(&self) -> &'static str {
        Self::tool_name()
    }

    fn description(&self) -> &'static str {
        Self::tool_description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(AgentsIpcInput::schema())
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let req: IpcRequest =
            serde_json::from_str(input).context("agents_ipc input must be valid JSON request")?;

        if ctx.event_bus.is_some() {
            execute_bus(req, ctx).await
        } else {
            execute_file(req, ctx).await
        }
    }
}

/// Bus-based IPC: publish/subscribe events on the event bus.
async fn execute_bus(req: IpcRequest, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
    let bus = ctx.event_bus.as_ref().unwrap();

    let output = match req {
        IpcRequest::Send { from, to, payload } => {
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(anyhow!("`from` and `to` must not be empty"));
            }
            let event = Event::new(format!("ipc.message.{to}"), &from, &payload)
                .with_boundary(&ctx.privacy_boundary);
            bus.publish(event).await?;
            json!({ "status": "published", "topic": format!("ipc.message.{to}") })
        }
        IpcRequest::Recv { to } => {
            if to.trim().is_empty() {
                return Err(anyhow!("`to` must not be empty"));
            }
            let mut sub = bus.subscribe();
            let filter = agentzero_core::event_bus::EventFilter::topic(format!("ipc.message.{to}"));
            match tokio::time::timeout(
                Duration::from_secs(BUS_RECV_TIMEOUT_SECS),
                sub.recv_with_filter(&filter),
            )
            .await
            {
                Ok(Ok(event)) => {
                    json!({
                        "message": {
                            "from": event.source,
                            "to": to,
                            "payload": event.payload,
                        },
                        "source": "event_bus"
                    })
                }
                Ok(Err(e)) => {
                    json!({ "message": null, "error": e.to_string() })
                }
                Err(_) => {
                    json!({ "message": null, "timeout": true })
                }
            }
        }
        IpcRequest::List { .. } => {
            json!({
                "messages": [],
                "count": 0,
                "note": "list is not supported with event bus (events are transient)"
            })
        }
        IpcRequest::Clear { .. } => {
            json!({
                "removed": 0,
                "note": "clear is a no-op with event bus (events are transient)"
            })
        }
    };

    Ok(ToolResult {
        output: serde_json::to_string_pretty(&output)?,
    })
}

/// File-based IPC: legacy fallback using EncryptedJsonStore.
async fn execute_file(req: IpcRequest, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
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
    use agentzero_core::event_bus::InMemoryBus;
    use agentzero_core::{Tool, ToolContext};
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

    fn ctx_with_bus(dir: &std::path::Path) -> ToolContext {
        let bus = Arc::new(InMemoryBus::new(64));
        let mut ctx = ToolContext::new(dir.to_string_lossy().to_string());
        ctx.event_bus = Some(bus);
        ctx.agent_id = Some("test-agent".to_string());
        ctx
    }

    // --- File-based IPC tests (backward compatibility) ---

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

        let _ = fs::remove_dir_all(dir);
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

        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn agents_ipc_recv_missing_returns_no_messages() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = AgentsIpcTool;

        let result = tool
            .execute(r#"{"op":"recv","to":"nobody"}"#, &ctx)
            .await
            .expect("recv for empty mailbox should succeed");
        assert!(
            result.output.contains("\"message\": null")
                || result.output.contains("\"remaining\": 0"),
            "should indicate no messages, got: {}",
            result.output
        );
        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn agents_ipc_message_round_trip() {
        let dir = temp_dir();
        let ctx = ToolContext::new(dir.to_string_lossy().to_string());
        let tool = AgentsIpcTool;

        tool.execute(
            r#"{"op":"send","from":"alice","to":"bob","payload":"msg-1"}"#,
            &ctx,
        )
        .await
        .expect("send 1");
        tool.execute(
            r#"{"op":"send","from":"alice","to":"bob","payload":"msg-2"}"#,
            &ctx,
        )
        .await
        .expect("send 2");

        let list = tool
            .execute(r#"{"op":"list","to":"bob"}"#, &ctx)
            .await
            .expect("list");
        assert!(list.output.contains("msg-1"), "list should contain msg-1");
        assert!(list.output.contains("msg-2"), "list should contain msg-2");

        fs::remove_dir_all(dir).ok();
    }

    // --- Event bus IPC tests ---

    #[tokio::test]
    async fn bus_ipc_send_publishes_event() {
        let dir = temp_dir();
        let ctx = ctx_with_bus(&dir);
        let tool = AgentsIpcTool;

        // Subscribe before sending so we can verify the event
        let mut sub = ctx.event_bus.as_ref().unwrap().subscribe();

        let result = tool
            .execute(
                r#"{"op":"send","from":"planner","to":"worker","payload":"do task"}"#,
                &ctx,
            )
            .await
            .expect("send should succeed");
        assert!(result.output.contains("published"));
        assert!(result.output.contains("ipc.message.worker"));

        // Verify the event was published on the bus
        let event = sub.recv().await.expect("should receive event");
        assert_eq!(event.topic, "ipc.message.worker");
        assert_eq!(event.source, "planner");
        assert_eq!(&*event.payload, "do task");

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn bus_ipc_recv_gets_message() {
        let dir = temp_dir();
        let ctx = ctx_with_bus(&dir);
        let tool = AgentsIpcTool;
        let bus = ctx.event_bus.as_ref().unwrap().clone();

        // Spawn a task that sends a message after a small delay
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            bus.publish(agentzero_core::event_bus::Event::new(
                "ipc.message.worker",
                "planner",
                "hello worker",
            ))
            .await
            .unwrap();
        });

        let result = tool
            .execute(r#"{"op":"recv","to":"worker"}"#, &ctx)
            .await
            .expect("recv should succeed");
        assert!(result.output.contains("hello worker"));
        assert!(result.output.contains("event_bus"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn bus_ipc_list_returns_transient_note() {
        let dir = temp_dir();
        let ctx = ctx_with_bus(&dir);
        let tool = AgentsIpcTool;

        let result = tool
            .execute(r#"{"op":"list","to":"bob"}"#, &ctx)
            .await
            .expect("list should succeed");
        assert!(result.output.contains("transient"));

        fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn bus_ipc_clear_is_noop() {
        let dir = temp_dir();
        let ctx = ctx_with_bus(&dir);
        let tool = AgentsIpcTool;

        let result = tool
            .execute(r#"{"op":"clear","to":"bob"}"#, &ctx)
            .await
            .expect("clear should succeed");
        assert!(result.output.contains("no-op"));

        fs::remove_dir_all(dir).ok();
    }
}

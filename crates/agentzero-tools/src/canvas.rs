//! Canvas tool — pushes rich visual content to a live canvas viewable in the web UI.
//!
//! Supports rendering HTML, SVG, Markdown, and plain text to named canvases.
//! The gateway exposes REST and WebSocket endpoints for real-time consumption.

use agentzero_core::canvas::CanvasStore;
use agentzero_core::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    canvas_id: Option<String>,
    content_type: Option<String>,
    content: Option<String>,
}

/// Tool that pushes rich visual content to a live canvas in the web UI.
///
/// Actions:
/// - `render` — push content (HTML, SVG, Markdown, plain text) to a canvas
/// - `snapshot` — read the current state of a canvas
/// - `clear` — reset a canvas
/// - `list` — list all active canvases
pub struct CanvasTool {
    store: Arc<CanvasStore>,
}

impl CanvasTool {
    pub fn new(store: Arc<CanvasStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for CanvasTool {
    fn name(&self) -> &'static str {
        "canvas"
    }

    fn description(&self) -> &'static str {
        "Push rich visual content (HTML, SVG, Markdown) to a live canvas viewable in the web UI. \
         Use 'render' to push content, 'snapshot' to read current state, 'clear' to reset, \
         'list' to see all canvases."
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        Some(json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["render", "snapshot", "clear", "list"],
                    "description": "The canvas action to perform"
                },
                "canvas_id": {
                    "type": "string",
                    "description": "Canvas identifier (required for render, snapshot, clear)"
                },
                "content_type": {
                    "type": "string",
                    "enum": ["text/html", "image/svg+xml", "text/markdown", "text/plain"],
                    "description": "MIME type of the content (required for render)"
                },
                "content": {
                    "type": "string",
                    "description": "The content to render (required for render)"
                }
            }
        }))
    }

    async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        let parsed: Input = serde_json::from_str(input)
            .map_err(|e| anyhow::anyhow!("canvas expects JSON: {{\"action\": \"...\"}}: {e}"))?;

        match parsed.action.as_str() {
            "render" => {
                let canvas_id = parsed
                    .canvas_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("render requires canvas_id"))?;
                let content_type = parsed
                    .content_type
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("render requires content_type"))?;
                let content = parsed
                    .content
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("render requires content"))?;

                self.store.render(canvas_id, content_type, content).await?;
                Ok(ToolResult {
                    output: format!("Rendered to canvas {canvas_id}"),
                })
            }
            "snapshot" => {
                let canvas_id = parsed
                    .canvas_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("snapshot requires canvas_id"))?;

                match self.store.snapshot(canvas_id).await {
                    Some(canvas) => {
                        let json = serde_json::to_string_pretty(&canvas)
                            .unwrap_or_else(|e| format!("serialization error: {e}"));
                        Ok(ToolResult { output: json })
                    }
                    None => Ok(ToolResult {
                        output: "Canvas not found".to_string(),
                    }),
                }
            }
            "clear" => {
                let canvas_id = parsed
                    .canvas_id
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("clear requires canvas_id"))?;

                if self.store.clear(canvas_id).await {
                    Ok(ToolResult {
                        output: "Canvas cleared".to_string(),
                    })
                } else {
                    Ok(ToolResult {
                        output: "Canvas not found".to_string(),
                    })
                }
            }
            "list" => {
                let summaries = self.store.list().await;
                let json = serde_json::to_string_pretty(&summaries)
                    .unwrap_or_else(|e| format!("serialization error: {e}"));
                Ok(ToolResult { output: json })
            }
            other => Err(anyhow::anyhow!(
                "unknown action '{other}'; expected render, snapshot, clear, or list"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ToolContext;

    fn ctx() -> ToolContext {
        ToolContext::new("/tmp".to_string())
    }

    fn make_tool() -> CanvasTool {
        CanvasTool::new(Arc::new(CanvasStore::new()))
    }

    #[test]
    fn input_schema_is_valid_json() {
        let tool = make_tool();
        let schema = tool.input_schema().expect("should have schema");
        assert_eq!(schema["required"][0], "action");
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["canvas_id"].is_object());
        assert!(schema["properties"]["content_type"].is_object());
        assert!(schema["properties"]["content"].is_object());
    }

    #[tokio::test]
    async fn render_requires_canvas_id() {
        let tool = make_tool();
        let err = tool
            .execute(
                r#"{"action": "render", "content_type": "text/html", "content": "<p>hi</p>"}"#,
                &ctx(),
            )
            .await
            .expect_err("missing canvas_id should fail");
        assert!(err.to_string().contains("canvas_id"));
    }

    #[tokio::test]
    async fn render_requires_content() {
        let tool = make_tool();
        let err = tool
            .execute(
                r#"{"action": "render", "canvas_id": "c1", "content_type": "text/html"}"#,
                &ctx(),
            )
            .await
            .expect_err("missing content should fail");
        assert!(err.to_string().contains("content"));
    }

    #[tokio::test]
    async fn invalid_action_returns_error() {
        let tool = make_tool();
        let err = tool
            .execute(r#"{"action": "explode"}"#, &ctx())
            .await
            .expect_err("invalid action should fail");
        assert!(err.to_string().contains("unknown action"));
    }

    #[tokio::test]
    async fn invalid_json_returns_error() {
        let tool = make_tool();
        let err = tool
            .execute("not json at all", &ctx())
            .await
            .expect_err("invalid JSON should fail");
        assert!(err.to_string().contains("canvas expects JSON"));
    }

    #[tokio::test]
    async fn render_with_valid_input_succeeds() {
        let store = Arc::new(CanvasStore::new());
        let tool = CanvasTool::new(store.clone());
        let result = tool
            .execute(
                r#"{"action": "render", "canvas_id": "test1", "content_type": "text/html", "content": "<h1>Hello</h1>"}"#,
                &ctx(),
            )
            .await
            .expect("render should succeed");
        assert!(result.output.contains("Rendered to canvas test1"));

        // Verify via snapshot
        let snap = store.snapshot("test1").await.expect("canvas should exist");
        assert_eq!(
            snap.current.as_ref().expect("current frame exists").content,
            "<h1>Hello</h1>"
        );
    }

    #[tokio::test]
    async fn snapshot_returns_canvas_not_found() {
        let tool = make_tool();
        let result = tool
            .execute(
                r#"{"action": "snapshot", "canvas_id": "nonexistent"}"#,
                &ctx(),
            )
            .await
            .expect("snapshot should succeed even for missing canvas");
        assert!(result.output.contains("Canvas not found"));
    }

    #[tokio::test]
    async fn clear_returns_canvas_not_found() {
        let tool = make_tool();
        let result = tool
            .execute(r#"{"action": "clear", "canvas_id": "nonexistent"}"#, &ctx())
            .await
            .expect("clear should succeed even for missing canvas");
        assert!(result.output.contains("Canvas not found"));
    }

    #[tokio::test]
    async fn list_returns_empty_array() {
        let tool = make_tool();
        let result = tool
            .execute(r#"{"action": "list"}"#, &ctx())
            .await
            .expect("list should succeed");
        assert!(result.output.contains("[]"));
    }
}

//! Composable pre/post interceptors for tool execution.
//!
//! Similar to the provider `LlmLayer` pipeline, tool middleware enables
//! pluggable cross-cutting concerns: checkpointing, audit logging, rate
//! limiting, timing — all without modifying individual tool implementations.

use crate::{Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Instant;

/// A middleware that wraps tool execution with pre/post hooks.
#[async_trait]
pub trait ToolMiddleware: Send + Sync {
    /// Called before tool execution. Return `Err` to block the tool.
    async fn before(&self, tool_name: &str, input: &str, ctx: &ToolContext) -> anyhow::Result<()> {
        let _ = (tool_name, input, ctx);
        Ok(())
    }

    /// Called after tool execution with the result.
    async fn after(
        &self,
        tool_name: &str,
        input: &str,
        result: &anyhow::Result<ToolResult>,
        duration_ms: u64,
        ctx: &ToolContext,
    ) {
        let _ = (tool_name, input, result, duration_ms, ctx);
    }
}

/// A tool wrapped with middleware. Implements `Tool` by delegating to
/// the inner tool with pre/post hooks from the middleware stack.
pub struct MiddlewareWrappedTool {
    inner: Box<dyn Tool>,
    middleware: Vec<Arc<dyn ToolMiddleware>>,
}

impl MiddlewareWrappedTool {
    pub fn new(inner: Box<dyn Tool>, middleware: Vec<Arc<dyn ToolMiddleware>>) -> Self {
        Self { inner, middleware }
    }
}

#[async_trait]
impl Tool for MiddlewareWrappedTool {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn description(&self) -> &'static str {
        self.inner.description()
    }

    fn input_schema(&self) -> Option<serde_json::Value> {
        self.inner.input_schema()
    }

    async fn execute(&self, input: &str, ctx: &ToolContext) -> anyhow::Result<ToolResult> {
        // Run pre-hooks.
        for mw in &self.middleware {
            mw.before(self.inner.name(), input, ctx).await?;
        }

        // Execute the tool.
        let start = Instant::now();
        let result = self.inner.execute(input, ctx).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Run post-hooks (even on failure).
        for mw in &self.middleware {
            mw.after(self.inner.name(), input, &result, duration_ms, ctx)
                .await;
        }

        result
    }
}

/// Wrap a set of tools with the given middleware stack.
pub fn wrap_tools(
    tools: Vec<Box<dyn Tool>>,
    middleware: &[Arc<dyn ToolMiddleware>],
) -> Vec<Box<dyn Tool>> {
    if middleware.is_empty() {
        return tools;
    }
    tools
        .into_iter()
        .map(|tool| {
            Box::new(MiddlewareWrappedTool::new(tool, middleware.to_vec())) as Box<dyn Tool>
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Built-in middleware implementations
// ---------------------------------------------------------------------------

/// Logs tool execution timing and success/failure.
pub struct TimingMiddleware;

#[async_trait]
impl ToolMiddleware for TimingMiddleware {
    async fn after(
        &self,
        tool_name: &str,
        _input: &str,
        result: &anyhow::Result<ToolResult>,
        duration_ms: u64,
        _ctx: &ToolContext,
    ) {
        let success = result.is_ok();
        tracing::debug!(
            tool = tool_name,
            duration_ms,
            success,
            "tool execution completed"
        );
    }
}

/// Rate limiter that blocks tool execution when invoked too frequently.
pub struct RateLimitMiddleware {
    /// Maximum invocations per window.
    max_per_window: u32,
    /// Window duration.
    window: std::time::Duration,
    /// Tracks invocations per tool name.
    counts: std::sync::Mutex<std::collections::HashMap<String, Vec<Instant>>>,
}

impl RateLimitMiddleware {
    pub fn new(max_per_window: u32, window: std::time::Duration) -> Self {
        Self {
            max_per_window,
            window,
            counts: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[async_trait]
impl ToolMiddleware for RateLimitMiddleware {
    async fn before(
        &self,
        tool_name: &str,
        _input: &str,
        _ctx: &ToolContext,
    ) -> anyhow::Result<()> {
        let now = Instant::now();
        let mut counts = self.counts.lock().expect("rate limit lock poisoned");
        let entries = counts.entry(tool_name.to_string()).or_default();

        // Prune old entries.
        entries.retain(|t| now.duration_since(*t) < self.window);

        if entries.len() >= self.max_per_window as usize {
            anyhow::bail!(
                "tool '{}' rate limited: {} invocations in {:?}",
                tool_name,
                self.max_per_window,
                self.window
            );
        }

        entries.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }
        async fn execute(&self, input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: input.to_string(),
            })
        }
    }

    struct FailTool;

    #[async_trait]
    impl Tool for FailTool {
        fn name(&self) -> &'static str {
            "fail"
        }
        async fn execute(&self, _input: &str, _ctx: &ToolContext) -> anyhow::Result<ToolResult> {
            anyhow::bail!("intentional failure")
        }
    }

    struct RecordingMiddleware {
        before_count: std::sync::atomic::AtomicU32,
        after_count: std::sync::atomic::AtomicU32,
    }

    impl RecordingMiddleware {
        fn new() -> Self {
            Self {
                before_count: std::sync::atomic::AtomicU32::new(0),
                after_count: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl ToolMiddleware for RecordingMiddleware {
        async fn before(&self, _: &str, _: &str, _: &ToolContext) -> anyhow::Result<()> {
            self.before_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
        async fn after(
            &self,
            _: &str,
            _: &str,
            _: &anyhow::Result<ToolResult>,
            _: u64,
            _: &ToolContext,
        ) {
            self.after_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn middleware_hooks_called() {
        let mw = Arc::new(RecordingMiddleware::new());
        let tool = MiddlewareWrappedTool::new(Box::new(EchoTool), vec![mw.clone()]);
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute("hello", &ctx).await.expect("should succeed");
        assert_eq!(result.output, "hello");
        assert_eq!(
            mw.before_count.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(mw.after_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn after_called_on_failure() {
        let mw = Arc::new(RecordingMiddleware::new());
        let tool = MiddlewareWrappedTool::new(Box::new(FailTool), vec![mw.clone()]);
        let ctx = ToolContext::new("/tmp".to_string());
        let result = tool.execute("", &ctx).await;
        assert!(result.is_err());
        assert_eq!(mw.after_count.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn rate_limit_blocks() {
        let mw: Arc<dyn ToolMiddleware> = Arc::new(RateLimitMiddleware::new(
            2,
            std::time::Duration::from_secs(60),
        ));
        let tool = MiddlewareWrappedTool::new(Box::new(EchoTool), vec![mw]);
        let ctx = ToolContext::new("/tmp".to_string());
        tool.execute("1", &ctx).await.expect("first ok");
        tool.execute("2", &ctx).await.expect("second ok");
        let result = tool.execute("3", &ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rate limited"));
    }

    #[tokio::test]
    async fn wrap_tools_applies_middleware() {
        let mw = Arc::new(RecordingMiddleware::new());
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(EchoTool), Box::new(EchoTool)];
        let mw_dyn: Arc<dyn ToolMiddleware> = mw.clone();
        let wrapped = wrap_tools(tools, std::slice::from_ref(&mw_dyn));
        assert_eq!(wrapped.len(), 2);

        let ctx = ToolContext::new("/tmp".to_string());
        for t in &wrapped {
            t.execute("test", &ctx).await.expect("ok");
        }
        assert_eq!(
            mw.before_count.load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn wrap_empty_middleware_returns_original() {
        let tools: Vec<Box<dyn Tool>> = vec![Box::new(EchoTool)];
        let wrapped = wrap_tools(tools, &[]);
        assert_eq!(wrapped.len(), 1);
    }
}

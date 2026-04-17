//! Composable LLM provider pipeline — tower-style middleware layers.
//!
//! Each [`LlmLayer`] wraps an `Arc<dyn Provider>` and returns a new
//! `Arc<dyn Provider>`, adding cross-cutting behavior (metrics, cost caps,
//! fallback) transparently. The inner provider is unaware of the wrapping.
//!
//! ```ignore
//! let provider = PipelineBuilder::new()
//!     .layer(MetricsLayer::new("anthropic", "claude-sonnet"))
//!     .layer(CostCapLayer::new(500_000)) // 50 cents
//!     .build(base_provider);
//! ```

use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Core trait
// ---------------------------------------------------------------------------

/// A composable middleware layer that wraps an LLM provider.
///
/// Layers are applied outermost-first: the last layer added to the
/// [`PipelineBuilder`] is the first to see each request.
pub trait LlmLayer: Send + Sync {
    /// Wrap `inner` and return a new provider with added behavior.
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider>;
}

/// Builder that composes [`LlmLayer`]s around a base provider.
///
/// Layers are applied in the order they were added: the first layer wraps the
/// base, the second wraps the first, etc. This means the *last* added layer
/// is the outermost (first to see requests).
pub struct PipelineBuilder {
    layers: Vec<Box<dyn LlmLayer>>,
}

impl PipelineBuilder {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a layer. Returns `self` for chaining.
    pub fn layer(mut self, layer: impl LlmLayer + 'static) -> Self {
        self.layers.push(Box::new(layer));
        self
    }

    /// Build the final provider by wrapping `base` with all layers.
    pub fn build(self, base: Arc<dyn Provider>) -> Arc<dyn Provider> {
        let mut provider = base;
        for layer in self.layers {
            provider = layer.wrap(provider);
        }
        provider
    }
}

impl Default for PipelineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MetricsLayer — wraps any provider with timing + counting
// ---------------------------------------------------------------------------

/// Layer that records per-request metrics: latency, success/error counts,
/// and token usage via the `metrics` crate.
pub struct MetricsLayer {
    provider_label: String,
    model_label: String,
}

impl MetricsLayer {
    pub fn new(provider_label: impl Into<String>, model_label: impl Into<String>) -> Self {
        Self {
            provider_label: provider_label.into(),
            model_label: model_label.into(),
        }
    }
}

impl LlmLayer for MetricsLayer {
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(MetricsProvider {
            inner,
            provider_label: self.provider_label.clone(),
            model_label: self.model_label.clone(),
        })
    }
}

struct MetricsProvider {
    inner: Arc<dyn Provider>,
    provider_label: String,
    model_label: String,
}

impl MetricsProvider {
    fn record_success(&self, duration: std::time::Duration, result: &ChatResult) {
        crate::provider_metrics::record_provider_success(
            &self.provider_label,
            &self.model_label,
            duration.as_secs_f64(),
        );
        crate::provider_metrics::record_token_usage(
            &self.provider_label,
            &self.model_label,
            result.input_tokens as u32,
            result.output_tokens as u32,
        );
    }

    fn record_error(&self, duration: std::time::Duration, err: &anyhow::Error) {
        let error_type = if err.to_string().contains("(429)") {
            "rate_limit"
        } else if err.to_string().contains("timeout") {
            "timeout"
        } else {
            "error"
        };
        crate::provider_metrics::record_provider_error(
            &self.provider_label,
            &self.model_label,
            error_type,
            duration.as_secs_f64(),
        );
    }
}

#[async_trait]
impl Provider for MetricsProvider {
    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        let start = Instant::now();
        match self.inner.complete(prompt).await {
            Ok(result) => {
                self.record_success(start.elapsed(), &result);
                Ok(result)
            }
            Err(e) => {
                self.record_error(start.elapsed(), &e);
                Err(e)
            }
        }
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let start = Instant::now();
        match self.inner.complete_with_reasoning(prompt, reasoning).await {
            Ok(result) => {
                self.record_success(start.elapsed(), &result);
                Ok(result)
            }
            Err(e) => {
                self.record_error(start.elapsed(), &e);
                Err(e)
            }
        }
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let start = Instant::now();
        match self.inner.complete_streaming(prompt, sender).await {
            Ok(result) => {
                self.record_success(start.elapsed(), &result);
                Ok(result)
            }
            Err(e) => {
                self.record_error(start.elapsed(), &e);
                Err(e)
            }
        }
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let start = Instant::now();
        match self
            .inner
            .complete_with_tools(messages, tools, reasoning)
            .await
        {
            Ok(result) => {
                self.record_success(start.elapsed(), &result);
                Ok(result)
            }
            Err(e) => {
                self.record_error(start.elapsed(), &e);
                Err(e)
            }
        }
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let start = Instant::now();
        match self
            .inner
            .complete_streaming_with_tools(messages, tools, reasoning, sender)
            .await
        {
            Ok(result) => {
                self.record_success(start.elapsed(), &result);
                Ok(result)
            }
            Err(e) => {
                self.record_error(start.elapsed(), &e);
                Err(e)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CostCapLayer — enforces per-session spending limits
// ---------------------------------------------------------------------------

/// Layer that tracks cumulative cost and rejects requests when the budget
/// is exceeded. Cost is estimated from token counts using microdollar pricing.
pub struct CostCapLayer {
    /// Maximum allowed cost in microdollars (1 USD = 1_000_000).
    budget_microdollars: u64,
    /// Shared counter across all requests through this layer.
    spent: Arc<AtomicU64>,
    /// Provider name for pricing lookup.
    provider: String,
    /// Model name for pricing lookup.
    model: String,
}

impl CostCapLayer {
    /// Create a cost cap layer.
    ///
    /// - `budget_microdollars`: max spend (e.g., 500_000 = $0.50)
    /// - `provider`: provider name for pricing lookup (e.g., "anthropic")
    /// - `model`: model name for pricing lookup (e.g., "claude-sonnet-4-6")
    pub fn new(
        budget_microdollars: u64,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            budget_microdollars,
            spent: Arc::new(AtomicU64::new(0)),
            provider: provider.into(),
            model: model.into(),
        }
    }

    /// Current cumulative spend in microdollars.
    pub fn spent(&self) -> u64 {
        self.spent.load(Ordering::Relaxed)
    }

    /// Remaining budget in microdollars.
    pub fn remaining(&self) -> u64 {
        self.budget_microdollars
            .saturating_sub(self.spent.load(Ordering::Relaxed))
    }
}

impl LlmLayer for CostCapLayer {
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(CostCapProvider {
            inner,
            budget_microdollars: self.budget_microdollars,
            spent: self.spent.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
        })
    }
}

struct CostCapProvider {
    inner: Arc<dyn Provider>,
    budget_microdollars: u64,
    spent: Arc<AtomicU64>,
    provider: String,
    model: String,
}

impl CostCapProvider {
    fn check_budget(&self) -> anyhow::Result<()> {
        let current = self.spent.load(Ordering::Relaxed);
        if current >= self.budget_microdollars {
            anyhow::bail!(
                "cost cap exceeded: spent ${:.4} of ${:.4} budget",
                current as f64 / 1_000_000.0,
                self.budget_microdollars as f64 / 1_000_000.0
            );
        }
        Ok(())
    }

    fn record_cost(&self, result: &ChatResult) {
        if let Some(pricing) = crate::model_pricing(&self.provider, &self.model) {
            let cost = crate::compute_cost_microdollars(
                &pricing,
                result.input_tokens,
                result.output_tokens,
            );
            self.spent.fetch_add(cost, Ordering::Relaxed);
        }
        // If no pricing found for the model, cost tracking is a no-op.
    }
}

#[async_trait]
impl Provider for CostCapProvider {
    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.check_budget()?;
        let result = self.inner.complete(prompt).await?;
        self.record_cost(&result);
        Ok(result)
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.check_budget()?;
        let result = self
            .inner
            .complete_with_reasoning(prompt, reasoning)
            .await?;
        self.record_cost(&result);
        Ok(result)
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.check_budget()?;
        let result = self.inner.complete_streaming(prompt, sender).await?;
        self.record_cost(&result);
        Ok(result)
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.check_budget()?;
        let result = self
            .inner
            .complete_with_tools(messages, tools, reasoning)
            .await?;
        self.record_cost(&result);
        Ok(result)
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.check_budget()?;
        let result = self
            .inner
            .complete_streaming_with_tools(messages, tools, reasoning, sender)
            .await?;
        self.record_cost(&result);
        Ok(result)
    }
}

// Note: FallbackProvider is already composable and implements Provider directly.
// Use it before the pipeline: `PipelineBuilder::new().layer(...).build(Arc::new(fallback_provider))`
// A FallbackLayer is not needed since FallbackProvider takes ownership of providers at construction.

// ---------------------------------------------------------------------------
// CostEstimateLayer — warns before expensive calls
// ---------------------------------------------------------------------------

/// Layer that estimates the cost of a request before execution.
/// Logs a warning when the estimated cost exceeds the configured threshold.
/// Does NOT block — only warns. Use `CostCapLayer` for hard limits.
pub struct CostEstimateLayer {
    /// Warn when estimated cost exceeds this threshold (microdollars).
    warn_threshold_microdollars: u64,
    provider: String,
    model: String,
}

impl CostEstimateLayer {
    pub fn new(
        warn_threshold_microdollars: u64,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            warn_threshold_microdollars,
            provider: provider.into(),
            model: model.into(),
        }
    }
}

impl LlmLayer for CostEstimateLayer {
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(CostEstimateProvider {
            inner,
            warn_threshold: self.warn_threshold_microdollars,
            provider: self.provider.clone(),
            model: self.model.clone(),
        })
    }
}

struct CostEstimateProvider {
    inner: Arc<dyn Provider>,
    warn_threshold: u64,
    provider: String,
    model: String,
}

impl CostEstimateProvider {
    /// Estimate cost from character count (rough: ~4 chars per token).
    fn estimate_and_warn(&self, char_count: usize) {
        let estimated_tokens = (char_count / 4) as u64;
        if let Some(pricing) = crate::model_pricing(&self.provider, &self.model) {
            let estimated_cost = crate::compute_cost_microdollars(&pricing, estimated_tokens, 0);
            if estimated_cost > self.warn_threshold {
                tracing::warn!(
                    estimated_input_tokens = estimated_tokens,
                    estimated_cost_microdollars = estimated_cost,
                    threshold_microdollars = self.warn_threshold,
                    "pre-execution cost estimate exceeds warning threshold (${:.4} estimated)",
                    estimated_cost as f64 / 1_000_000.0
                );
            }
        }
    }

    fn estimate_messages_and_warn(&self, messages: &[ConversationMessage]) {
        let total_chars: usize = messages.iter().map(|m| m.char_count()).sum();
        self.estimate_and_warn(total_chars);
    }
}

#[async_trait]
impl Provider for CostEstimateProvider {
    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.estimate_and_warn(prompt.len());
        self.inner.complete(prompt).await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.estimate_and_warn(prompt.len());
        self.inner.complete_with_reasoning(prompt, reasoning).await
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.estimate_and_warn(prompt.len());
        self.inner.complete_streaming(prompt, sender).await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.estimate_messages_and_warn(messages);
        self.inner
            .complete_with_tools(messages, tools, reasoning)
            .await
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.estimate_messages_and_warn(messages);
        self.inner
            .complete_streaming_with_tools(messages, tools, reasoning, sender)
            .await
    }
}

// ---------------------------------------------------------------------------
// PromptCacheLayer — Anthropic cache_control annotations
// ---------------------------------------------------------------------------

/// Layer that annotates messages with Anthropic's `cache_control` to enable
/// prompt caching. Caches the system prompt + last N messages, reducing
/// input token costs by up to 90% for repeated prefixes.
///
/// Only effective with Anthropic providers — other providers ignore the
/// annotation and pass through unchanged.
pub struct PromptCacheLayer {
    /// Number of recent messages to mark for caching (in addition to system prompt).
    cache_recent: usize,
}

impl PromptCacheLayer {
    /// Create a prompt cache layer. `cache_recent` is the number of recent
    /// messages to cache (default recommendation: 3).
    pub fn new(cache_recent: usize) -> Self {
        Self { cache_recent }
    }
}

impl LlmLayer for PromptCacheLayer {
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(PromptCacheProvider {
            inner,
            cache_recent: self.cache_recent,
        })
    }
}

struct PromptCacheProvider {
    inner: Arc<dyn Provider>,
    cache_recent: usize,
}

impl PromptCacheProvider {
    /// Clone messages and add cache markers to system prompt + last N messages.
    fn annotate_messages(&self, messages: &[ConversationMessage]) -> Vec<ConversationMessage> {
        if messages.is_empty() {
            return vec![];
        }

        let mut annotated = messages.to_vec();
        let len = annotated.len();

        // Mark system prompt for caching (always first if present).
        if matches!(&annotated[0], ConversationMessage::System { .. }) {
            // System prompts are cached by the provider automatically when
            // cache_control is set. We mark with a sentinel in the content
            // that the Anthropic adapter recognizes.
            if let ConversationMessage::System { ref mut content } = annotated[0] {
                if !content.ends_with("\n[cache:ephemeral]") {
                    content.push_str("\n[cache:ephemeral]");
                }
            }
        }

        // Mark the last N messages for caching.
        let cache_start = len.saturating_sub(self.cache_recent);
        for msg in annotated[cache_start..].iter_mut() {
            match msg {
                ConversationMessage::User {
                    ref mut content, ..
                } if !content.ends_with("\n[cache:ephemeral]") => {
                    content.push_str("\n[cache:ephemeral]");
                }
                ConversationMessage::Assistant {
                    content: Some(ref mut c),
                    ..
                } if !c.ends_with("\n[cache:ephemeral]") => {
                    c.push_str("\n[cache:ephemeral]");
                }
                _ => {}
            }
        }

        annotated
    }
}

#[async_trait]
impl Provider for PromptCacheProvider {
    fn supports_streaming(&self) -> bool {
        self.inner.supports_streaming()
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        // Simple complete doesn't use messages, pass through.
        self.inner.complete(prompt).await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.inner.complete_with_reasoning(prompt, reasoning).await
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.inner.complete_streaming(prompt, sender).await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let annotated = self.annotate_messages(messages);
        self.inner
            .complete_with_tools(&annotated, tools, reasoning)
            .await
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let annotated = self.annotate_messages(messages);
        self.inner
            .complete_streaming_with_tools(&annotated, tools, reasoning, sender)
            .await
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ChatResult;
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

    struct MockProvider {
        call_count: Arc<AtomicU32>,
        output: String,
        input_tokens: u64,
        output_tokens: u64,
    }

    impl MockProvider {
        fn new(output: &str) -> (Arc<Self>, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            let provider = Arc::new(Self {
                call_count: count.clone(),
                output: output.to_string(),
                input_tokens: 100_u64,
                output_tokens: 50_u64,
            });
            (provider, count)
        }
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            self.call_count.fetch_add(1, AtomicOrdering::Relaxed);
            Ok(ChatResult {
                output_text: self.output.clone(),
                input_tokens: self.input_tokens,
                output_tokens: self.output_tokens,
                ..ChatResult::default()
            })
        }
    }

    struct FailingMockProvider;

    #[async_trait]
    impl Provider for FailingMockProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            anyhow::bail!("mock provider failed (429): rate limited")
        }
    }

    #[tokio::test]
    async fn pipeline_builder_no_layers() {
        let (provider, count) = MockProvider::new("hello");
        let pipeline = PipelineBuilder::new().build(provider);
        let result = pipeline.complete("test").await.expect("should succeed");
        assert_eq!(result.output_text, "hello");
        assert_eq!(count.load(AtomicOrdering::Relaxed), 1);
    }

    #[tokio::test]
    async fn pipeline_builder_with_metrics_layer() {
        let (provider, count) = MockProvider::new("hello");
        let pipeline = PipelineBuilder::new()
            .layer(MetricsLayer::new("test", "test-model"))
            .build(provider);
        let result = pipeline.complete("test").await.expect("should succeed");
        assert_eq!(result.output_text, "hello");
        assert_eq!(count.load(AtomicOrdering::Relaxed), 1);
    }

    #[tokio::test]
    async fn cost_cap_layer_blocks_when_exceeded() {
        let (provider, _) = MockProvider::new("hello");
        // 1 microdollar budget — any real call will exceed this
        let cost_cap = CostCapLayer::new(1, "anthropic", "claude-sonnet-4-6");

        let pipeline = PipelineBuilder::new().layer(cost_cap).build(provider);

        // First call succeeds (cost recorded after)
        let result = pipeline.complete("test").await;
        assert!(result.is_ok());

        // Second call should fail (budget exceeded)
        let result = pipeline.complete("test").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("cost cap exceeded"));
    }

    #[tokio::test]
    async fn cost_cap_layer_tracks_spending() {
        let cost_cap = CostCapLayer::new(10_000_000, "anthropic", "unknown-model");
        assert_eq!(cost_cap.spent(), 0);
        assert_eq!(cost_cap.remaining(), 10_000_000);
    }

    #[tokio::test]
    async fn multiple_layers_compose() {
        let (provider, count) = MockProvider::new("composed");
        let pipeline = PipelineBuilder::new()
            .layer(MetricsLayer::new("test", "model"))
            .layer(CostCapLayer::new(10_000_000, "test", "unknown"))
            .build(provider);

        let result = pipeline.complete("test").await.expect("should succeed");
        assert_eq!(result.output_text, "composed");
        assert_eq!(count.load(AtomicOrdering::Relaxed), 1);
    }

    #[tokio::test]
    async fn metrics_layer_records_errors() {
        let provider: Arc<dyn Provider> = Arc::new(FailingMockProvider);
        let pipeline = PipelineBuilder::new()
            .layer(MetricsLayer::new("test", "model"))
            .build(provider);

        let result = pipeline.complete("test").await;
        assert!(result.is_err());
        // Metrics recorded (no-op without recorder, but no panic)
    }

    #[tokio::test]
    async fn cost_estimate_layer_passes_through() {
        let (provider, count) = MockProvider::new("hello");
        let pipeline = PipelineBuilder::new()
            .layer(CostEstimateLayer::new(1, "anthropic", "claude-sonnet-4-6"))
            .build(provider);
        // CostEstimateLayer only warns, never blocks.
        let result = pipeline.complete("test").await.expect("should succeed");
        assert_eq!(result.output_text, "hello");
        assert_eq!(count.load(AtomicOrdering::Relaxed), 1);
    }

    #[tokio::test]
    async fn prompt_cache_layer_annotates_messages() {
        let cache_provider = PromptCacheProvider {
            inner: MockProvider::new("ok").0 as Arc<dyn Provider>,
            cache_recent: 2,
        };
        let messages = vec![
            ConversationMessage::System {
                content: "You are a bot.".to_string(),
            },
            ConversationMessage::user("first question".to_string()),
            ConversationMessage::Assistant {
                content: Some("answer 1".to_string()),
                tool_calls: vec![],
            },
            ConversationMessage::user("second question".to_string()),
        ];
        let annotated = cache_provider.annotate_messages(&messages);
        // System prompt should have cache marker.
        if let ConversationMessage::System { content } = &annotated[0] {
            assert!(content.ends_with("[cache:ephemeral]"));
        } else {
            panic!("expected system message");
        }
        // Last 2 messages should have cache markers.
        if let ConversationMessage::Assistant { content, .. } = &annotated[2] {
            assert!(content
                .as_ref()
                .expect("content")
                .ends_with("[cache:ephemeral]"));
        }
        if let ConversationMessage::User { content, .. } = &annotated[3] {
            assert!(content.ends_with("[cache:ephemeral]"));
        }
        // First user message (not in last 2) should NOT have cache marker.
        if let ConversationMessage::User { content, .. } = &annotated[1] {
            assert!(!content.contains("[cache:ephemeral]"));
        }
    }

    #[tokio::test]
    async fn prompt_cache_layer_idempotent() {
        let cache_provider = PromptCacheProvider {
            inner: MockProvider::new("ok").0 as Arc<dyn Provider>,
            cache_recent: 1,
        };
        let messages = vec![ConversationMessage::user("hi".to_string())];
        let annotated1 = cache_provider.annotate_messages(&messages);
        let annotated2 = cache_provider.annotate_messages(&annotated1);
        // Should not double-annotate.
        if let ConversationMessage::User { content, .. } = &annotated2[0] {
            assert_eq!(
                content.matches("[cache:ephemeral]").count(),
                1,
                "should not double-annotate"
            );
        }
    }
}

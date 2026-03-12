//! Fallback provider chain — tries alternate providers when the primary fails.
//!
//! When the primary provider returns an error (circuit breaker open, 5xx,
//! timeout), the `FallbackProvider` transparently tries the next provider in
//! the chain. Emits `provider_fallback_total` metrics with `from`/`to` labels.

use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition,
};
use async_trait::async_trait;
use tracing::{info, warn};

/// A provider that chains multiple providers in fallback order.
///
/// On each request, tries providers in order. If the first provider fails,
/// the error is logged and the next provider is tried. If all providers fail,
/// the last error is returned.
pub struct FallbackProvider {
    /// Ordered list of `(label, provider)` pairs. The first is the primary.
    providers: Vec<(String, Box<dyn Provider>)>,
}

impl FallbackProvider {
    /// Create a new fallback provider from an ordered list of `(label, provider)` pairs.
    ///
    /// At least one provider must be supplied; panics otherwise.
    pub fn new(providers: Vec<(String, Box<dyn Provider>)>) -> Self {
        assert!(
            !providers.is_empty(),
            "FallbackProvider requires at least one provider"
        );
        Self { providers }
    }

    /// Number of providers in the chain.
    pub fn chain_len(&self) -> usize {
        self.providers.len()
    }

    /// Record a fallback metric.
    fn record_fallback(from: &str, to: &str) {
        metrics::counter!("provider_fallback_total", "from" => from.to_string(), "to" => to.to_string())
            .increment(1);
        info!(from = from, to = to, "provider fallback triggered");
    }
}

#[async_trait]
impl Provider for FallbackProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            match provider.complete(prompt).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!(provider = %label, error = %e, "provider failed, trying next in chain");
                    if i + 1 < self.providers.len() {
                        Self::record_fallback(label, &self.providers[i + 1].0);
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.expect("at least one provider in chain"))
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            match provider.complete_with_reasoning(prompt, reasoning).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!(provider = %label, error = %e, "provider failed, trying next in chain");
                    if i + 1 < self.providers.len() {
                        Self::record_fallback(label, &self.providers[i + 1].0);
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.expect("at least one provider in chain"))
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        // Streaming can only be attempted on the first viable provider since
        // partial chunks may have been sent. We try each provider but only
        // create a new sender channel for attempts after the first.
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            if i == 0 {
                // First attempt uses the original sender directly.
                match provider.complete_streaming(prompt, sender.clone()).await {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        warn!(provider = %label, error = %e, "streaming provider failed, trying next");
                        if i + 1 < self.providers.len() {
                            Self::record_fallback(label, &self.providers[i + 1].0);
                        }
                        last_err = Some(e);
                    }
                }
            } else {
                // Subsequent attempts: fall back to non-streaming complete to
                // avoid sending partial/duplicate chunks from multiple providers.
                match provider.complete(prompt).await {
                    Ok(result) => {
                        let _ = sender.send(StreamChunk {
                            delta: result.output_text.clone(),
                            done: true,
                            tool_call_delta: None,
                        });
                        return Ok(result);
                    }
                    Err(e) => {
                        warn!(provider = %label, error = %e, "fallback provider failed");
                        if i + 1 < self.providers.len() {
                            Self::record_fallback(label, &self.providers[i + 1].0);
                        }
                        last_err = Some(e);
                    }
                }
            }
        }
        Err(last_err.expect("at least one provider in chain"))
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            match provider
                .complete_with_tools(messages, tools, reasoning)
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    warn!(provider = %label, error = %e, "provider failed, trying next in chain");
                    if i + 1 < self.providers.len() {
                        Self::record_fallback(label, &self.providers[i + 1].0);
                    }
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.expect("at least one provider in chain"))
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            if i == 0 {
                match provider
                    .complete_streaming_with_tools(messages, tools, reasoning, sender.clone())
                    .await
                {
                    Ok(result) => return Ok(result),
                    Err(e) => {
                        warn!(provider = %label, error = %e, "streaming provider failed, trying next");
                        if i + 1 < self.providers.len() {
                            Self::record_fallback(label, &self.providers[i + 1].0);
                        }
                        last_err = Some(e);
                    }
                }
            } else {
                // Fall back to non-streaming to avoid duplicate chunks.
                match provider
                    .complete_with_tools(messages, tools, reasoning)
                    .await
                {
                    Ok(result) => {
                        let _ = sender.send(StreamChunk {
                            delta: result.output_text.clone(),
                            done: true,
                            tool_call_delta: None,
                        });
                        return Ok(result);
                    }
                    Err(e) => {
                        warn!(provider = %label, error = %e, "fallback provider failed");
                        if i + 1 < self.providers.len() {
                            Self::record_fallback(label, &self.providers[i + 1].0);
                        }
                        last_err = Some(e);
                    }
                }
            }
        }
        Err(last_err.expect("at least one provider in chain"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{ChatResult, ReasoningConfig};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// A provider that always fails.
    struct FailingProvider {
        label: String,
        call_count: Arc<AtomicU32>,
    }

    impl FailingProvider {
        fn new(label: &str) -> (Self, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            (
                Self {
                    label: label.to_string(),
                    call_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Provider for FailingProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Err(anyhow::anyhow!("{} failed", self.label))
        }
    }

    /// A provider that always succeeds.
    struct SucceedingProvider {
        response: String,
        call_count: Arc<AtomicU32>,
    }

    impl SucceedingProvider {
        fn new(response: &str) -> (Self, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            (
                Self {
                    response: response.to_string(),
                    call_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Provider for SucceedingProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Ok(ChatResult {
                output_text: self.response.clone(),
                ..ChatResult::default()
            })
        }
    }

    #[tokio::test]
    async fn primary_succeeds_no_fallback() {
        let (primary, primary_count) = SucceedingProvider::new("primary-ok");
        let (secondary, secondary_count) = SucceedingProvider::new("secondary-ok");

        let fallback = FallbackProvider::new(vec![
            ("primary".into(), Box::new(primary)),
            ("secondary".into(), Box::new(secondary)),
        ]);

        let result = fallback.complete("hello").await.expect("should succeed");
        assert_eq!(result.output_text, "primary-ok");
        assert_eq!(primary_count.load(Ordering::Relaxed), 1);
        assert_eq!(secondary_count.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn primary_fails_fallback_succeeds() {
        let (primary, primary_count) = FailingProvider::new("primary");
        let (secondary, secondary_count) = SucceedingProvider::new("fallback-ok");

        let fallback = FallbackProvider::new(vec![
            ("primary".into(), Box::new(primary)),
            ("secondary".into(), Box::new(secondary)),
        ]);

        let result = fallback
            .complete("hello")
            .await
            .expect("fallback should succeed");
        assert_eq!(result.output_text, "fallback-ok");
        assert_eq!(primary_count.load(Ordering::Relaxed), 1);
        assert_eq!(secondary_count.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn all_providers_fail_returns_last_error() {
        let (p1, _) = FailingProvider::new("first");
        let (p2, _) = FailingProvider::new("second");
        let (p3, _) = FailingProvider::new("third");

        let fallback = FallbackProvider::new(vec![
            ("first".into(), Box::new(p1)),
            ("second".into(), Box::new(p2)),
            ("third".into(), Box::new(p3)),
        ]);

        let err = fallback
            .complete("hello")
            .await
            .expect_err("all should fail");
        assert!(
            err.to_string().contains("third failed"),
            "expected last error, got: {err}"
        );
    }

    #[tokio::test]
    async fn complete_with_tools_fallback() {
        let (primary, _) = FailingProvider::new("primary");
        let (secondary, _) = SucceedingProvider::new("tools-fallback-ok");

        let fallback = FallbackProvider::new(vec![
            ("primary".into(), Box::new(primary)),
            ("secondary".into(), Box::new(secondary)),
        ]);

        let result = fallback
            .complete_with_tools(&[], &[], &ReasoningConfig::default())
            .await
            .expect("fallback should succeed");
        assert_eq!(result.output_text, "tools-fallback-ok");
    }

    #[tokio::test]
    async fn chain_len_reports_correctly() {
        let (p1, _) = SucceedingProvider::new("a");
        let (p2, _) = SucceedingProvider::new("b");

        let fallback =
            FallbackProvider::new(vec![("a".into(), Box::new(p1)), ("b".into(), Box::new(p2))]);

        assert_eq!(fallback.chain_len(), 2);
    }

    #[test]
    #[should_panic(expected = "at least one provider")]
    fn empty_chain_panics() {
        let _ = FallbackProvider::new(vec![]);
    }
}

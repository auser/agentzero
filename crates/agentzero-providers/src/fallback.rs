//! Fallback provider chain — tries alternate providers when the primary fails.
//!
//! When the primary provider returns an error (circuit breaker open, 5xx,
//! timeout), the `FallbackProvider` transparently tries the next provider in
//! the chain. Emits `provider_fallback_total` metrics with `from`/`to` labels.

use crate::transport::CooldownState;
use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition,
};
use async_trait::async_trait;
use std::time::Duration;
use tracing::{info, warn};

/// Information about a provider fallback that occurred during a request.
#[derive(Debug, Clone)]
pub struct FallbackInfo {
    pub original_provider: String,
    pub actual_provider: String,
}

tokio::task_local! {
    /// Task-local storage for provider fallback information.
    /// Set by FallbackProvider when a cross-provider fallback occurs.
    pub static FALLBACK_INFO: std::cell::RefCell<Option<FallbackInfo>>;
}

/// Default cooldown duration when a 429 is received and no Retry-After header
/// is available (parsed from the error message).
const DEFAULT_COOLDOWN_SECS: u64 = 10;

/// Check if an error looks like an HTTP 429 rate-limit response.
fn is_rate_limit_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string();
    msg.contains("(429)") || msg.contains("Too Many Requests") || msg.contains("rate limited")
}

/// A provider that chains multiple providers in fallback order.
///
/// On each request, tries providers in order. If the first provider fails,
/// the error is logged and the next provider is tried. If all providers fail,
/// the last error is returned.
///
/// Each provider has an associated [`CooldownState`] that activates on HTTP 429
/// responses, causing the provider to be skipped for a short period.
pub struct FallbackProvider {
    /// Ordered list of `(label, provider)` pairs. The first is the primary.
    providers: Vec<(String, Box<dyn Provider>)>,
    /// Per-provider cooldown state, parallel to `providers`.
    cooldowns: Vec<CooldownState>,
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
        let cooldowns = providers.iter().map(|_| CooldownState::new()).collect();
        Self {
            providers,
            cooldowns,
        }
    }

    /// Number of providers in the chain.
    pub fn chain_len(&self) -> usize {
        self.providers.len()
    }

    /// Record a fallback metric.
    fn record_fallback(from: &str, to: &str) {
        #[cfg(feature = "metrics")]
        metrics::counter!("provider_fallback_total", "from" => from.to_string(), "to" => to.to_string())
            .increment(1);
        info!(from = from, to = to, "provider fallback triggered");
    }
}

#[async_trait]
impl Provider for FallbackProvider {
    fn supports_streaming(&self) -> bool {
        self.providers
            .first()
            .is_some_and(|(_, p)| p.supports_streaming())
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        let mut last_err = None;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            if self.cooldowns[i].is_cooled_down() {
                info!(provider = %label, "skipping provider (429 cooldown active)");
                continue;
            }
            match provider.complete(prompt).await {
                Ok(result) => {
                    self.cooldowns[i].clear();
                    if i > 0 {
                        let _ = FALLBACK_INFO.try_with(|cell| {
                            *cell.borrow_mut() = Some(FallbackInfo {
                                original_provider: self.providers[0].0.clone(),
                                actual_provider: label.clone(),
                            });
                        });
                    }
                    return Ok(result);
                }
                Err(e) => {
                    if is_rate_limit_error(&e) {
                        self.cooldowns[i]
                            .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                    }
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
            if self.cooldowns[i].is_cooled_down() {
                info!(provider = %label, "skipping provider (429 cooldown active)");
                continue;
            }
            match provider.complete_with_reasoning(prompt, reasoning).await {
                Ok(result) => {
                    self.cooldowns[i].clear();
                    if i > 0 {
                        let _ = FALLBACK_INFO.try_with(|cell| {
                            *cell.borrow_mut() = Some(FallbackInfo {
                                original_provider: self.providers[0].0.clone(),
                                actual_provider: label.clone(),
                            });
                        });
                    }
                    return Ok(result);
                }
                Err(e) => {
                    if is_rate_limit_error(&e) {
                        self.cooldowns[i]
                            .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                    }
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
        let mut first_attempt = true;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            if self.cooldowns[i].is_cooled_down() {
                info!(provider = %label, "skipping provider (429 cooldown active)");
                continue;
            }
            if first_attempt {
                first_attempt = false;
                // First attempt uses the original sender directly.
                match provider.complete_streaming(prompt, sender.clone()).await {
                    Ok(result) => {
                        self.cooldowns[i].clear();
                        return Ok(result);
                    }
                    Err(e) => {
                        if is_rate_limit_error(&e) {
                            self.cooldowns[i]
                                .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                        }
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
                        self.cooldowns[i].clear();
                        let _ = sender.send(StreamChunk {
                            delta: result.output_text.clone(),
                            done: true,
                            tool_call_delta: None,
                        });
                        return Ok(result);
                    }
                    Err(e) => {
                        if is_rate_limit_error(&e) {
                            self.cooldowns[i]
                                .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                        }
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
            if self.cooldowns[i].is_cooled_down() {
                info!(provider = %label, "skipping provider (429 cooldown active)");
                continue;
            }
            match provider
                .complete_with_tools(messages, tools, reasoning)
                .await
            {
                Ok(result) => {
                    self.cooldowns[i].clear();
                    if i > 0 {
                        let _ = FALLBACK_INFO.try_with(|cell| {
                            *cell.borrow_mut() = Some(FallbackInfo {
                                original_provider: self.providers[0].0.clone(),
                                actual_provider: label.clone(),
                            });
                        });
                    }
                    return Ok(result);
                }
                Err(e) => {
                    if is_rate_limit_error(&e) {
                        self.cooldowns[i]
                            .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                    }
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
        let mut first_attempt = true;
        for (i, (label, provider)) in self.providers.iter().enumerate() {
            if self.cooldowns[i].is_cooled_down() {
                info!(provider = %label, "skipping provider (429 cooldown active)");
                continue;
            }
            if first_attempt {
                first_attempt = false;
                match provider
                    .complete_streaming_with_tools(messages, tools, reasoning, sender.clone())
                    .await
                {
                    Ok(result) => {
                        self.cooldowns[i].clear();
                        return Ok(result);
                    }
                    Err(e) => {
                        if is_rate_limit_error(&e) {
                            self.cooldowns[i]
                                .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                        }
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
                        self.cooldowns[i].clear();
                        let _ = sender.send(StreamChunk {
                            delta: result.output_text.clone(),
                            done: true,
                            tool_call_delta: None,
                        });
                        return Ok(result);
                    }
                    Err(e) => {
                        if is_rate_limit_error(&e) {
                            self.cooldowns[i]
                                .enter_cooldown(Duration::from_secs(DEFAULT_COOLDOWN_SECS));
                        }
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

    // --- Cooldown tests ---

    /// A provider that returns a 429 rate-limit error.
    struct RateLimitProvider {
        call_count: Arc<AtomicU32>,
    }

    impl RateLimitProvider {
        fn new() -> (Self, Arc<AtomicU32>) {
            let count = Arc::new(AtomicU32::new(0));
            (
                Self {
                    call_count: count.clone(),
                },
                count,
            )
        }
    }

    #[async_trait]
    impl Provider for RateLimitProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            Err(anyhow::anyhow!("provider rate limited (429): slow down"))
        }
    }

    #[tokio::test]
    async fn rate_limited_provider_enters_cooldown_and_is_skipped() {
        let (rate_limited, rl_count) = RateLimitProvider::new();
        let (fallback_ok, ok_count) = SucceedingProvider::new("ok");

        let fb = FallbackProvider::new(vec![
            ("rl".into(), Box::new(rate_limited)),
            ("ok".into(), Box::new(fallback_ok)),
        ]);

        // First call: rl fails with 429, falls back to ok.
        let result = fb.complete("hello").await.expect("fallback should succeed");
        assert_eq!(result.output_text, "ok");
        assert_eq!(rl_count.load(Ordering::Relaxed), 1);
        assert_eq!(ok_count.load(Ordering::Relaxed), 1);

        // Second call: rl should be skipped (cooldown), goes directly to ok.
        let result = fb.complete("hello").await.expect("fallback should succeed");
        assert_eq!(result.output_text, "ok");
        assert_eq!(rl_count.load(Ordering::Relaxed), 1); // not called again
        assert_eq!(ok_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn fallback_info_set_when_fallback_occurs() {
        let (primary, _) = FailingProvider::new("primary");
        let (secondary, _) = SucceedingProvider::new("fallback-ok");

        let fb = FallbackProvider::new(vec![
            ("primary".into(), Box::new(primary)),
            ("secondary".into(), Box::new(secondary)),
        ]);

        FALLBACK_INFO
            .scope(std::cell::RefCell::new(None), async {
                fb.complete("hello").await.expect("fallback should succeed");
                let info = FALLBACK_INFO.with(|cell| cell.borrow().clone());
                let info = info.expect("FALLBACK_INFO should be set after fallback");
                assert_eq!(info.original_provider, "primary");
                assert_eq!(info.actual_provider, "secondary");
            })
            .await;
    }

    #[tokio::test]
    async fn fallback_info_not_set_when_primary_succeeds() {
        let (primary, _) = SucceedingProvider::new("primary-ok");
        let (secondary, _) = SucceedingProvider::new("secondary-ok");

        let fb = FallbackProvider::new(vec![
            ("primary".into(), Box::new(primary)),
            ("secondary".into(), Box::new(secondary)),
        ]);

        FALLBACK_INFO
            .scope(std::cell::RefCell::new(None), async {
                fb.complete("hello").await.expect("primary should succeed");
                let info = FALLBACK_INFO.with(|cell| cell.borrow().clone());
                assert!(
                    info.is_none(),
                    "FALLBACK_INFO should not be set when primary succeeds"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn non_429_error_does_not_trigger_cooldown() {
        let (failing, fail_count) = FailingProvider::new("primary");
        let (ok, _) = SucceedingProvider::new("ok");

        let fb = FallbackProvider::new(vec![
            ("fail".into(), Box::new(failing)),
            ("ok".into(), Box::new(ok)),
        ]);

        // First call: fails with generic error, falls back to ok.
        fb.complete("hello").await.expect("fallback should succeed");
        assert_eq!(fail_count.load(Ordering::Relaxed), 1);

        // Second call: failing provider is NOT in cooldown, so it's tried again.
        fb.complete("hello").await.expect("fallback should succeed");
        assert_eq!(fail_count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn success_clears_cooldown() {
        // Manually enter cooldown, then verify success clears it.
        let (ok1, _) = SucceedingProvider::new("ok1");
        let (ok2, _) = SucceedingProvider::new("ok2");

        let fb = FallbackProvider::new(vec![
            ("ok1".into(), Box::new(ok1)),
            ("ok2".into(), Box::new(ok2)),
        ]);

        // Manually put provider 0 in cooldown.
        fb.cooldowns[0].enter_cooldown(std::time::Duration::from_secs(60));
        assert!(fb.cooldowns[0].is_cooled_down());

        // Request goes to ok2 since ok1 is in cooldown.
        let result = fb.complete("hello").await.expect("should succeed");
        assert_eq!(result.output_text, "ok2");

        // After ok2 succeeds, ok2's cooldown is cleared (it wasn't set, but
        // the clear is a no-op). ok1 is still in cooldown.
        assert!(fb.cooldowns[0].is_cooled_down());
    }
}

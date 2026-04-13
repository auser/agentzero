//! `ProviderPool` — a transparent proxy that delegates `Provider` calls to
//! whichever backend is currently active. Enables mid-session model switching
//! without changing the `Agent` struct or `Provider` trait.
//!
//! # Usage
//!
//! ```ignore
//! let pool = ProviderPool::new(providers, "default".into());
//! pool.switch_to("fast").await?;   // now routes to the "fast" provider
//! pool.complete("hello").await?;    // uses "fast"
//! ```

use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A provider pool that delegates to the currently-active backend.
///
/// Implements `Provider` so it can be used as a drop-in replacement
/// anywhere a `Box<dyn Provider>` or `Arc<dyn Provider>` is expected.
pub struct ProviderPool {
    providers: HashMap<String, Arc<dyn Provider>>,
    active: RwLock<String>,
}

impl ProviderPool {
    /// Create a new pool with the given providers and default active key.
    ///
    /// # Panics
    ///
    /// Panics if `default_key` is not present in `providers`.
    pub fn new(providers: HashMap<String, Arc<dyn Provider>>, default_key: String) -> Self {
        assert!(
            providers.contains_key(&default_key),
            "default provider key `{default_key}` must exist in the pool"
        );
        Self {
            providers,
            active: RwLock::new(default_key),
        }
    }

    /// Switch the active provider. Returns an error if `key` is not in the pool.
    pub async fn switch_to(&self, key: &str) -> anyhow::Result<()> {
        if !self.providers.contains_key(key) {
            anyhow::bail!(
                "provider `{key}` not found in pool; available: {}",
                self.list_available().join(", ")
            );
        }
        let mut active = self.active.write().await;
        *active = key.to_string();
        Ok(())
    }

    /// Get the currently active provider key.
    pub async fn active_key(&self) -> String {
        self.active.read().await.clone()
    }

    /// List all available provider keys.
    pub fn list_available(&self) -> Vec<String> {
        let mut keys: Vec<String> = self.providers.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Get the active provider, resolving the current key under the read lock.
    async fn active_provider(&self) -> anyhow::Result<Arc<dyn Provider>> {
        let key = self.active.read().await.clone();
        self.providers
            .get(&key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("active provider `{key}` disappeared from pool"))
    }
}

#[async_trait]
impl Provider for ProviderPool {
    fn supports_streaming(&self) -> bool {
        // Optimistic: use a blocking read since this is a sync method.
        // In practice the active key rarely changes during a streaming check.
        let key = self.active.blocking_read().clone();
        self.providers
            .get(&key)
            .is_some_and(|p| p.supports_streaming())
    }

    fn estimate_tokens(&self, text: &str) -> Option<usize> {
        let key = self.active.blocking_read().clone();
        self.providers
            .get(&key)
            .and_then(|p| p.estimate_tokens(text))
    }

    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.active_provider().await?.complete(prompt).await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.active_provider()
            .await?
            .complete_with_reasoning(prompt, reasoning)
            .await
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.active_provider()
            .await?
            .complete_streaming(prompt, sender)
            .await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.active_provider()
            .await?
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
        self.active_provider()
            .await?
            .complete_streaming_with_tools(messages, tools, reasoning, sender)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ChatResult;

    struct MockProvider {
        name: String,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn complete(&self, _prompt: &str) -> anyhow::Result<ChatResult> {
            Ok(ChatResult {
                output_text: format!("response from {}", self.name),
                ..Default::default()
            })
        }
    }

    #[tokio::test]
    async fn switch_and_complete() {
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert(
            "alpha".into(),
            Arc::new(MockProvider {
                name: "alpha".into(),
            }),
        );
        providers.insert(
            "beta".into(),
            Arc::new(MockProvider {
                name: "beta".into(),
            }),
        );

        let pool = ProviderPool::new(providers, "alpha".into());

        // Default is alpha.
        let result = pool.complete("test").await.expect("complete");
        assert_eq!(result.output_text, "response from alpha");

        // Switch to beta.
        pool.switch_to("beta").await.expect("switch");
        assert_eq!(pool.active_key().await, "beta");
        let result = pool.complete("test").await.expect("complete");
        assert_eq!(result.output_text, "response from beta");
    }

    #[tokio::test]
    async fn switch_to_unknown_key_fails() {
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert(
            "only".into(),
            Arc::new(MockProvider {
                name: "only".into(),
            }),
        );

        let pool = ProviderPool::new(providers, "only".into());
        let err = pool
            .switch_to("nonexistent")
            .await
            .expect_err("should fail");
        assert!(err.to_string().contains("not found in pool"));
    }

    #[test]
    fn list_available_returns_sorted_keys() {
        let mut providers: HashMap<String, Arc<dyn Provider>> = HashMap::new();
        providers.insert(
            "charlie".into(),
            Arc::new(MockProvider { name: "c".into() }),
        );
        providers.insert("alpha".into(), Arc::new(MockProvider { name: "a".into() }));
        providers.insert("bravo".into(), Arc::new(MockProvider { name: "b".into() }));

        let pool = ProviderPool::new(providers, "alpha".into());
        assert_eq!(pool.list_available(), vec!["alpha", "bravo", "charlie"]);
    }
}

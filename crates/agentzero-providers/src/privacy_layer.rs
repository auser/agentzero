//! Mandatory PII-stripping pipeline layer for remote LLM provider calls.
//!
//! `PrivacyFirstLayer` wraps any `Provider` and runs `PiiRedactionGuard` on
//! every prompt — system messages, user messages, tool results — before the
//! text reaches the inner provider. Redaction is **always sanitize** (never
//! block, never audit-only) so the LLM always sees the prompt, but with PII
//! replaced by `[EMAIL_REDACTED]`, `[PHONE_REDACTED]`, etc.
//!
//! This layer is wired unconditionally in `build_runtime_execution()` as the
//! **outermost** layer in the pipeline. It cannot be disabled for remote
//! providers.

use crate::guardrails::{Guard, GuardVerdict, PiiRedactionGuard};
use crate::pipeline::LlmLayer;
use agentzero_core::{
    ChatResult, ConversationMessage, Provider, ReasoningConfig, StreamChunk, ToolDefinition,
    ToolResultMessage,
};
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Pipeline layer that unconditionally strips PII from prompts before they
/// reach a remote LLM provider. Not a config option — always on.
pub struct PrivacyFirstLayer;

impl LlmLayer for PrivacyFirstLayer {
    fn wrap(&self, inner: Arc<dyn Provider>) -> Arc<dyn Provider> {
        Arc::new(PrivacyFirstProvider {
            inner,
            guard: PiiRedactionGuard::default(),
            redactions: Arc::new(AtomicU64::new(0)),
        })
    }
}

struct PrivacyFirstProvider {
    inner: Arc<dyn Provider>,
    guard: PiiRedactionGuard,
    redactions: Arc<AtomicU64>,
}

impl PrivacyFirstProvider {
    /// Run the PII guard on a single text block. Returns the sanitized text
    /// (unchanged if no PII was found).
    fn sanitize(&self, text: &str) -> String {
        match self.guard.check_input(text) {
            GuardVerdict::Pass => text.to_string(),
            GuardVerdict::Violation { sanitized, reason } => {
                let clean = sanitized.unwrap_or_else(|| text.to_string());
                tracing::info!(reason = %reason, "PII redacted from outbound prompt");
                self.redactions.fetch_add(1, Ordering::Relaxed);
                metrics::counter!("agentzero_pii_redactions_total").increment(1);
                clean
            }
        }
    }

    /// Sanitize all text fields in a conversation message, producing a new message.
    fn sanitize_message(&self, msg: &ConversationMessage) -> ConversationMessage {
        match msg {
            ConversationMessage::System { content } => ConversationMessage::System {
                content: self.sanitize(content),
            },
            ConversationMessage::User { content, parts } => ConversationMessage::User {
                content: self.sanitize(content),
                parts: parts.clone(),
            },
            ConversationMessage::Assistant {
                content,
                tool_calls,
            } => ConversationMessage::Assistant {
                content: content.as_ref().map(|c| self.sanitize(c)),
                tool_calls: tool_calls.clone(),
            },
            ConversationMessage::ToolResult(tr) => {
                ConversationMessage::ToolResult(ToolResultMessage {
                    tool_use_id: tr.tool_use_id.clone(),
                    content: self.sanitize(&tr.content),
                    is_error: tr.is_error,
                })
            }
        }
    }

    fn sanitize_messages(&self, messages: &[ConversationMessage]) -> Vec<ConversationMessage> {
        messages.iter().map(|m| self.sanitize_message(m)).collect()
    }
}

#[async_trait]
impl Provider for PrivacyFirstProvider {
    async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
        self.inner.complete(&self.sanitize(prompt)).await
    }

    async fn complete_with_reasoning(
        &self,
        prompt: &str,
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.inner
            .complete_with_reasoning(&self.sanitize(prompt), reasoning)
            .await
    }

    async fn complete_streaming(
        &self,
        prompt: &str,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.inner
            .complete_streaming(&self.sanitize(prompt), sender)
            .await
    }

    async fn complete_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
    ) -> anyhow::Result<ChatResult> {
        self.inner
            .complete_with_tools(&self.sanitize_messages(messages), tools, reasoning)
            .await
    }

    async fn complete_streaming_with_tools(
        &self,
        messages: &[ConversationMessage],
        tools: &[ToolDefinition],
        reasoning: &ReasoningConfig,
        sender: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
    ) -> anyhow::Result<ChatResult> {
        self.inner
            .complete_streaming_with_tools(
                &self.sanitize_messages(messages),
                tools,
                reasoning,
                sender,
            )
            .await
    }

    fn estimate_tokens(&self, text: &str) -> Option<usize> {
        self.inner.estimate_tokens(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::ChatResult;
    use std::sync::Mutex;

    /// Capture what the inner provider actually receives.
    struct CapturingProvider {
        received_prompts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl Provider for CapturingProvider {
        async fn complete(&self, prompt: &str) -> anyhow::Result<ChatResult> {
            self.received_prompts
                .lock()
                .expect("lock")
                .push(prompt.to_string());
            Ok(ChatResult {
                output_text: "ok".to_string(),
                tool_calls: vec![],
                stop_reason: None,
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    #[tokio::test]
    async fn email_is_redacted_before_reaching_provider() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("Contact alice@example.com for details")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert_eq!(prompts.len(), 1);
        assert!(
            !prompts[0].contains("alice@example.com"),
            "email should be stripped: got {:?}",
            prompts[0]
        );
        assert!(prompts[0].contains("[EMAIL_REDACTED]"));
    }

    #[tokio::test]
    async fn clean_prompt_passes_through_unchanged() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("What is the weather today?")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert_eq!(prompts[0], "What is the weather today?");
    }

    #[tokio::test]
    async fn phone_number_redacted() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("Call me at 555-123-4567 please")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert!(!prompts[0].contains("555-123-4567"));
        assert!(prompts[0].contains("[PHONE_REDACTED]"));
    }

    #[tokio::test]
    async fn ssn_redacted() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("My SSN is 123-45-6789")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert!(!prompts[0].contains("123-45-6789"));
        assert!(prompts[0].contains("[SSN_REDACTED]"));
    }

    #[tokio::test]
    async fn api_key_redacted() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("Use this key: sk-abcdefghijklmnopqrstuvwxyz1234567890")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert!(
            !prompts[0].contains("sk-abcdefghijklmnopqrst"),
            "API key should be stripped"
        );
        assert!(prompts[0].contains("[API_KEY_REDACTED]"));
    }

    #[tokio::test]
    async fn multiple_pii_types_in_one_prompt() {
        let inner = Arc::new(CapturingProvider {
            received_prompts: Mutex::new(Vec::new()),
        });
        let provider = PrivacyFirstLayer.wrap(inner.clone());

        provider
            .complete("Email alice@corp.com, call 555-111-2222, SSN 999-88-7777")
            .await
            .expect("complete");

        let prompts = inner.received_prompts.lock().expect("lock");
        assert!(!prompts[0].contains("alice@corp.com"));
        assert!(!prompts[0].contains("555-111-2222"));
        assert!(!prompts[0].contains("999-88-7777"));
    }

    #[tokio::test]
    async fn conversation_messages_are_sanitized() {
        let provider = PrivacyFirstProvider {
            inner: Arc::new(CapturingProvider {
                received_prompts: Mutex::new(Vec::new()),
            }),
            guard: PiiRedactionGuard::default(),
            redactions: Arc::new(AtomicU64::new(0)),
        };

        let messages = vec![
            ConversationMessage::user("My email is bob@secret.io".to_string()),
            ConversationMessage::ToolResult(ToolResultMessage {
                tool_use_id: "t1".to_string(),
                content: "Found user jane@internal.co in DB".to_string(),
                is_error: false,
            }),
        ];

        let sanitized = provider.sanitize_messages(&messages);
        match &sanitized[0] {
            ConversationMessage::User { content, .. } => {
                assert!(!content.contains("bob@secret.io"));
                assert!(content.contains("[EMAIL_REDACTED]"));
            }
            other => panic!("expected User, got {other:?}"),
        }
        match &sanitized[1] {
            ConversationMessage::ToolResult(tr) => {
                assert!(!tr.content.contains("jane@internal.co"));
                assert!(tr.content.contains("[EMAIL_REDACTED]"));
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }

    #[test]
    fn redaction_counter_increments() {
        let provider = PrivacyFirstProvider {
            inner: Arc::new(CapturingProvider {
                received_prompts: Mutex::new(Vec::new()),
            }),
            guard: PiiRedactionGuard::default(),
            redactions: Arc::new(AtomicU64::new(0)),
        };

        provider.sanitize("user@test.com and 123-45-6789");
        assert_eq!(provider.redactions.load(Ordering::Relaxed), 1);
    }
}

//! Outbound message processing — applies security filters before sending.
//!
//! Currently applies the leak guard to scan and redact/block credential leaks
//! in outbound channel messages.

use crate::leak_guard::LeakGuardPolicy;
use crate::SendMessage;

/// Result of processing an outbound message through the security pipeline.
#[derive(Debug, Clone)]
pub enum OutboundResult {
    /// Message is safe to send (possibly with redacted content).
    Send(SendMessage),
    /// Message was blocked by a security filter.
    Blocked { reason: String },
}

/// Process an outbound message through the leak guard.
///
/// Returns `OutboundResult::Send` with potentially redacted content,
/// or `OutboundResult::Blocked` if the leak guard action is "block".
pub fn process_outbound(msg: SendMessage, guard: &LeakGuardPolicy) -> OutboundResult {
    match guard.process(&msg.content) {
        Ok(processed_content) => {
            if processed_content == msg.content {
                OutboundResult::Send(msg)
            } else {
                tracing::info!("leak guard redacted content in outbound message");
                OutboundResult::Send(SendMessage {
                    content: processed_content,
                    ..msg
                })
            }
        }
        Err(reason) => {
            tracing::warn!(reason = %reason, "leak guard blocked outbound message");
            OutboundResult::Blocked { reason }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leak_guard::{LeakAction, LeakGuardPolicy};

    fn msg(content: &str) -> SendMessage {
        SendMessage::new(content, "user-1")
    }

    #[test]
    fn clean_message_passes_through() {
        let guard = LeakGuardPolicy::default();
        let result = process_outbound(msg("Hello, how can I help?"), &guard);
        match result {
            OutboundResult::Send(m) => assert_eq!(m.content, "Hello, how can I help?"),
            OutboundResult::Blocked { .. } => panic!("should not be blocked"),
        }
    }

    #[test]
    fn leaked_key_gets_redacted() {
        let guard = LeakGuardPolicy {
            enabled: true,
            action: LeakAction::Redact,
            sensitivity: 0.7,
            extra_patterns: Vec::new(),
        };
        let result = process_outbound(
            msg("Here is the key: sk-abc123def456ghi789jkl012mno345"),
            &guard,
        );
        match result {
            OutboundResult::Send(m) => {
                assert!(m.content.contains("[REDACTED:"));
                assert!(!m.content.contains("sk-abc123"));
            }
            OutboundResult::Blocked { .. } => panic!("should be redacted, not blocked"),
        }
    }

    #[test]
    fn leaked_key_gets_blocked() {
        let guard = LeakGuardPolicy {
            enabled: true,
            action: LeakAction::Block,
            sensitivity: 0.7,
            extra_patterns: Vec::new(),
        };
        let result = process_outbound(
            msg("Here is the key: sk-abc123def456ghi789jkl012mno345"),
            &guard,
        );
        match result {
            OutboundResult::Blocked { reason } => {
                assert!(reason.contains("blocked"));
            }
            OutboundResult::Send(_) => panic!("should be blocked"),
        }
    }

    #[test]
    fn disabled_guard_passes_everything() {
        let guard = LeakGuardPolicy {
            enabled: false,
            action: LeakAction::Block,
            sensitivity: 0.7,
            extra_patterns: Vec::new(),
        };
        let result = process_outbound(msg("sk-abc123def456ghi789jkl012mno345"), &guard);
        match result {
            OutboundResult::Send(m) => {
                assert!(m.content.contains("sk-abc123"));
            }
            OutboundResult::Blocked { .. } => panic!("disabled guard should not block"),
        }
    }

    #[test]
    fn preserves_message_metadata() {
        let guard = LeakGuardPolicy::default();
        let original = SendMessage::with_subject("Hello", "user-1", "Subject")
            .in_thread(Some("thread-1".into()));
        let result = process_outbound(original, &guard);
        match result {
            OutboundResult::Send(m) => {
                assert_eq!(m.recipient, "user-1");
                assert_eq!(m.subject.as_deref(), Some("Subject"));
                assert_eq!(m.thread_ts.as_deref(), Some("thread-1"));
            }
            OutboundResult::Blocked { .. } => panic!("should not be blocked"),
        }
    }
}

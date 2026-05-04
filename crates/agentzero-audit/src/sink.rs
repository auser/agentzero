use std::sync::Mutex;

use agentzero_core::AuditEvent;

/// Trait for audit event sinks.
///
/// Implementations write audit events to their backing store.
pub trait AuditSink: Send + Sync {
    fn record(&self, event: &AuditEvent) -> Result<(), String>;
}

/// In-memory audit sink for testing.
///
/// Stores events in a `Vec` behind a `Mutex` for thread-safe test access.
pub struct InMemorySink {
    events: Mutex<Vec<AuditEvent>>,
}

impl InMemorySink {
    pub fn new() -> Self {
        Self {
            events: Mutex::new(Vec::new()),
        }
    }

    pub fn events(&self) -> Vec<AuditEvent> {
        self.events
            .lock()
            .expect("InMemorySink lock should not be poisoned")
            .clone()
    }

    pub fn len(&self) -> usize {
        self.events
            .lock()
            .expect("InMemorySink lock should not be poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for InMemorySink {
    fn default() -> Self {
        Self::new()
    }
}

impl AuditSink for InMemorySink {
    fn record(&self, event: &AuditEvent) -> Result<(), String> {
        self.events
            .lock()
            .expect("InMemorySink lock should not be poisoned")
            .push(event.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentzero_core::{
        Capability, DataClassification, ExecutionId, PolicyDecision, RuntimeTier, SessionId,
    };
    use chrono::Utc;

    fn sample_event() -> AuditEvent {
        AuditEvent {
            execution_id: ExecutionId::new(),
            session_id: SessionId::from_string("test-session"),
            timestamp: Utc::now(),
            action: "test_action".into(),
            capability: Capability::FileRead,
            classification: DataClassification::Private,
            decision: PolicyDecision::Allow,
            reason: "test".into(),
            runtime: RuntimeTier::HostReadonly,
            skill_id: None,
            tool_id: None,
            redactions_applied: vec![],
            approval_scope: None,
        }
    }

    #[test]
    fn in_memory_sink_starts_empty() {
        let sink = InMemorySink::new();
        assert!(sink.is_empty());
        assert_eq!(sink.len(), 0);
    }

    #[test]
    fn in_memory_sink_records_events() {
        let sink = InMemorySink::new();
        sink.record(&sample_event())
            .expect("in-memory record should succeed");
        sink.record(&sample_event())
            .expect("in-memory record should succeed");
        assert_eq!(sink.len(), 2);
    }

    #[test]
    fn in_memory_sink_returns_events() {
        let sink = InMemorySink::new();
        sink.record(&sample_event())
            .expect("in-memory record should succeed");
        let events = sink.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "test_action");
    }
}

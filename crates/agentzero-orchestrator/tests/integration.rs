//! Integration tests for the orchestrator coordinator.
//!
//! These tests exercise the full coordinator lifecycle: agent registration,
//! event bus routing, chaining, pipeline execution, privacy enforcement,
//! and graceful shutdown — all with mock providers (no real LLM needed).

use agentzero_channels::{Channel, ChannelMessage, ChannelRegistry, SendMessage};
use agentzero_core::event_bus::{Event, EventBus, InMemoryBus};
use agentzero_core::{Agent, AgentConfig};
use agentzero_orchestrator::{AgentDescriptor, AgentRouter, Coordinator};
use agentzero_testkit::{StaticProvider, TestMemoryStore};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};

// ─── Test Channel ────────────────────────────────────────────────────────────

/// A test channel that captures sent messages for assertion.
struct TestChannel {
    name: String,
    sent: Arc<Mutex<Vec<SendMessage>>>,
}

impl TestChannel {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            sent: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn sent_messages(&self) -> Arc<Mutex<Vec<SendMessage>>> {
        self.sent.clone()
    }
}

#[async_trait]
impl Channel for TestChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        self.sent.lock().await.push(SendMessage {
            content: message.content.clone(),
            recipient: message.recipient.clone(),
            subject: message.subject.clone(),
            thread_ts: message.thread_ts.clone(),
        });
        Ok(())
    }

    async fn listen(&self, _tx: mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        // Test channel doesn't produce inbound messages.
        // Hold open until dropped.
        std::future::pending::<()>().await;
        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_agent(response: &str) -> Agent {
    Agent::new(
        AgentConfig {
            max_tool_iterations: 1,
            ..Default::default()
        },
        Box::new(StaticProvider {
            output_text: response.to_string(),
        }),
        Box::new(TestMemoryStore::default()),
        vec![],
    )
}

fn make_descriptor(
    id: &str,
    subscribes_to: Vec<&str>,
    produces: Vec<&str>,
    boundary: &str,
) -> AgentDescriptor {
    AgentDescriptor {
        id: id.to_string(),
        name: id.to_string(),
        description: format!("Agent {id}"),
        keywords: vec![id.to_string()],
        subscribes_to: subscribes_to.into_iter().map(String::from).collect(),
        produces: produces.into_iter().map(String::from).collect(),
        privacy_boundary: boundary.to_string(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_chain_a_b_c_dispatches_to_channel() {
    // A produces "task.a.done", B subscribes to "task.a.*" and produces "task.b.done",
    // C subscribes to "task.b.*" and produces "task.c.done".
    // Nobody subscribes to "task.c.*", so the terminal event dispatches to the
    // originating channel via correlation_id.

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
    let test_channel = TestChannel::new("test");
    let sent = test_channel.sent_messages();

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![], 100);

    coord.register_agent(
        make_descriptor(
            "agent-a",
            vec!["channel.*.message"],
            vec!["task.a.done"],
            "any",
        ),
        make_agent("result-from-A"),
        "/tmp".to_string(),
    );
    coord.register_agent(
        make_descriptor("agent-b", vec!["task.a.*"], vec!["task.b.done"], "any"),
        make_agent("result-from-B"),
        "/tmp".to_string(),
    );
    coord.register_agent(
        make_descriptor("agent-c", vec!["task.b.*"], vec!["task.c.done"], "any"),
        make_agent("result-from-C"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    // Give coordinator time to start its loops and subscribe.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Simulate a channel message arriving on the bus with a correlation_id.
    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"agent-a do something","sender":"user","reply_target":"user-123","channel":"test"}"#,
    )
    .with_correlation("corr-chain-test")
    .with_boundary("any");

    bus.publish(event).await.unwrap();

    // Wait for the chain to complete (A→B→C→channel reply).
    // With StaticProvider the agent responds immediately, but we need
    // time for the async loops to process each hop.
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    let messages = sent.lock().await;
    assert!(
        !messages.is_empty(),
        "expected at least one message dispatched to channel, got none"
    );
    // The terminal message should contain agent-C's output.
    assert_eq!(messages.last().unwrap().content, "result-from-C");
}

#[tokio::test]
async fn privacy_boundary_blocks_incompatible_routing() {
    // A local_only event should NOT be routed to an "any" agent.

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
    let test_channel = TestChannel::new("test");
    let sent = test_channel.sent_messages();

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![], 100);

    // This agent has "any" boundary — should NOT receive "local_only" events.
    coord.register_agent(
        make_descriptor(
            "agent-any",
            vec!["channel.*.message"],
            vec!["task.done"],
            "any",
        ),
        make_agent("should-not-see-this"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Publish a local_only message — the "any" agent should NOT pick it up.
    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"agent-any secret","sender":"user","reply_target":"user-1","channel":"test"}"#,
    )
    .with_correlation("corr-privacy-test")
    .with_boundary("local_only");

    bus.publish(event).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    let messages = sent.lock().await;
    assert!(
        messages.is_empty(),
        "local_only event should NOT have been dispatched to 'any' agent, but got {} messages",
        messages.len()
    );
}

#[tokio::test]
async fn privacy_boundary_allows_compatible_routing() {
    // A local_only event SHOULD be routed to a local_only agent.

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
    let test_channel = TestChannel::new("test");
    let sent = test_channel.sent_messages();

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![], 100);

    coord.register_agent(
        make_descriptor(
            "agent-local",
            vec!["channel.*.message"],
            vec![],
            "local_only",
        ),
        make_agent("local-response"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"agent-local secret","sender":"user","reply_target":"user-1","channel":"test"}"#,
    )
    .with_correlation("corr-compat-test")
    .with_boundary("local_only");

    bus.publish(event).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    let messages = sent.lock().await;
    assert!(
        !messages.is_empty(),
        "local_only event should have been routed to local_only agent"
    );
    assert_eq!(messages[0].content, "local-response");
}

#[tokio::test]
async fn pipeline_executes_sequential_steps() {
    use agentzero_config::PipelineConfig;

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
    let test_channel = TestChannel::new("test");
    let sent = test_channel.sent_messages();

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let pipeline = PipelineConfig {
        name: "translate-summarize".to_string(),
        trigger: agentzero_config::PipelineTriggerConfig {
            keywords: vec!["pipeline".to_string()],
            regex: String::new(),
            topic: String::new(),
            ai_classified: String::new(),
        },
        steps: vec!["step-1".to_string(), "step-2".to_string()],
        channel_reply: true,
        on_step_error: "abort".to_string(),
        max_retries: 1,
        step_timeout_secs: 10,
    };

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![pipeline], 100);

    coord.register_agent(
        make_descriptor("step-1", vec![], vec![], "any"),
        make_agent("step-1-output"),
        "/tmp".to_string(),
    );
    coord.register_agent(
        make_descriptor("step-2", vec![], vec![], "any"),
        make_agent("step-2-final"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"pipeline please translate and summarize","sender":"user","reply_target":"user-1","channel":"test"}"#,
    )
    .with_correlation("corr-pipeline-test")
    .with_boundary("any");

    bus.publish(event).await.unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    let messages = sent.lock().await;
    assert!(
        !messages.is_empty(),
        "pipeline should have dispatched a reply to channel"
    );
    // The pipeline's channel_reply sends step-2's output to the channel.
    // Individual agent workers may also publish completion events that the
    // response handler dispatches, so we check that at least one message
    // contains the final pipeline output.
    let has_final = messages.iter().any(|m| m.content == "step-2-final");
    assert!(
        has_final,
        "expected pipeline final output 'step-2-final' in channel messages, got: {:?}",
        messages.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn graceful_shutdown_completes_in_flight() {
    // Start coordinator, dispatch work, immediately send shutdown.
    // The coordinator should still process the in-flight task within the grace period.

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));
    let test_channel = TestChannel::new("test");
    let sent = test_channel.sent_messages();

    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![], 3000);

    coord.register_agent(
        make_descriptor("worker", vec!["channel.*.message"], vec![], "any"),
        make_agent("graceful-result"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Dispatch work.
    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"worker task","sender":"user","reply_target":"user-1","channel":"test"}"#,
    )
    .with_correlation("corr-shutdown-test")
    .with_boundary("any");

    bus.publish(event).await.unwrap();

    // Give some time for the agent to pick up the task.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Send shutdown — the grace period (3000ms) should allow in-flight to finish.
    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    let messages = sent.lock().await;
    assert!(
        !messages.is_empty(),
        "in-flight task should have completed during grace period"
    );
}

#[tokio::test]
async fn correlation_id_traces_full_chain() {
    // Verify that the correlation_id from the original channel message
    // is preserved through the chain and appears in the terminal dispatch.

    let bus: Arc<dyn EventBus> = Arc::new(InMemoryBus::new(64));

    // Subscribe to all events to capture correlation IDs.
    let mut spy = bus.subscribe();

    let test_channel = TestChannel::new("test");
    let mut registry = ChannelRegistry::new();
    registry.register(Arc::new(test_channel));
    let channels = Arc::new(registry);

    let router = AgentRouter::keywords_only();

    let mut coord = Coordinator::new(bus.clone(), channels.clone(), router, vec![], 100);

    coord.register_agent(
        make_descriptor(
            "alpha",
            vec!["channel.*.message"],
            vec!["task.alpha.done"],
            "any",
        ),
        make_agent("alpha-output"),
        "/tmp".to_string(),
    );

    let (shutdown_tx, _) = watch::channel(false);
    let shutdown_rx = shutdown_tx.subscribe();

    let coord_handle = tokio::spawn(async move { coord.run(shutdown_rx).await });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let event = Event::new(
        "channel.test.message",
        "channel.test",
        r#"{"content":"alpha do work","sender":"user","reply_target":"user-1","channel":"test"}"#,
    )
    .with_correlation("corr-trace-test")
    .with_boundary("any");

    bus.publish(event).await.unwrap();

    // Collect events from the spy for up to 2 seconds.
    let mut captured = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(2000);
    loop {
        tokio::select! {
            result = spy.recv() => {
                if let Ok(evt) = result {
                    captured.push(evt);
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }

    shutdown_tx.send(true).unwrap();
    let _ = coord_handle.await;

    // Find the agent output event.
    let agent_output = captured
        .iter()
        .find(|e| e.topic == "task.alpha.done")
        .expect("should have captured agent output event");

    assert_eq!(
        agent_output.correlation_id.as_deref(),
        Some("corr-trace-test"),
        "agent output should carry the original correlation_id"
    );
}

//! Gateway coordinator for multi-agent orchestration.
//!
//! The coordinator runs three concurrent loops:
//! 1. **Channel ingestion** — spawns `channel.listen()` for pull-based channels,
//!    publishes inbound messages to the event bus.
//! 2. **Router** — subscribes to `channel.*.message` events, uses an AI router
//!    to classify and dispatch to the best agent.
//! 3. **Response/chain handler** — subscribes to agent output events, either
//!    chains them to subscribing agents or dispatches to originating channels.
//!
//! Each agent runs in its own `tokio::spawn` task, receiving work via an mpsc
//! channel and publishing results back on the event bus.

use crate::agent_router::{AgentDescriptor, AgentRouter};
use crate::presence::PresenceStore;
use agentzero_channels::{ChannelMessage, ChannelRegistry, SendMessage};
use agentzero_config::PipelineConfig;
use agentzero_core::event_bus::{is_boundary_compatible, topic_matches, Event, EventBus};
use agentzero_core::{Agent, AnnounceMessage, JobStatus, RunId, ToolContext};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Status codes for agent workers.
const STATUS_IDLE: u8 = 0;
const STATUS_BUSY: u8 = 1;
const STATUS_STOPPED: u8 = 2;

/// A task dispatched to an agent worker.
pub struct TaskMessage {
    pub event: Event,
    pub correlation_id: String,
    /// If set, the worker sends the result back through this channel
    /// (used by pipeline executor for synchronous step execution).
    pub result_tx: Option<oneshot::Sender<TaskResult>>,
    /// Cancellation flag — set to `true` to signal the agent to stop.
    pub cancelled: Arc<std::sync::atomic::AtomicBool>,
}

pub struct TaskResult {
    pub payload: String,
    pub source: String,
}

/// Tracks the origin of a correlation chain (which channel + reply target).
#[derive(Clone)]
struct CorrelationOrigin {
    channel: String,
    reply_target: String,
}

/// Per-agent worker handle.
#[allow(dead_code)]
struct AgentWorker {
    id: String,
    descriptor: AgentDescriptor,
    task_tx: mpsc::Sender<TaskMessage>,
    join_handle: JoinHandle<()>,
    status: Arc<AtomicU8>,
}

/// Error strategies for pipeline steps.
#[derive(Debug, Clone, Copy)]
pub enum ErrorStrategy {
    Abort,
    Skip,
    Retry { max_attempts: u8 },
}

impl ErrorStrategy {
    fn from_config(s: &str, max_retries: u8) -> Self {
        match s {
            "skip" => Self::Skip,
            "retry" => Self::Retry {
                max_attempts: max_retries,
            },
            _ => Self::Abort,
        }
    }
}

// ─── Coordinator ─────────────────────────────────────────────────────────────

pub struct Coordinator {
    bus: Arc<dyn EventBus>,
    agents: Arc<tokio::sync::RwLock<HashMap<String, AgentWorker>>>,
    channels: Arc<ChannelRegistry>,
    router: AgentRouter,
    pipelines: Vec<PipelineConfig>,
    /// Maps correlation_id → origin channel info.
    correlation_store: Arc<tokio::sync::Mutex<HashMap<String, CorrelationOrigin>>>,
    shutdown_grace_ms: u64,
    /// Optional presence store for agent heartbeat tracking.
    presence: Option<Arc<PresenceStore>>,
}

impl Coordinator {
    /// Create a new coordinator (does not start it — call `run()`).
    pub fn new(
        bus: Arc<dyn EventBus>,
        channels: Arc<ChannelRegistry>,
        router: AgentRouter,
        pipelines: Vec<PipelineConfig>,
        shutdown_grace_ms: u64,
    ) -> Self {
        Self {
            bus,
            agents: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            channels,
            router,
            pipelines,
            correlation_store: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            shutdown_grace_ms,
            presence: None,
        }
    }

    /// Set the presence store for agent heartbeat tracking.
    pub fn with_presence(mut self, store: Arc<PresenceStore>) -> Self {
        self.presence = Some(store);
        self
    }

    /// Register an agent worker. The coordinator owns the agent's task channel
    /// and join handle.
    pub async fn register_agent(
        &mut self,
        descriptor: AgentDescriptor,
        agent: Agent,
        workspace_root: String,
    ) {
        let (task_tx, task_rx) = mpsc::channel::<TaskMessage>(32);
        let status = Arc::new(AtomicU8::new(STATUS_IDLE));
        let id = descriptor.id.clone();

        let bus = self.bus.clone();
        let desc = descriptor.clone();
        let worker_status = status.clone();
        let presence = self.presence.clone();

        let join_handle = tokio::spawn(agent_worker(
            agent,
            task_rx,
            bus,
            desc,
            worker_status,
            workspace_root,
            presence,
        ));

        self.agents.write().await.insert(
            id.clone(),
            AgentWorker {
                id,
                descriptor,
                task_tx,
                join_handle,
                status,
            },
        );
    }

    /// Register an agent worker with a pre-created task channel.
    ///
    /// This variant is used by the swarm builder when it needs the `task_tx`
    /// before registration (e.g. to wire up `ConverseTool` endpoints).
    pub async fn register_agent_with_rx(
        &mut self,
        descriptor: AgentDescriptor,
        agent: Agent,
        workspace_root: String,
        task_tx: mpsc::Sender<TaskMessage>,
        task_rx: mpsc::Receiver<TaskMessage>,
    ) {
        let status = Arc::new(AtomicU8::new(STATUS_IDLE));
        let id = descriptor.id.clone();

        let bus = self.bus.clone();
        let desc = descriptor.clone();
        let worker_status = status.clone();
        let presence = self.presence.clone();

        let join_handle = tokio::spawn(agent_worker(
            agent,
            task_rx,
            bus,
            desc,
            worker_status,
            workspace_root,
            presence,
        ));

        self.agents.write().await.insert(
            id.clone(),
            AgentWorker {
                id,
                descriptor,
                task_tx,
                join_handle,
                status,
            },
        );
    }

    /// Register a dynamic agent at runtime (after `run()` has been called).
    ///
    /// Unlike `register_agent()`, this method works on a running coordinator
    /// via the shared agents map. The agent worker is spawned immediately.
    pub async fn register_dynamic_agent(
        &self,
        descriptor: AgentDescriptor,
        agent: Agent,
        workspace_root: String,
    ) {
        let (task_tx, task_rx) = mpsc::channel::<TaskMessage>(32);
        let status = Arc::new(AtomicU8::new(STATUS_IDLE));
        let id = descriptor.id.clone();

        let bus = self.bus.clone();
        let desc = descriptor.clone();
        let worker_status = status.clone();
        let presence = self.presence.clone();

        let join_handle = tokio::spawn(agent_worker(
            agent,
            task_rx,
            bus,
            desc,
            worker_status,
            workspace_root,
            presence,
        ));

        tracing::info!(agent_id = %id, "dynamic agent registered");

        self.agents.write().await.insert(
            id.clone(),
            AgentWorker {
                id,
                descriptor,
                task_tx,
                join_handle,
                status,
            },
        );
    }

    /// Deregister a dynamic agent at runtime.
    ///
    /// Drops the task sender, causing the agent worker to shut down gracefully.
    /// Returns `true` if the agent was found and removed.
    pub async fn deregister_agent(&self, agent_id: &str) -> bool {
        let worker = self.agents.write().await.remove(agent_id);
        if let Some(w) = worker {
            // Drop the sender — this causes `task_rx.recv()` to return `None`,
            // which triggers the worker's graceful shutdown path.
            drop(w.task_tx);
            // Abort the join handle if the worker is stuck.
            w.join_handle.abort();

            tracing::info!(agent_id = %agent_id, "dynamic agent deregistered");
            true
        } else {
            false
        }
    }

    /// Check if an agent is registered and alive.
    pub async fn is_agent_registered(&self, agent_id: &str) -> bool {
        let agents = self.agents.read().await;
        if let Some(worker) = agents.get(agent_id) {
            worker.status.load(Ordering::Relaxed) != STATUS_STOPPED
        } else {
            false
        }
    }

    /// Descriptors for all registered agents (used by the router).
    async fn agent_descriptors(&self) -> Vec<AgentDescriptor> {
        self.agents
            .read()
            .await
            .values()
            .map(|w| w.descriptor.clone())
            .collect()
    }

    /// Run the coordinator until shutdown signal.
    pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) -> anyhow::Result<()> {
        let coord = Arc::new(self);

        // Spawn the three concurrent loops.
        let c1 = coord.clone();
        let mut s1 = shutdown.clone();
        let ingestion = tokio::spawn(async move { c1.run_channel_ingestion(&mut s1).await });

        let c2 = coord.clone();
        let mut s2 = shutdown.clone();
        let router_loop = tokio::spawn(async move { c2.run_router(&mut s2).await });

        let c3 = coord.clone();
        let mut s3 = shutdown.clone();
        let response_loop = tokio::spawn(async move { c3.run_response_handler(&mut s3).await });

        // Wait for shutdown signal or any loop to exit.
        tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("coordinator received shutdown signal");
            }
            r = ingestion => {
                if let Err(e) = r { tracing::error!(error = %e, "channel ingestion loop panicked"); }
            }
            r = router_loop => {
                if let Err(e) = r { tracing::error!(error = %e, "router loop panicked"); }
            }
            r = response_loop => {
                if let Err(e) = r { tracing::error!(error = %e, "response handler loop panicked"); }
            }
        }

        // Graceful shutdown: give in-flight tasks time to complete.
        tracing::info!(
            grace_ms = coord.shutdown_grace_ms,
            "coordinator shutting down, waiting for in-flight tasks"
        );
        tokio::time::sleep(Duration::from_millis(coord.shutdown_grace_ms)).await;

        Ok(())
    }

    // ─── Loop 1: Channel Ingestion ──────────────────────────────────────────

    async fn run_channel_ingestion(
        &self,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        // For each channel that supports listen(), spawn a listener task
        // that publishes inbound messages to the bus.
        let channels = self.channels.all_channels();
        let mut listener_handles = Vec::new();

        for channel in channels {
            let name = channel.name().to_string();
            let bus = self.bus.clone();
            let (tx, mut rx) = mpsc::channel::<ChannelMessage>(64);

            // Spawn the channel listener
            let ch = channel.clone();
            let listen_handle = tokio::spawn(async move {
                if let Err(e) = ch.listen(tx).await {
                    tracing::warn!(channel = %ch.name(), error = %e, "channel listener exited");
                }
            });

            // Spawn a relay that publishes received messages to the bus
            let relay_handle = tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    let correlation_id = uuid_v4();
                    let event = Event::new(
                        format!("channel.{}.message", name),
                        format!("channel.{}", name),
                        serde_json::to_string(&serde_json::json!({
                            "sender": msg.sender,
                            "reply_target": msg.reply_target,
                            "content": msg.content,
                            "channel": msg.channel,
                            "thread_ts": msg.thread_ts,
                        }))
                        .unwrap_or_default(),
                    )
                    .with_correlation(correlation_id)
                    .with_boundary(&msg.privacy_boundary);

                    if let Err(e) = bus.publish(event).await {
                        tracing::error!(error = %e, "failed to publish channel message to bus");
                    }
                }
            });

            listener_handles.push(listen_handle);
            listener_handles.push(relay_handle);
        }

        // Wait for shutdown
        let _ = shutdown.changed().await;
        // Drop handles — listeners will stop on their own when channels close.
        for handle in listener_handles {
            handle.abort();
        }
        Ok(())
    }

    // ─── Loop 2: AI Router ──────────────────────────────────────────────────

    async fn run_router(
        &self,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut sub = self.bus.subscribe();

        loop {
            let event = tokio::select! {
                e = sub.recv_filtered("channel.") => match e {
                    Ok(event) => event,
                    Err(e) => {
                        tracing::error!(error = %e, "router bus subscription failed");
                        break;
                    }
                },
                _ = shutdown.changed() => break,
            };

            // Only route channel messages (not channel system events).
            if !event.topic.ends_with(".message") {
                continue;
            }

            let correlation_id = event.correlation_id.clone().unwrap_or_else(uuid_v4);

            // Extract origin info for later channel reply.
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload) {
                let origin = CorrelationOrigin {
                    channel: payload["channel"].as_str().unwrap_or_default().to_string(),
                    reply_target: payload["reply_target"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                };
                self.correlation_store
                    .lock()
                    .await
                    .insert(correlation_id.clone(), origin);
            }

            let content = extract_content(&event.payload);

            // Check pipelines first
            if let Some(pipeline) = self.match_pipeline(&content) {
                tracing::info!(pipeline = %pipeline.name, "pipeline matched");
                let bus = self.bus.clone();
                let corr_store = self.correlation_store.clone();
                let channels = self.channels.clone();
                let agents = self.collect_agent_senders().await;
                let pipeline = pipeline.clone();
                let step_timeout = pipeline.step_timeout_secs;
                let error_strategy =
                    ErrorStrategy::from_config(&pipeline.on_step_error, pipeline.max_retries);

                tokio::spawn(async move {
                    if let Err(e) = execute_pipeline(
                        &pipeline,
                        event,
                        &agents,
                        &bus,
                        &corr_store,
                        &channels,
                        error_strategy,
                        step_timeout,
                    )
                    .await
                    {
                        tracing::error!(pipeline = %pipeline.name, error = %e, "pipeline failed");
                    }
                });
                continue;
            }

            // AI/keyword routing — filter out dead agents from candidates.
            let mut descriptors = self.agent_descriptors().await;
            if let Some(ref ps) = self.presence {
                let mut alive_descriptors = Vec::with_capacity(descriptors.len());
                for d in descriptors {
                    if ps.is_alive(&d.id).await {
                        alive_descriptors.push(d);
                    } else {
                        tracing::debug!(agent = %d.id, "skipping dead agent in routing");
                    }
                }
                descriptors = alive_descriptors;
            }
            match self.router.route(&content, &descriptors).await {
                Ok(Some(agent_id)) => {
                    let agents = self.agents.read().await;
                    if let Some(worker) = agents.get(&agent_id) {
                        // Privacy check
                        if !is_boundary_compatible(
                            &event.privacy_boundary,
                            &worker.descriptor.privacy_boundary,
                        ) {
                            tracing::warn!(
                                agent = %agent_id,
                                event_boundary = %event.privacy_boundary,
                                agent_boundary = %worker.descriptor.privacy_boundary,
                                "privacy boundary mismatch, skipping"
                            );
                            continue;
                        }

                        let _ = worker
                            .task_tx
                            .send(TaskMessage {
                                event,
                                correlation_id,
                                result_tx: None,
                                cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                            })
                            .await;
                    }
                }
                Ok(None) => {
                    tracing::debug!(content = %content, "no agent matched for message");
                }
                Err(e) => {
                    tracing::error!(error = %e, "routing failed");
                }
            }
        }

        Ok(())
    }

    // ─── Loop 3: Response / Chain Handler ───────────────────────────────────

    async fn run_response_handler(
        &self,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<()> {
        let mut sub = self.bus.subscribe();

        loop {
            let event = tokio::select! {
                e = sub.recv() => match e {
                    Ok(event) => event,
                    Err(e) => {
                        tracing::error!(error = %e, "response handler bus subscription failed");
                        break;
                    }
                },
                _ = shutdown.changed() => break,
            };

            // Skip channel messages (handled by router) and system events.
            if event.topic.starts_with("channel.") || event.topic.starts_with("system.") {
                continue;
            }
            // Skip IPC messages (handled by IPC tool directly).
            if event.topic.starts_with("ipc.") {
                continue;
            }
            // Skip pipeline step events (internal observability, handled by executor).
            if event.topic.starts_with("pipeline.") {
                continue;
            }

            // Handle announce events: dispatch summary to parent agent or channel.
            if event.topic.ends_with(".announce") {
                if let Ok(announce) = serde_json::from_str::<AnnounceMessage>(&event.payload) {
                    tracing::info!(
                        agent = %announce.agent_id,
                        run_id = %announce.run_id,
                        parent = ?announce.parent_run_id,
                        depth = announce.depth,
                        "sub-agent announce received"
                    );

                    // Publish a synthesized event on the parent's topic so the
                    // response handler's normal chain logic can pick it up.
                    let parent_topic = "agent.announce.summary".to_string();
                    let summary_event = Event::new(
                        &parent_topic,
                        &announce.agent_id,
                        serde_json::to_string(&announce).unwrap_or_default(),
                    )
                    .with_correlation(event.correlation_id.clone().unwrap_or_default())
                    .with_boundary(&event.privacy_boundary);

                    // Try to dispatch to the originating channel via correlation.
                    if let Some(ref corr_id) = event.correlation_id {
                        let store = self.correlation_store.lock().await;
                        if let Some(origin) = store.get(corr_id) {
                            let channel = origin.channel.clone();
                            let reply_target = origin.reply_target.clone();
                            drop(store);

                            if let Some(ch) = self.channels.get(&channel) {
                                let announce_text = format!(
                                    "[Sub-agent {} completed]: {}",
                                    announce.agent_id, announce.summary
                                );
                                let _ = ch
                                    .send(&SendMessage {
                                        recipient: reply_target,
                                        content: announce_text,
                                        subject: None,
                                        thread_ts: None,
                                    })
                                    .await;
                            }
                        }
                    }

                    // Also publish the summary event so subscribed agents can chain.
                    let _ = self.bus.publish(summary_event).await;
                }
                continue;
            }

            // 1. Check if any agent subscribes to this topic → CHAIN
            let mut routed = false;
            let agents = self.agents.read().await;
            for worker in agents.values() {
                let matches = worker
                    .descriptor
                    .subscribes_to
                    .iter()
                    .any(|pattern| topic_matches(pattern, &event.topic));
                if matches {
                    // Privacy check
                    if !is_boundary_compatible(
                        &event.privacy_boundary,
                        &worker.descriptor.privacy_boundary,
                    ) {
                        continue;
                    }

                    let correlation_id = event.correlation_id.clone().unwrap_or_default();

                    let _ = worker
                        .task_tx
                        .send(TaskMessage {
                            event: event.clone(),
                            correlation_id,
                            result_tx: None,
                            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                        })
                        .await;
                    routed = true;
                }
            }

            // 2. If nobody subscribed AND this has a correlation_id → terminal?
            if !routed {
                if let Some(ref corr_id) = event.correlation_id {
                    let store = self.correlation_store.lock().await;
                    if let Some(origin) = store.get(corr_id) {
                        let channel = origin.channel.clone();
                        let reply_target = origin.reply_target.clone();
                        drop(store);

                        tracing::info!(
                            correlation_id = %corr_id,
                            channel = %channel,
                            "terminal event, dispatching to channel"
                        );

                        if let Some(ch) = self.channels.get(&channel) {
                            let _ = ch
                                .send(&SendMessage {
                                    recipient: reply_target,
                                    content: event.payload.clone(),
                                    subject: None,
                                    thread_ts: None,
                                })
                                .await;
                        }

                        // Clean up correlation entry.
                        self.correlation_store.lock().await.remove(corr_id);
                    }
                }
            }
        }

        Ok(())
    }

    // ─── Helpers ────────────────────────────────────────────────────────────

    fn match_pipeline(&self, content: &str) -> Option<&PipelineConfig> {
        let lower = content.to_lowercase();
        self.pipelines.iter().find(|p| {
            let trigger = &p.trigger;
            // Keyword match
            if !trigger.keywords.is_empty()
                && trigger
                    .keywords
                    .iter()
                    .any(|kw| lower.contains(&kw.to_lowercase()))
            {
                return true;
            }
            // Regex match
            if !trigger.regex.is_empty() {
                if let Ok(re) = regex::Regex::new(&trigger.regex) {
                    if re.is_match(content) {
                        return true;
                    }
                }
            }
            false
        })
    }

    async fn collect_agent_senders(&self) -> HashMap<String, mpsc::Sender<TaskMessage>> {
        self.agents
            .read()
            .await
            .iter()
            .map(|(id, w)| (id.clone(), w.task_tx.clone()))
            .collect()
    }
}

// ─── Agent Worker ────────────────────────────────────────────────────────────

async fn agent_worker(
    agent: Agent,
    mut task_rx: mpsc::Receiver<TaskMessage>,
    bus: Arc<dyn EventBus>,
    descriptor: AgentDescriptor,
    status: Arc<AtomicU8>,
    workspace_root: String,
    presence: Option<Arc<PresenceStore>>,
) {
    tracing::info!(agent = %descriptor.id, "agent worker started");

    // Register with presence store if available.
    if let Some(ref ps) = presence {
        ps.register(&descriptor.id, Duration::from_secs(30)).await;
    }

    while let Some(task) = task_rx.recv().await {
        status.store(STATUS_BUSY, Ordering::Relaxed);

        // Heartbeat on task start.
        if let Some(ref ps) = presence {
            ps.heartbeat(&descriptor.id).await;
        }

        let content = extract_content(&task.event.payload);
        let mut ctx = ToolContext::new(workspace_root.clone());
        ctx.event_bus = Some(bus.clone());
        ctx.agent_id = Some(descriptor.id.clone());
        ctx.privacy_boundary = descriptor.privacy_boundary.clone();
        ctx.cancelled = task.cancelled.clone();

        // Give each task a unique conversation ID so pipeline agents don't
        // see each other's tool call history in shared memory.
        ctx.conversation_id = Some(format!(
            "{}-{}-{}",
            descriptor.id,
            task.correlation_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));

        // Extract source channel from event if available.
        if task.event.topic.starts_with("channel.") {
            let parts: Vec<&str> = task.event.topic.splitn(3, '.').collect();
            if parts.len() >= 2 {
                ctx.source_channel = Some(parts[1].to_string());
            }
        }

        let user_msg = agentzero_core::UserMessage {
            text: content.clone(),
        };

        // Extract run metadata from the event payload for announce-back.
        let run_meta = extract_run_metadata(&task.event.payload);

        match agent.respond(user_msg, &ctx).await {
            Ok(response) => {
                let response_text = response.text;

                // If this is a pipeline step with a result channel, send it there.
                if let Some(result_tx) = task.result_tx {
                    let _ = result_tx.send(TaskResult {
                        payload: response_text,
                        source: descriptor.id.clone(),
                    });
                } else {
                    // Publish output on the bus for each declared topic.
                    for topic in &descriptor.produces {
                        let event = Event::new(topic, &descriptor.id, &response_text)
                            .with_correlation(task.correlation_id.clone())
                            .with_boundary(&descriptor.privacy_boundary);
                        if let Err(e) = bus.publish(event).await {
                            tracing::error!(
                                agent = %descriptor.id,
                                topic = %topic,
                                error = %e,
                                "failed to publish agent output"
                            );
                        }
                    }

                    // If no produces topics, publish a generic completion event.
                    if descriptor.produces.is_empty() {
                        let event = Event::new(
                            format!("agent.{}.complete", descriptor.id),
                            &descriptor.id,
                            &response_text,
                        )
                        .with_correlation(task.correlation_id.clone())
                        .with_boundary(&descriptor.privacy_boundary);
                        let _ = bus.publish(event).await;
                    }

                    // Announce-back: if this execution has run metadata, publish
                    // an AnnounceMessage so the parent agent is notified.
                    if let Some((run_id, parent_run_id, depth)) = &run_meta {
                        let announce = AnnounceMessage {
                            run_id: run_id.clone(),
                            agent_id: descriptor.id.clone(),
                            parent_run_id: Some(parent_run_id.clone()),
                            summary: truncate_for_announce(&response_text, 500),
                            status: JobStatus::Completed {
                                result: response_text.clone(),
                            },
                            depth: *depth,
                        };
                        let announce_event = Event::new(
                            format!("agent.{}.announce", descriptor.id),
                            &descriptor.id,
                            serde_json::to_string(&announce).unwrap_or_default(),
                        )
                        .with_correlation(task.correlation_id.clone())
                        .with_boundary(&descriptor.privacy_boundary);
                        let _ = bus.publish(announce_event).await;
                    }
                }
            }
            Err(e) => {
                tracing::error!(agent = %descriptor.id, error = %e, "agent execution failed");

                if let Some(result_tx) = task.result_tx {
                    let _ = result_tx.send(TaskResult {
                        payload: format!("Error: {e}"),
                        source: descriptor.id.clone(),
                    });
                } else {
                    let event = Event::new(
                        format!("agent.{}.error", descriptor.id),
                        &descriptor.id,
                        e.to_string(),
                    )
                    .with_correlation(task.correlation_id.clone())
                    .with_boundary(&descriptor.privacy_boundary);
                    let _ = bus.publish(event).await;

                    // Announce-back failure.
                    if let Some((run_id, parent_run_id, depth)) = &run_meta {
                        let announce = AnnounceMessage {
                            run_id: run_id.clone(),
                            agent_id: descriptor.id.clone(),
                            parent_run_id: Some(parent_run_id.clone()),
                            summary: format!("Agent failed: {e}"),
                            status: JobStatus::Failed {
                                error: e.to_string(),
                            },
                            depth: *depth,
                        };
                        let announce_event = Event::new(
                            format!("agent.{}.announce", descriptor.id),
                            &descriptor.id,
                            serde_json::to_string(&announce).unwrap_or_default(),
                        )
                        .with_correlation(task.correlation_id)
                        .with_boundary(&descriptor.privacy_boundary);
                        let _ = bus.publish(announce_event).await;
                    }
                }
            }
        }

        // Heartbeat on task completion.
        if let Some(ref ps) = presence {
            ps.heartbeat(&descriptor.id).await;
        }

        status.store(STATUS_IDLE, Ordering::Relaxed);
    }

    // Deregister from presence store on shutdown.
    if let Some(ref ps) = presence {
        ps.deregister(&descriptor.id).await;
    }

    status.store(STATUS_STOPPED, Ordering::Relaxed);
    tracing::info!(agent = %descriptor.id, "agent worker stopped");
}

// ─── Pipeline Executor ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn execute_pipeline(
    pipeline: &PipelineConfig,
    initial_event: Event,
    agents: &HashMap<String, mpsc::Sender<TaskMessage>>,
    bus: &Arc<dyn EventBus>,
    correlation_store: &Arc<tokio::sync::Mutex<HashMap<String, CorrelationOrigin>>>,
    channels: &Arc<ChannelRegistry>,
    error_strategy: ErrorStrategy,
    step_timeout_secs: u64,
) -> anyhow::Result<()> {
    let correlation_id = initial_event.correlation_id.clone().unwrap_or_else(uuid_v4);
    let mut current_payload = extract_content(&initial_event.payload);

    match pipeline.execution_mode.as_str() {
        "fanout" => {
            // Fan-out mode: run all fanout_steps groups in parallel.
            current_payload = execute_fanout_steps(
                pipeline,
                &current_payload,
                agents,
                &correlation_id,
                &initial_event.privacy_boundary,
                step_timeout_secs,
            )
            .await?;
        }
        "mixed" => {
            // Mixed mode: alternate between sequential steps and fanout steps.
            // First run sequential steps, then fanout steps.
            for (i, agent_id) in pipeline.steps.iter().enumerate() {
                current_payload = execute_single_step(
                    pipeline,
                    i,
                    agent_id,
                    &current_payload,
                    agents,
                    bus,
                    &correlation_id,
                    &initial_event.privacy_boundary,
                    error_strategy,
                    step_timeout_secs,
                )
                .await?;
            }
            if !pipeline.fanout_steps.is_empty() {
                current_payload = execute_fanout_steps(
                    pipeline,
                    &current_payload,
                    agents,
                    &correlation_id,
                    &initial_event.privacy_boundary,
                    step_timeout_secs,
                )
                .await?;
            }
        }
        _ => {
            // Default sequential mode.
        }
    }

    // Sequential steps (for "sequential" mode only).
    if pipeline.execution_mode != "fanout" && pipeline.execution_mode != "mixed" {
        for (i, agent_id) in pipeline.steps.iter().enumerate() {
            current_payload = execute_single_step(
                pipeline,
                i,
                agent_id,
                &current_payload,
                agents,
                bus,
                &correlation_id,
                &initial_event.privacy_boundary,
                error_strategy,
                step_timeout_secs,
            )
            .await?;
        }
    }

    // Pipeline complete — if channel_reply, send to originating channel.
    if pipeline.channel_reply {
        let store = correlation_store.lock().await;
        if let Some(origin) = store.get(&correlation_id) {
            let channel = origin.channel.clone();
            let reply_target = origin.reply_target.clone();
            drop(store);

            if let Some(ch) = channels.get(&channel) {
                let _ = ch
                    .send(&SendMessage {
                        recipient: reply_target,
                        content: current_payload.clone(),
                        subject: None,
                        thread_ts: None,
                    })
                    .await;
            }
        }
    }

    // Announce-on-complete: publish an AnnounceMessage summarizing the
    // pipeline result so parent agents or listeners can react.
    if pipeline.announce_on_complete {
        let announce = AnnounceMessage {
            run_id: RunId::new(),
            agent_id: format!("pipeline:{}", pipeline.name),
            parent_run_id: None,
            summary: truncate_for_announce(&current_payload, 500),
            status: JobStatus::Completed {
                result: current_payload,
            },
            depth: 0,
        };
        let announce_event = Event::new(
            format!("pipeline.{}.announce", pipeline.name),
            "coordinator",
            serde_json::to_string(&announce).unwrap_or_default(),
        )
        .with_correlation(correlation_id)
        .with_boundary(&initial_event.privacy_boundary);
        let _ = bus.publish(announce_event).await;
    }

    Ok(())
}

// ─── Pipeline Step Helpers ──────────────────────────────────────────────────

/// Execute a single sequential pipeline step with retry/skip/abort handling.
#[allow(clippy::too_many_arguments)]
async fn execute_single_step(
    pipeline: &PipelineConfig,
    step_index: usize,
    agent_id: &str,
    current_payload: &str,
    agents: &HashMap<String, mpsc::Sender<TaskMessage>>,
    bus: &Arc<dyn EventBus>,
    correlation_id: &str,
    privacy_boundary: &str,
    error_strategy: ErrorStrategy,
    step_timeout_secs: u64,
) -> anyhow::Result<String> {
    let agent_tx = agents.get(agent_id).ok_or_else(|| {
        anyhow::anyhow!(
            "pipeline '{}' step {}: agent '{}' not found",
            pipeline.name,
            step_index,
            agent_id
        )
    })?;

    let mut attempts = 0u8;
    let max_attempts = match error_strategy {
        ErrorStrategy::Retry { max_attempts } => max_attempts,
        _ => 1,
    };

    loop {
        attempts += 1;
        let (result_tx, result_rx) = oneshot::channel();

        agent_tx
            .send(TaskMessage {
                event: Event::new(
                    format!("pipeline.{}.step.{}", pipeline.name, step_index),
                    "coordinator",
                    current_payload,
                )
                .with_correlation(correlation_id.to_string())
                .with_boundary(privacy_boundary),
                correlation_id: correlation_id.to_string(),
                result_tx: Some(result_tx),
                cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            })
            .await
            .map_err(|_| anyhow::anyhow!("agent '{}' channel closed", agent_id))?;

        match tokio::time::timeout(Duration::from_secs(step_timeout_secs), result_rx).await {
            Ok(Ok(result)) => {
                let payload = result.payload;
                let _ = bus
                    .publish(
                        Event::new(
                            format!("pipeline.{}.step.{}.complete", pipeline.name, step_index),
                            "coordinator",
                            &payload,
                        )
                        .with_correlation(correlation_id.to_string()),
                    )
                    .await;
                return Ok(payload);
            }
            Ok(Err(_)) | Err(_) => match error_strategy {
                ErrorStrategy::Abort => {
                    return Err(anyhow::anyhow!(
                        "pipeline '{}' aborted at step {} (agent '{}')",
                        pipeline.name,
                        step_index,
                        agent_id
                    ));
                }
                ErrorStrategy::Skip => {
                    tracing::warn!(
                        pipeline = %pipeline.name,
                        step = step_index,
                        agent = %agent_id,
                        "step failed, skipping"
                    );
                    return Ok(current_payload.to_string());
                }
                ErrorStrategy::Retry { max_attempts: _ } => {
                    if attempts >= max_attempts {
                        return Err(anyhow::anyhow!(
                            "pipeline '{}' step {} exhausted retries",
                            pipeline.name,
                            step_index
                        ));
                    }
                    tracing::warn!(
                        pipeline = %pipeline.name,
                        step = step_index,
                        attempt = attempts,
                        "step failed, retrying"
                    );
                }
            },
        }
    }
}

/// Execute fan-out steps: for each `FanOutStepConfig`, run all agents in parallel
/// and merge their results according to the configured strategy.
async fn execute_fanout_steps(
    pipeline: &PipelineConfig,
    current_payload: &str,
    agents: &HashMap<String, mpsc::Sender<TaskMessage>>,
    correlation_id: &str,
    privacy_boundary: &str,
    step_timeout_secs: u64,
) -> anyhow::Result<String> {
    let mut merged_payload = current_payload.to_string();

    for (group_idx, fanout_step) in pipeline.fanout_steps.iter().enumerate() {
        let merge_strategy = match fanout_step.merge.as_str() {
            "wait_any" => agentzero_core::MergeStrategy::WaitAny,
            "wait_quorum" => agentzero_core::MergeStrategy::WaitQuorum {
                min: fanout_step.quorum_min,
            },
            _ => agentzero_core::MergeStrategy::WaitAll,
        };

        let step = crate::fanout::FanOutStep {
            agents: fanout_step.agents.clone(),
            merge: merge_strategy,
            timeout: Duration::from_secs(step_timeout_secs),
        };

        // Capture agent senders for the closure.
        let agents_clone = agents.clone();
        let payload = merged_payload.clone();
        let corr = correlation_id.to_string();
        let boundary = privacy_boundary.to_string();
        let pipe_name = pipeline.name.clone();

        let results = crate::fanout::execute_fanout(&step, |agent_id| {
            let agents_inner = agents_clone.clone();
            let payload_inner = payload.clone();
            let corr_inner = corr.clone();
            let boundary_inner = boundary.clone();
            let pipe_name_inner = pipe_name.clone();
            async move {
                let agent_tx = agents_inner
                    .get(&agent_id)
                    .ok_or_else(|| format!("agent '{}' not found for fanout", agent_id))?;

                let (result_tx, result_rx) = oneshot::channel();
                agent_tx
                    .send(TaskMessage {
                        event: Event::new(
                            format!("pipeline.{}.fanout.{}", pipe_name_inner, agent_id),
                            "coordinator",
                            &payload_inner,
                        )
                        .with_correlation(corr_inner)
                        .with_boundary(&boundary_inner),
                        correlation_id: String::new(),
                        result_tx: Some(result_tx),
                        cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    })
                    .await
                    .map_err(|_| format!("agent '{}' channel closed", agent_id))?;

                match tokio::time::timeout(Duration::from_secs(120), result_rx).await {
                    Ok(Ok(result)) => Ok(result.payload),
                    Ok(Err(_)) => Err(format!("agent '{}' result channel dropped", agent_id)),
                    Err(_) => Err(format!("agent '{}' timed out", agent_id)),
                }
            }
        })
        .await;

        // Merge results into a single payload.
        let successful: Vec<String> = results
            .iter()
            .filter_map(|r| r.output.as_ref().ok().cloned())
            .collect();

        if successful.is_empty() {
            let errors: Vec<String> = results
                .iter()
                .filter_map(|r| r.output.as_ref().err().cloned())
                .collect();
            return Err(anyhow::anyhow!(
                "pipeline '{}' fanout group {} failed: {}",
                pipeline.name,
                group_idx,
                errors.join("; ")
            ));
        }

        // Combine results with separators.
        merged_payload = if successful.len() == 1 {
            successful.into_iter().next().unwrap()
        } else {
            successful.join("\n\n---\n\n")
        };
    }

    Ok(merged_payload)
}

// ─── Utilities ──────────────────────────────────────────────────────────────

/// Extract the text content from a JSON payload, falling back to raw string.
fn extract_content(payload: &str) -> String {
    serde_json::from_str::<serde_json::Value>(payload)
        .ok()
        .and_then(|v| v["content"].as_str().map(String::from))
        .unwrap_or_else(|| payload.to_string())
}

/// Extract run metadata (run_id, parent_run_id, depth) embedded in an event
/// payload. Returns None if the payload doesn't contain run tracking fields.
fn extract_run_metadata(payload: &str) -> Option<(RunId, RunId, u8)> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    let run_id = v.get("run_id")?.as_str()?;
    let parent_run_id = v.get("parent_run_id")?.as_str()?;
    let depth = v.get("depth").and_then(|d| d.as_u64()).unwrap_or(0) as u8;
    Some((
        RunId(run_id.to_string()),
        RunId(parent_run_id.to_string()),
        depth,
    ))
}

/// Truncate a string for use in announce summaries.
fn truncate_for_announce(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len])
    }
}

fn uuid_v4() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("corr-{ts}-{seq}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_content_from_json() {
        let payload = r#"{"content":"hello world","sender":"user"}"#;
        assert_eq!(extract_content(payload), "hello world");
    }

    #[test]
    fn extract_content_fallback_to_raw() {
        assert_eq!(extract_content("just raw text"), "just raw text");
    }

    #[test]
    fn error_strategy_from_config() {
        assert!(matches!(
            ErrorStrategy::from_config("abort", 3),
            ErrorStrategy::Abort
        ));
        assert!(matches!(
            ErrorStrategy::from_config("skip", 3),
            ErrorStrategy::Skip
        ));
        assert!(matches!(
            ErrorStrategy::from_config("retry", 5),
            ErrorStrategy::Retry { max_attempts: 5 }
        ));
        assert!(matches!(
            ErrorStrategy::from_config("unknown", 3),
            ErrorStrategy::Abort
        ));
    }

    #[test]
    fn extract_run_metadata_parses_valid_payload() {
        let payload =
            r#"{"run_id":"run-abc","parent_run_id":"run-parent","depth":2,"content":"hello"}"#;
        let meta = extract_run_metadata(payload);
        assert!(meta.is_some());
        let (run_id, parent_run_id, depth) = meta.unwrap();
        assert_eq!(run_id.0, "run-abc");
        assert_eq!(parent_run_id.0, "run-parent");
        assert_eq!(depth, 2);
    }

    #[test]
    fn extract_run_metadata_returns_none_for_missing_fields() {
        assert!(extract_run_metadata(r#"{"content":"hello"}"#).is_none());
        assert!(extract_run_metadata(r#"{"run_id":"run-1"}"#).is_none());
        assert!(extract_run_metadata("not json").is_none());
    }

    #[test]
    fn extract_run_metadata_defaults_depth_to_zero() {
        let payload = r#"{"run_id":"run-abc","parent_run_id":"run-parent"}"#;
        let (_, _, depth) = extract_run_metadata(payload).unwrap();
        assert_eq!(depth, 0);
    }

    #[test]
    fn truncate_for_announce_short_string() {
        let s = "hello world";
        assert_eq!(truncate_for_announce(s, 100), "hello world");
    }

    #[test]
    fn truncate_for_announce_long_string() {
        let s = "a".repeat(600);
        let result = truncate_for_announce(&s, 500);
        // 500 chars + ellipsis
        assert!(result.len() < 510);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn announce_message_serialization_roundtrip() {
        let msg = AnnounceMessage {
            run_id: RunId("run-test-1".to_string()),
            agent_id: "researcher".to_string(),
            parent_run_id: Some(RunId("run-parent-1".to_string())),
            summary: "Found 3 relevant papers".to_string(),
            status: JobStatus::Completed {
                result: "full result text".to_string(),
            },
            depth: 1,
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: AnnounceMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.run_id.0, "run-test-1");
        assert_eq!(parsed.agent_id, "researcher");
        assert_eq!(parsed.depth, 1);
        assert!(matches!(parsed.status, JobStatus::Completed { .. }));
    }
}

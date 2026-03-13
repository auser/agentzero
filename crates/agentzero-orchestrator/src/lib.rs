//! Agent orchestration — the central nervous system for multi-agent coordination.
//!
//! Handles agent routing (AI + keyword), multi-agent coordination via an
//! event bus, pipeline execution, and swarm construction from config.

pub mod agent_router;
pub mod agent_store;
pub mod block_stream;
pub mod coordinator;
pub mod event_bus;
pub mod fanout;
pub mod gossip;
pub mod job_store;
pub mod lanes;
pub mod loop_detection;
pub mod presence;
pub mod swarm;

pub use agent_router::{AgentDescriptor, AgentRouter};
pub use agent_store::{AgentChannelConfig, AgentRecord, AgentStatus, AgentStore, AgentUpdate};
pub use block_stream::{Block, BlockAccumulator};
pub use coordinator::{Coordinator, ErrorStrategy, TaskMessage, TaskResult};
pub use event_bus::{
    BusEvent, EventBus, EventReceiver, FileBackedEventBus, InMemoryEventBus, PersistedEvent,
};
pub use fanout::{execute_fanout, FanOutResult, FanOutStep};
pub use gossip::{GossipConfig, GossipEventBus};
pub use job_store::{EventKind, EventLog, JobStore, RunEvent};
pub use lanes::{LaneConfig, LaneManager, LaneReceivers, WorkItem, WorkResult};
pub use loop_detection::{LoopDetectionConfig, ToolLoopDetector};
pub use presence::{PresenceRecord, PresenceStatus, PresenceStore};
pub use swarm::{build_event_bus, build_swarm, build_swarm_with_presence};

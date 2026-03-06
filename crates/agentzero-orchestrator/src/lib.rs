//! Agent orchestration — the central nervous system for multi-agent coordination.
//!
//! Handles agent routing (AI + keyword), multi-agent coordination via an
//! event bus, pipeline execution, and swarm construction from config.

pub mod agent_router;
pub mod coordinator;
pub mod swarm;

pub use agent_router::{AgentDescriptor, AgentRouter};
pub use coordinator::{Coordinator, ErrorStrategy, TaskMessage, TaskResult};
pub use swarm::build_swarm;

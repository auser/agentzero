//! Agent orchestration — the central nervous system for multi-agent coordination.
//!
//! Handles agent routing (AI + keyword), multi-agent coordination via an
//! event bus, pipeline execution, and swarm construction from config.

pub mod a2a_client;
pub mod agent_router;
pub mod agent_store;
pub mod block_stream;
pub mod coordinator;
pub mod cron_executor;
pub mod fanout;
pub mod goal_planner;
pub mod gossip;
pub mod job_store;
#[cfg(feature = "lanes")]
pub mod lanes;
pub mod loop_detection;
pub mod presence;
pub mod recovery;
pub mod sandbox;
pub mod swarm;
pub mod swarm_context;
pub mod swarm_supervisor;
pub mod template_store;
pub mod trigger_loop;
pub mod workflow_executor;
pub mod workflow_store;
pub mod workspace;

pub use agent_router::{AgentDescriptor, AgentRouter};
pub use agent_store::{AgentChannelConfig, AgentRecord, AgentStatus, AgentStore, AgentUpdate};
pub use block_stream::{Block, BlockAccumulator};
pub use coordinator::{Coordinator, ErrorStrategy, TaskMessage, TaskResult};
pub use cron_executor::{run_cron_executor, CronExecutorConfig};
pub use fanout::{execute_fanout, FanOutResult, FanOutStep};
pub use goal_planner::{
    parse_planner_response, GoalPlanner, PlannedNode, PlannedWorkflow, GOAL_PLANNER_PROMPT,
};
pub use gossip::{GossipConfig, GossipEventBus};
pub use job_store::{EventKind, EventLog, JobRecord, JobStore, RunEvent};
#[cfg(feature = "lanes")]
pub use lanes::{LaneConfig, LaneManager, LaneReceivers, WorkItem, WorkResult};
pub use loop_detection::{LoopDetectionConfig, ToolLoopDetector};
pub use presence::{PresenceRecord, PresenceStatus, PresenceStore};
pub use recovery::{RecoveryAction, RecoveryActionType, RecoveryConfig, RecoveryMonitor};
pub use sandbox::{
    AgentOutput, AgentSandbox, AgentTask, ContainerConfig, ContainerSandbox, MicroVmConfig,
    MicroVmSandbox, SandboxConfig, SandboxHandle, SandboxLevel, WorktreeSandbox,
};
pub use swarm::{build_event_bus, build_swarm, build_swarm_with_presence};
pub use swarm_context::{AgentAssignment, AgentAssignmentStatus, SiblingContext, SwarmContext};
pub use swarm_supervisor::{
    CompletedNodeSummary, ExecutionSnapshot, ReplanPolicy, ReplanRecord, SwarmConfig, SwarmResult,
    SwarmSupervisor,
};
pub use template_store::{TemplateRecord, TemplateStore, TemplateUpdate};
pub use trigger_loop::run_trigger_loop;
pub use workflow_executor::{
    compile as compile_workflow, execute_with_updates as execute_workflow_streaming, ExecutionPlan,
    ExecutionStep, NodeStatus, NodeType, StatusUpdate, StepDispatcher, WorkflowRun,
};
pub use workflow_store::{WorkflowRecord, WorkflowStore, WorkflowUpdate};

//! Core types and domain contracts for the AgentZero secure agent runtime.
//!
//! This crate defines the shared vocabulary used across all AgentZero crates:
//! identifiers, data classification, capability model, policy decisions,
//! approval scopes, runtime isolation tiers, audit event schema, secret
//! handles, trust boundaries, redaction results, and action kinds.

mod action;
pub mod crypto;
mod id;
mod redaction;
mod routing;
pub mod secret;
mod trust;
mod types;
pub mod vault;

pub use action::ActionKind;
pub use id::{AgentId, ExecutionId, SessionId, SkillId, ToolId};
pub use redaction::{placeholder_for, Redaction, RedactionResult};
pub use routing::{route_for_classification, ModelRoutingDecision};
pub use secret::{ResolvedSecret, SecretHandle};
pub use trust::{LabeledContent, TrustSource};
pub use types::{
    ApprovalScope, AuditEvent, Capability, DataClassification, PolicyDecision, RuntimeTier,
    SandboxProfile, SkillManifest, ToolSchema,
};

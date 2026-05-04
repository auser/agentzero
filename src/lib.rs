//! AgentZero: secure AI agent runtime.
//!
//! This is the facade crate that re-exports all AgentZero sub-crates
//! under a single namespace.
//!
//! ```text
//! use agentzero::core::{AgentId, DataClassification};
//! use agentzero::policy::PolicyEngine;
//! use agentzero::audit::AuditLogger;
//! use agentzero::session::Session;
//! use agentzero::tools::ToolRegistry;
//! use agentzero::skills::SkillManifest;
//! use agentzero::sandbox::SandboxProfile;
//! use agentzero::tracing::info;
//! ```

pub use agentzero_acp as acp;
pub use agentzero_audit as audit;
pub use agentzero_core as core;
pub use agentzero_mcp as mcp;
pub use agentzero_policy as policy;
pub use agentzero_sandbox as sandbox;
pub use agentzero_session as session;
pub use agentzero_skills as skills;
pub use agentzero_tools as tools;
pub use agentzero_tracing as tracing;

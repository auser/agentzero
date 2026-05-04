//! Agent Control Protocol adapter for AgentZero.
//!
//! ACP is an adapter, not the core (ADR 0007). This crate provides a thin
//! JSON-RPC over stdio transport for editor integrations. The internal
//! runtime contracts are not coupled to ACP — editors talk ACP, AgentZero
//! translates to its own session/tool/policy contracts.
//!
//! Protocol: newline-delimited JSON over stdin/stdout.

mod protocol;
mod server;

pub use protocol::{AcpMethod, AcpRequest, AcpResponse};
pub use server::AcpServer;

//! MCP (Model Context Protocol) server for AgentZero.
//!
//! Exposes AgentZero's tools through the standard MCP protocol,
//! allowing any MCP-capable client (Claude Code, Cursor, Zed, etc.)
//! to use AgentZero's policy-controlled, audited tool execution.
//!
//! Transport: JSON-RPC 2.0 over stdio (newline-delimited).
//!
//! Supported MCP methods:
//! - `initialize` — server capabilities and info
//! - `tools/list` — list available tools with schemas
//! - `tools/call` — execute a tool with policy enforcement
//! - `shutdown` — graceful shutdown

mod protocol;
mod server;

pub use protocol::{JsonRpcRequest, JsonRpcResponse, McpError};
pub use server::{McpServer, McpServerConfig};

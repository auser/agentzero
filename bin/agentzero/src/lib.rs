//! AgentZero — modular AI-agent runtime and tool framework.
//!
//! This crate provides the public API for embedding AgentZero as a library.
//! For CLI usage, install with `cargo install agentzero`.
//!
//! # Feature flags
//!
//! | Flag | Description |
//! |------|-------------|
//! | `default` | Full build: all tools, plugins, gateway, TUI |
//! | `minimal` | SQLite memory + core tools only |
//! | `embedded` | Plain SQLite (no encryption) + core tools |
//! | `tools-core` / `tools-extended` / `tools-full` | Tool tier selection |
//! | `gateway` | HTTP/WebSocket gateway server |
//! | `plugins` | WASM plugin runtime |
//! | `channels-standard` | Telegram/Discord/Slack integrations |
//! | `privacy` | Noise protocol encrypted transport |

/// Core traits and types: `Agent`, `Tool`, `Provider`, `ToolContext`, etc.
pub use agentzero_core as core;

/// Configuration loading and security policy mapping.
pub use agentzero_config as config;

/// LLM provider implementations (Anthropic, OpenAI-compatible).
pub use agentzero_providers as providers;

/// Tool implementations organized by tier.
pub use agentzero_tools as tools;

/// Agent orchestration, runtime execution, tool wiring.
pub use agentzero_infra as infra;

/// Multi-agent coordination, swarm, pipelines.
pub use agentzero_orchestrator as orchestrator;

/// Encrypted storage and conversation memory.
pub use agentzero_storage as storage;

/// Authentication and credential management.
pub use agentzero_auth as auth;

/// Platform integrations (Telegram, Discord, Slack).
pub use agentzero_channels as channels;

/// HTTP/WebSocket gateway server.
#[cfg(feature = "gateway")]
pub use agentzero_gateway as gateway;

/// WASM plugin runtime.
#[cfg(feature = "plugins")]
pub use agentzero_plugins as plugins;

/// Prelude for convenient imports.
pub mod prelude {
    pub use agentzero_config::{load as load_config, AgentZeroConfig};
    pub use agentzero_core::{Agent, Tool, ToolContext, ToolDefinition, ToolResult};
    pub use agentzero_orchestrator::{build_swarm, Coordinator};
    pub use agentzero_providers::build_provider;
    pub use agentzero_storage::StorageKey;
    pub use agentzero_tools::{ToolSecurityPolicy, ToolTier};
}

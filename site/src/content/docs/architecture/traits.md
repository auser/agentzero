---
title: Trait System
description: AgentZero's trait-driven architecture — every subsystem is swappable through Rust trait interfaces.
---

AgentZero's core principle is that **every subsystem is a trait**. This means you can swap implementations without touching the orchestration layer.

## Architecture Table

| Subsystem | Trait | Ships With | Extension |
|---|---|---|---|
| **AI Models** | `Provider` | OpenAI-compatible (OpenRouter, OpenAI, Anthropic, Ollama) | Implement `Provider` trait |
| **Memory** | `MemoryStore` | SQLite, Turso/libsql | Implement `MemoryStore` trait |
| **Tools** | `Tool` | `read_file`, `write_file`, `shell`, `http_request`, `web_fetch`, `web_search`, `browser`, `delegate`, `memory` | Implement `Tool` trait or WASM plugin |
| **Channels** | `Channel` | Telegram, Discord, Slack, Mattermost | Implement `Channel` trait |
| **Security** | Policy config | Allowlists, OTP, audit, estop, leak guard, syscall anomaly | Config-driven (`[security.*]`) |
| **Observability** | Config-driven | Runtime traces, OpenTelemetry export | `[observability]` config |
| **Runtime** | Orchestrator | Native single-process | `[runtime]` config (native/docker) |
| **Plugins** | WASM sandbox | Extism-based plugin host | `.wasm` modules with `manifest.json` |
| **Skills** | Skill registry | Built-in skillforge + SOP engine | Install from local/remote/git |
| **Identity** | Config-driven | OpenClaw markdown, AIEOS JSON | `[identity]` config |
| **Gateway** | HTTP service | Axum-based REST API | Endpoint handlers |
| **Cost** | Tracker | Token + USD tracking with limits | `[cost]` config |

## Core Traits

### Provider

The `Provider` trait abstracts AI model access:

```rust
#[async_trait]
pub trait Provider: Send + Sync {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse>;
    fn name(&self) -> &str;
}
```

Ships with an OpenAI-compatible implementation that works with OpenRouter, OpenAI, Anthropic, Ollama, and any `/v1/chat/completions` endpoint.

### MemoryStore

The `MemoryStore` trait abstracts conversation persistence:

```rust
#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn append(&self, entry: MemoryEntry) -> Result<()>;
    async fn recent(&self, limit: usize) -> Result<Vec<MemoryEntry>>;
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>>;
}
```

Ships with SQLite (default) and Turso/libsql (feature-gated).

### Tool

The `Tool` trait abstracts agent capabilities:

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    async fn execute(&self, params: Value) -> Result<String>;
}
```

All built-in tools implement this trait. WASM plugins are wrapped in a `Tool` adapter.

## Crate Boundaries

Each trait lives in `agentzero-core`. Implementations live in their own crates:

```
agentzero-core          # Traits + orchestrator (no implementations)
├── agentzero-providers # Provider implementations
├── agentzero-memory    # MemoryStore implementations
├── agentzero-tools     # Tool implementations
├── agentzero-channels  # Channel implementations
├── agentzero-plugins   # WASM plugin host
└── agentzero-infra     # Wiring layer (connects traits to implementations)
```

This ensures the core never depends on infrastructure — only the reverse.

## Security Model

Security is **not a trait** — it's a policy layer enforced at construction time:

1. Tool instances are created with their security config baked in
2. Tools validate every call against allowlists, path restrictions, and size limits
3. The agent orchestrator doesn't need to know about security — it's already enforced

```
User message → Agent → Tool::execute() → [security policy check] → actual execution
```

If a policy check fails, the tool returns an error. The agent sees the error and can adjust.

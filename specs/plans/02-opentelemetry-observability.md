# Plan 02: OpenTelemetry & Observability

## Problem

The config model declares `[observability] otel_endpoint` and `otel_service_name` fields, but **no `opentelemetry` crate dependency exists** anywhere in the workspace. The config fields are dead code — users who set them get silent no-ops. This is misleading and wastes debugging time.

Additionally, there are no request correlation IDs, no distributed trace propagation, and no JSON structured logging for container environments.

## Current State

### What exists (good foundation)
- **Prometheus metrics** at `GET /metrics` via `metrics-exporter-prometheus` in `crates/agentzero-gateway/src/gateway_metrics.rs`:
  - `requests_total`, `request_duration_seconds`, `active_connections`, `ws_connections_total`, `errors_total`
  - 6 privacy-specific metrics (noise sessions, handshakes, relay, key rotation, encrypt duration)
- **Request metrics middleware** in `crates/agentzero-gateway/src/middleware.rs:217` — every request instrumented with method/path/status/latency
- **`tracing` crate** used throughout codebase with structured spans and fields
- **`tracing-subscriber`** configured in runtime initialization
- **Audit logging** in `crates/agentzero-infra/src/audit.rs` — file-based JSON lines with auto-redaction
- **Config model** in `crates/agentzero-config/src/model.rs`:
  ```rust
  pub struct ObservabilityConfig {
      pub otel_endpoint: Option<String>,    // dead
      pub otel_service_name: Option<String>, // dead
  }
  ```

### What's missing
- No `opentelemetry` crate dependency (config is dead)
- No request correlation IDs (`X-Request-Id` header)
- No W3C `traceparent` propagation
- No JSON log output for containers
- Logs go to files only (daemon mode) — no stdout JSON for Fluentd/Datadog/CloudWatch

## Implementation

### Phase 1: Request Correlation IDs (no new deps)

**File: `crates/agentzero-gateway/src/middleware.rs`**

Add middleware that:
1. Reads `X-Request-Id` header from request, or generates a UUID v4
2. Stores in request extensions
3. Creates a `tracing::span` with `request_id` field
4. Returns `X-Request-Id` in response headers

This is valuable even without OTel — every log line gets a request ID for debugging.

### Phase 2: OpenTelemetry Integration (feature-gated)

**New feature: `otel` in `crates/agentzero-gateway/Cargo.toml`**

```toml
[features]
otel = [
    "dep:opentelemetry",
    "dep:opentelemetry_sdk",
    "dep:opentelemetry-otlp",
    "dep:tracing-opentelemetry",
]

[dependencies]
opentelemetry = { version = "0.28", optional = true }
opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"], optional = true }
opentelemetry-otlp = { version = "0.28", optional = true }
tracing-opentelemetry = { version = "0.28", optional = true }
```

**New file: `crates/agentzero-gateway/src/otel.rs`**

```rust
/// Initialize OpenTelemetry tracing pipeline.
/// Returns a guard that flushes on drop.
pub fn init_otel(endpoint: &str, service_name: &str) -> OtelGuard {
    // 1. Create OTLP exporter targeting the configured endpoint
    // 2. Build TracerProvider with batch span processor
    // 3. Create tracing-opentelemetry layer
    // 4. Add layer to existing tracing subscriber
    // 5. Return guard for graceful shutdown
}

pub struct OtelGuard { /* TracerProvider handle */ }
impl Drop for OtelGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}
```

**Wire in gateway startup** (`crates/agentzero-gateway/src/lib.rs` or wherever server starts):
```rust
#[cfg(feature = "otel")]
let _otel_guard = if let Some(endpoint) = &config.observability.otel_endpoint {
    Some(otel::init_otel(endpoint, config.observability.otel_service_name.as_deref().unwrap_or("agentzero")))
} else {
    None
};
```

### Phase 3: W3C Trace Context Propagation

In the request correlation middleware:
- Read `traceparent` header → inject into current span context
- Write `traceparent` header in response
- This enables distributed tracing across services (agentzero → LLM provider → back)

### Phase 4: JSON Structured Logging

**Config addition** in `crates/agentzero-config/src/model.rs`:
```rust
pub struct LoggingConfig {
    pub format: LogFormat,  // "text" (default) | "json"
    pub level: String,      // "info" default
}

pub enum LogFormat { Text, Json }
```

**Runtime change** — wherever `tracing_subscriber` is initialized:
```rust
match config.logging.format {
    LogFormat::Json => subscriber.with(fmt::layer().json()),
    LogFormat::Text => subscriber.with(fmt::layer()),
}
```

JSON output enables:
- Container log aggregation (Fluentd, Datadog agent, CloudWatch)
- Structured search in log management tools
- Machine-parseable error analysis

### Phase 5: Build variant integration

- Add `otel` to `Justfile` build recipes:
  ```just
  build-otel:
      cargo build --release --features otel
  ```
- Add `otel` to Dockerfile build arg options
- Do NOT make `otel` default — keeps binary small for local users

## Files to Create/Modify

| File | Action |
|------|--------|
| `crates/agentzero-gateway/Cargo.toml` | Add otel deps (feature-gated) |
| `crates/agentzero-gateway/src/otel.rs` | New: OTel init + guard |
| `crates/agentzero-gateway/src/middleware.rs` | Add request ID + traceparent middleware |
| `crates/agentzero-gateway/src/lib.rs` | Wire OTel init on startup |
| `crates/agentzero-config/src/model.rs` | Add LoggingConfig, LogFormat |
| `crates/agentzero-infra/src/runtime.rs` | JSON log format support |
| `Justfile` | Add `build-otel` recipe |

## Tests (~10 new)

- Request ID middleware: generates ID when missing, preserves when present
- Request ID appears in response header
- JSON log format produces valid JSON to stdout
- OTel guard shutdown doesn't panic
- Traceparent header round-trip
- Config validation: invalid log format rejected
- Feature gate: code compiles with and without `otel` feature

## Verification

1. `cargo build -p agentzero-gateway` — compiles without `otel` feature (no new deps)
2. `cargo build -p agentzero-gateway --features otel` — compiles with OTel
3. Start gateway, send request → `X-Request-Id` in response headers
4. Start with `AGENTZERO__LOGGING__FORMAT=json` → JSON lines on stdout
5. Start Jaeger (`docker run jaegertracing/all-in-one`), configure `otel_endpoint = "http://localhost:4318"` → traces visible in Jaeger UI
6. All existing tests still pass

## Dependencies Added

| Crate | Version | Condition |
|-------|---------|-----------|
| `opentelemetry` | 0.28 | `otel` feature |
| `opentelemetry_sdk` | 0.28 | `otel` feature |
| `opentelemetry-otlp` | 0.28 | `otel` feature |
| `tracing-opentelemetry` | 0.28 | `otel` feature |
| `uuid` | 1 | always (for request IDs) |

# Plan 05: Structured Logging & JSON Output

## Problem

AgentZero logs to files only (daemon mode: 10MB rotation, 5 files). In containerized deployments, logs should go to stdout in structured JSON format so container orchestrators (Docker, k8s) and log aggregators (Fluentd, Datadog, CloudWatch, Loki) can ingest them. Currently there's no way to get JSON-formatted logs, and no per-module log level configuration.

## Current State

### Logging setup
- `tracing` crate used throughout codebase
- `tracing-subscriber` initialized in runtime startup
- Daemon mode (`crates/agentzero-cli/src/daemon.rs`): logs to file with rotation (10MB, 5 files)
- Non-daemon: logs to stderr with default `tracing_subscriber::fmt` format
- No config options for log format or per-module levels

### Config model (`crates/agentzero-config/src/model.rs`)
- No `[logging]` section exists currently
- Logging behavior is hardcoded in daemon vs non-daemon paths

### Audit logging (`crates/agentzero-infra/src/audit.rs`)
- Separate from `tracing` — file-based JSON lines with timestamps and auto-redaction
- This is fine and should remain separate (security audit trail ≠ application logs)

## Implementation

### Phase 1: Config Model

**Add to `crates/agentzero-config/src/model.rs`:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log output format: "text" (human-readable) or "json" (structured)
    #[serde(default = "default_log_format")]
    pub format: LogFormat,

    /// Default log level: "trace", "debug", "info", "warn", "error"
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Per-module log level overrides
    /// Example: { "agentzero_gateway" = "debug", "agentzero_storage" = "warn" }
    #[serde(default)]
    pub modules: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

fn default_log_format() -> LogFormat { LogFormat::Text }
fn default_log_level() -> String { "info".to_string() }
```

**Environment variables:**
- `AGENTZERO__LOGGING__FORMAT=json`
- `AGENTZERO__LOGGING__LEVEL=debug`

**TOML config:**
```toml
[logging]
format = "json"
level = "info"

[logging.modules]
agentzero_gateway = "debug"
agentzero_storage = "warn"
```

### Phase 2: Subscriber Initialization

Find where `tracing_subscriber` is currently initialized (likely in `crates/agentzero-infra/src/runtime.rs` or the binary entrypoint) and update:

```rust
use tracing_subscriber::{fmt, EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

fn init_logging(config: &LoggingConfig) {
    let filter = build_env_filter(config);

    match config.format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().json().with_target(true).with_span_list(true))
                .init();
        }
        LogFormat::Text => {
            tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_target(true))
                .init();
        }
    }
}

fn build_env_filter(config: &LoggingConfig) -> EnvFilter {
    let mut filter = EnvFilter::new(&config.level);
    for (module, level) in &config.modules {
        filter = filter.add_directive(
            format!("{module}={level}").parse().unwrap()
        );
    }
    // Allow RUST_LOG to override config
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        filter = EnvFilter::new(rust_log);
    }
    filter
}
```

### Phase 3: JSON Output Format

When `format = "json"`, each log line is a self-contained JSON object:

```json
{
    "timestamp": "2026-03-07T10:15:30.123Z",
    "level": "INFO",
    "target": "agentzero_gateway::middleware",
    "message": "request completed",
    "fields": {
        "method": "POST",
        "path": "/v1/chat/completions",
        "status": 200,
        "latency_ms": 1234,
        "request_id": "abc-123"
    },
    "span": {
        "name": "request",
        "request_id": "abc-123"
    }
}
```

This format is compatible with:
- **Fluentd**: json parser
- **Datadog**: auto-parsed JSON logs
- **CloudWatch**: JSON log insights queries
- **Loki/Grafana**: JSON label extraction
- **ELK**: Elasticsearch JSON ingestion

### Phase 4: Daemon Mode Integration

Update daemon log setup in `crates/agentzero-cli/src/daemon.rs`:
- If `format = "json"`: use JSON formatter for file logs too (enables machine-parsing of daemon log files)
- If `format = "text"`: keep current human-readable file format
- In both cases: respect `level` and `modules` config

### Phase 5: Docker Integration

In `Dockerfile` and `docker-compose.yml` (from Plan 01):
- Default `AGENTZERO__LOGGING__FORMAT=json` in container environment
- This makes `docker logs` output structured JSON by default
- Human users running locally keep `text` format (the default)

## Files to Create/Modify

| File | Action |
|------|--------|
| `crates/agentzero-config/src/model.rs` | Add LoggingConfig, LogFormat |
| `crates/agentzero-infra/src/runtime.rs` | Update tracing subscriber init |
| `crates/agentzero-cli/src/daemon.rs` | Respect format config in daemon mode |
| `docker-compose.yml` | Default to JSON format |

## Tests (~6 new)

1. Default config: format = text, level = info
2. JSON format: log output is valid JSON with expected fields
3. Per-module level: debug module produces debug logs, warn module suppresses info
4. RUST_LOG override: env var takes precedence over config
5. Config serde: LogFormat round-trips through TOML
6. Config validation: invalid level string rejected

## Verification

1. Start with default config → human-readable logs on stderr
2. Set `AGENTZERO__LOGGING__FORMAT=json` → JSON lines on stderr
3. Pipe JSON logs to `jq '.'` → all lines parse successfully
4. Set module override → only that module's verbosity changes
5. Daemon mode with JSON → log files contain JSON lines
6. All existing tests pass

## Notes

- `tracing-subscriber`'s JSON layer is already included in the `tracing-subscriber` crate with the `json` feature — likely already enabled. Verify in workspace `Cargo.toml`.
- JSON logging has ~5-10% overhead vs text due to serialization. Acceptable for production; local dev stays on text.
- This plan is independent of Plan 02 (OpenTelemetry) but complements it — OTel exports traces while JSON logging exports log events. Both can coexist.

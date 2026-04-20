# Plan 54: AI-Native Data Source Connectors

## Context

AgentZero currently has no way to connect external data sources (APIs, databases, files) to each other. Users need the ability to tie any datasource to any other datasource automatically — connecting a Shopify store to a Postgres database, syncing CSV files to a REST API, or bridging two SaaS tools. Rather than building a traditional ETL platform, AgentZero leverages its AI reasoning to discover schemas, propose field mappings, and orchestrate syncs via natural language.

**Key architectural insight:** Connectors should NOT be a new trait/abstraction. The existing `DynamicToolDef` + `DynamicToolStrategy` system already supports Shell, HTTP, LLM, Composite, and Codegen strategies with quality tracking, evolution, and persistence. A "connector" is a configuration object that generates a family of related `DynamicToolDef` entries plus a schema manifest.

---

## Phase 1: Connector Types & Registry — new crate `agentzero-connectors`

Create `crates/agentzero-connectors/` with core types and a registry.

### 1a. Core types — `crates/agentzero-connectors/src/lib.rs`

```rust
pub struct ConnectorManifest {
    pub name: String,                    // "shopify", "postgres", "csv"
    pub connector_type: ConnectorType,   // RestApi, Database, File
    pub auth: AuthConfig,                // api_key, oauth2, basic, connection_string
    pub entities: Vec<EntitySchema>,     // discovered or declared entity schemas
    pub capabilities: ConnectorCaps,     // read, write, list, search, subscribe, discover
}

pub struct EntitySchema {
    pub name: String,                    // "orders", "contacts", "products"
    pub fields: Vec<FieldDef>,
    pub primary_key: String,
    pub json_schema: serde_json::Value,  // full JSON Schema for validation
}

pub struct FieldDef {
    pub name: String,
    pub field_type: FieldType,           // String, Number, Boolean, DateTime, Reference(entity)
    pub required: bool,
    pub description: String,
}

pub struct DataLink {
    pub id: String,
    pub name: String,
    pub source: DataEndpoint,            // connector + entity
    pub target: DataEndpoint,
    pub field_mappings: Vec<FieldMapping>,
    pub sync_mode: SyncMode,             // OnDemand, Scheduled { cron }, EventDriven { topic }
    pub transform: Option<String>,       // optional jq/JSONata expression
}

pub struct FieldMapping {
    pub source_field: String,
    pub target_field: String,
    pub transform: Option<String>,       // per-field transform expression
}
```

### 1b. Connector registry — `crates/agentzero-connectors/src/registry.rs`

- `ConnectorRegistry` loads manifests from TOML config `[[connectors]]` sections
- Generates `DynamicToolDef` entries per connector entity (e.g. `shopify_list_orders`, `shopify_get_order`, `shopify_create_order`)
- Uses existing `DynamicToolStrategy::Http` for REST APIs, `DynamicToolStrategy::Shell` for CLI-based DB access, `DynamicToolStrategy::Composite` for multi-step operations
- Stores `DataLink` definitions in the encrypted JSON store (reuse `agentzero-storage`)

### 1c. Connector templates — `crates/agentzero-connectors/src/templates/`

A `ConnectorTemplate` trait generates `ConnectorManifest` + tool defs from config:

```rust
pub trait ConnectorTemplate: Send + Sync {
    fn manifest(&self, config: &serde_json::Value) -> anyhow::Result<ConnectorManifest>;
    fn generate_tools(&self, instance_name: &str, config: &serde_json::Value) -> Vec<DynamicToolDef>;
    async fn discover_schema(&self, config: &serde_json::Value) -> anyhow::Result<Vec<EntitySchema>>;
}
```

Built-in templates:
- `rest_api.rs` — generic REST connector (base_url + auth + optional OpenAPI spec URL for auto-discovery)
- `database.rs` — Postgres/SQLite via connection string (schema discovery from `information_schema` / `sqlite_master`)
- `file.rs` — CSV/JSON/JSONL file import/export (schema inferred from headers/first record)

---

## Phase 2: Agent-Facing Tools — 3 new tools in `agentzero-tools`

Three tools that let the AI agent manage connectors and data links during conversation:

### 2a. `connector_discover` tool

- Input: connector name (already configured in TOML)
- Action: calls `ConnectorTemplate::discover_schema()` on the connector
- Output: entity schemas as structured JSON
- The agent uses this to understand what data a source exposes

### 2b. `data_link` tool

- Input: CRUD operations on DataLink (create, list, update, delete)
- For `create` with `auto_map: true`: the tool returns source + target schemas, and the agent uses its LLM reasoning to propose field mappings (no special mapping code needed — the agent IS the mapper)
- Persists links in encrypted JSON store

### 2c. `data_sync` tool

- Input: link ID (or inline source/target/mappings)
- Action: reads from source connector tools, applies field mappings + transforms, writes to target connector tools
- Uses existing `DynamicToolStrategy::Composite` pattern internally
- Output: sync summary (records read, written, errors)

---

## Phase 3: Config Integration

### 3a. Add connector config to `crates/agentzero-config/src/model.rs`

```toml
[[connectors]]
name = "shopify"
type = "rest_api"
base_url = "https://mystore.myshopify.com/admin/api/2024-01"
auth = { type = "header", key = "X-Shopify-Access-Token", value_env = "SHOPIFY_TOKEN" }
openapi_url = "https://mystore.myshopify.com/admin/api/2024-01.json"

[[connectors]]
name = "orders_db"
type = "database"
connection_string_env = "ORDERS_DB_URL"

[[connectors]]
name = "products_csv"
type = "file"
path = "./data/products.csv"
```

### 3b. Wire into runtime — `crates/agentzero-infra/src/runtime.rs`

- On startup, `ConnectorRegistry` loads configured connectors
- Generated `DynamicToolDef`s registered in `DynamicToolRegistry`
- The 3 agent-facing tools (`connector_discover`, `data_link`, `data_sync`) added to tool set

---

## Phase 4: Workflow Integration

### 4a. New `NodeType` variants — `crates/agentzero-orchestrator/src/workflow_executor.rs`

Add `DataSource` and `DataSink` to the `NodeType` enum:
- `DataSource` — reads from a connector entity, outputs JSON records
- `DataSink` — writes JSON records to a connector entity

These compile to tool calls against the generated connector tools, so execution reuses the existing workflow dispatch path.

### 4b. Scheduled sync via existing cron

`DataLink` with `SyncMode::Scheduled { cron }` registers a cron job in the existing `cron_executor` that calls `data_sync` periodically.

---

## Phase 5: Event-Driven Sync

Wire connectors with webhook support into the existing `EventBus`:
- Connectors that support `subscribe` register a webhook handler in the gateway
- Incoming webhooks publish to `connector:{name}:{entity}:changed` event topic
- `DataLink` with `SyncMode::EventDriven` subscribes to these events and triggers `data_sync`

---

## Files to Create

| File | Purpose |
|------|---------|
| `crates/agentzero-connectors/Cargo.toml` | New crate dependencies |
| `crates/agentzero-connectors/src/lib.rs` | Core types (ConnectorManifest, EntitySchema, DataLink, FieldMapping) |
| `crates/agentzero-connectors/src/registry.rs` | ConnectorRegistry — loads config, generates tools, stores links |
| `crates/agentzero-connectors/src/templates/mod.rs` | ConnectorTemplate trait |
| `crates/agentzero-connectors/src/templates/rest_api.rs` | Generic REST API connector |
| `crates/agentzero-connectors/src/templates/database.rs` | Postgres/SQLite connector |
| `crates/agentzero-connectors/src/templates/file.rs` | CSV/JSON file connector |
| `crates/agentzero-tools/src/connector_discover.rs` | Schema discovery tool |
| `crates/agentzero-tools/src/data_link.rs` | DataLink CRUD tool |
| `crates/agentzero-tools/src/data_sync.rs` | Sync execution tool |

## Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add `agentzero-connectors` member |
| `crates/agentzero-config/src/model.rs` | Add `ConnectorConfig` struct and `connectors: Vec<ConnectorConfig>` field |
| `crates/agentzero-infra/src/runtime.rs` | Wire connector registry + tools on startup |
| `crates/agentzero-infra/src/tools/mod.rs` | Register connector tools in tool builder |
| `crates/agentzero-infra/Cargo.toml` | Add `agentzero-connectors` dependency |
| `crates/agentzero-tools/src/lib.rs` | Export new tool modules |
| `crates/agentzero-tools/Cargo.toml` | Add `agentzero-connectors` dependency |
| `crates/agentzero-orchestrator/src/workflow_executor.rs` | Add DataSource/DataSink NodeType variants (Phase 4) |

## Cross-Cutting Concerns

### Encryption at Rest

All connector data is encrypted at rest using the existing infrastructure:

- **DataLink definitions** — persisted in `agentzero-storage`'s encrypted JSON store (AES-256-GCM), same as dynamic tool definitions
- **Cached connector data** — any snapshots or sync state stored in the encrypted store with TTL expiry
- **Credentials** — never stored in plaintext. Connector configs reference environment variables via `value_env` fields (e.g. `connection_string_env = "ORDERS_DB_URL"`). OAuth tokens managed by `agentzero-auth`'s existing credential store (already encrypted)
- **Schema manifests** — treated as potentially sensitive metadata; stored encrypted alongside their connector config
- **Sync logs** — written to the existing `EventBus` which inherits the configured event bus encryption mode

### PII Safety

PII safety is a core AgentZero goal — no PII may ever reach a remote LLM provider. Connector data flows require special attention because they can contain arbitrary user data:

- **Schema discovery** — `connector_discover` returns field names, types, and descriptions only — never sample data. Safe for LLM reasoning
- **Field mapping proposals** — when the agent proposes mappings with `auto_map: true`, it operates on schema metadata only, not record values
- **Data sync** — `data_sync` moves data between connectors without passing through the LLM. The sync engine applies field mappings mechanically (no LLM in the data path)
- **Privacy boundary enforcement** — connector tools inherit `privacy_boundary` from their `ToolContext`. Connectors configured with `privacy_boundary = "local_only"` cannot have their data sent to remote providers
- **Audit trail** — every `data_sync` execution publishes an event to the `EventBus` with record counts and error summaries (never record contents)

### Rate Limiting & Pagination

Real-world APIs have rate limits and return paginated results. The REST API connector template must handle both:

- **Pagination** — `RestApiTemplate` supports three pagination strategies declared in connector config:
  - `cursor` — follows a `next_cursor` field in responses (Shopify, Stripe style)
  - `offset` — increments `offset` parameter (classic REST)
  - `link_header` — follows RFC 5988 `Link: <url>; rel="next"` headers (GitHub style)
  - Default page size configurable per connector; override per-request via tool input
- **Rate limiting** — respect `429 Too Many Requests` responses:
  - Honor `Retry-After` header (seconds or HTTP-date)
  - Configurable per-connector rate limit ceiling (e.g. `max_requests_per_second = 2`)
  - Exponential backoff with jitter on consecutive 429s (max 3 retries)
- **Batch operations** — `data_sync` batches writes where the target API supports it (configurable `batch_size`, default 100)

### Credential Management & OAuth Token Refresh

Connectors that use OAuth2 need automatic token refresh:

- **Reuse `agentzero-auth`** — the existing OAuth2 flow in `agentzero-auth` handles token acquisition, storage, and refresh. Connector configs reference an auth profile by name: `auth = { type = "oauth2", profile = "shopify_oauth" }`
- **Token refresh on 401** — when a connector tool receives a `401 Unauthorized`, it triggers a token refresh via `agentzero-auth` before retrying (single retry, then fail with clear error)
- **API key rotation** — for connectors using API keys via env vars, rotation is handled externally (user updates the env var, connector picks it up on next invocation — no caching of resolved env values)

### Schema Drift Detection

Source schemas change over time (fields renamed, added, removed). Broken field mappings should be detected early:

- **`connector_discover` detects drift** — when called, compares the live schema against the stored manifest. If fields referenced by existing `DataLink` mappings are missing or type-changed, returns a `drift_warnings` array in the response
- **`data_sync` pre-flight check** — before executing, validates that all mapped source fields still exist in the current schema. If not, the sync fails with a descriptive error listing the broken mappings (never silently drops fields)
- **Drift events** — schema changes detected during discovery publish a `connector:{name}:schema_drift` event to the `EventBus` so scheduled workflows can react

### Sync Error Recovery & Idempotency

Partial sync failures (wrote 500 of 1000 records) must be recoverable:

- **Idempotent writes** — `data_sync` uses upsert semantics (insert or update by primary key) for all target connectors. Re-running a sync on the same data is safe
- **Sync cursor** — each `DataLink` stores a `last_sync_cursor` (last processed primary key or timestamp) in the encrypted store. On failure, the next sync resumes from the cursor position rather than reprocessing everything
- **Sync state** — each execution records `SyncResult { records_read, records_written, records_skipped, records_failed, errors: Vec<SyncError>, cursor: Option<String> }` persisted alongside the link
- **Dead letter queue** — records that fail to write after 3 retries are collected in a `failed_records` array in the sync result. The agent can inspect these and decide how to handle them (fix data, skip, retry manually)
- **Transactional targets** — for database targets that support transactions, `data_sync` wraps each batch in a transaction. On batch failure, the transaction rolls back and the cursor is not advanced

---

## Key Design Decisions

1. **No new core trait** — connectors generate `DynamicToolDef`s, reusing the entire existing tool infrastructure (security, quality tracking, evolution, persistence)
2. **AI IS the field mapper** — no dedicated mapping engine. The agent reads both schemas and proposes mappings using LLM reasoning. Simpler and handles edge cases better than rule-based mapping
3. **Pass-through data** — no data warehouse. Data flows through tools as JSON during sync. For caching, the existing encrypted store can hold snapshots with TTL
4. **LLM never sees record data** — the agent reasons about schemas (field names/types), never about actual record values. Data sync is mechanical, not AI-mediated
5. **Incremental delivery** — Phases 1-3 are the MVP (~1500 lines). Phases 4-5 build on it

## Verification

1. **Unit tests**: Each connector template gets tests with mock HTTP/DB responses
2. **Integration test**: Configure a REST API connector + SQLite connector in TOML, run `connector_discover` on both, create a `DataLink`, run `data_sync`, verify records transfer
3. **PII test**: Verify that no record data appears in LLM tool call inputs when using `auto_map` — only schema metadata
4. **Pagination test**: Mock a paginated API (3 pages), verify `data_sync` retrieves all pages
5. **Drift test**: Modify a mock schema after link creation, verify `connector_discover` returns drift warnings and `data_sync` fails with descriptive error
6. **Idempotency test**: Run `data_sync` twice with same data, verify no duplicates in target
7. **Partial failure test**: Mock target that fails on record 50 of 100, verify cursor advances to 49, re-run picks up at 50
8. **Agent conversation test**: Start agent with connectors configured, verify it can discover schemas and create links via natural language
9. **Build**: `cargo build --all-features` and `cargo clippy --all-targets` must pass with zero warnings

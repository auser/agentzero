# Plan 06: Multi-Tenancy & Role-Based Access Control

## Problem

AgentZero has no concept of user identity, organization isolation, or permission scoping. Auth is binary: you have a valid bearer token or you don't. This means:

- Cannot serve multiple customers from one deployment
- Cannot differentiate admin vs read-only vs operator roles
- Cannot scope API keys to specific endpoints or agents
- No per-user resource quotas or rate limits
- No audit trail attribution (who did what)

This is the largest architectural gap for commercial deployment. It touches auth, storage, API layer, and configuration.

## Current State

### Authentication (`crates/agentzero-gateway/src/auth.rs`)
- **Pairing-based auth**: One-time code → persistent bearer token (XChaCha20Poly1305 encrypted)
- **Static bearer token**: `AGENTZERO_GATEWAY_BEARER_TOKEN` env var
- **OTP for sensitive ops**: TOTP gates high-risk operations
- No user identity attached to tokens — just "valid" or "not valid"

### Storage (`crates/agentzero-storage/src/memory/sqlite.rs`)
- Single `memory` table, no tenant/user column
- Privacy boundaries exist (`privacy_boundary`, `source_channel`) but are content-scoping, not user-scoping
- Conversation IDs exist but are not tied to users

### Gateway middleware (`crates/agentzero-gateway/src/middleware.rs`)
- Rate limiter: global, not per-user (600 req/60s)
- No user context propagation

## Implementation

This is a **multi-sprint effort**. This plan covers the full architecture but should be broken into at least 2-3 implementation sprints.

### Sprint A: Identity & API Keys

#### A.1 — User/Org Model

**New file: `crates/agentzero-auth/src/identity.rs`**

```rust
pub struct Organization {
    pub id: String,        // UUID
    pub name: String,
    pub created_at: DateTime<Utc>,
}

pub struct User {
    pub id: String,        // UUID
    pub org_id: String,    // belongs to one org
    pub email: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Role {
    Owner,     // full access, manages org
    Admin,     // manages agents, keys, users
    Operator,  // runs agents, reads data
    Viewer,    // read-only access
}
```

#### A.2 — API Key Management

**New file: `crates/agentzero-auth/src/api_keys.rs`**

```rust
pub struct ApiKey {
    pub id: String,             // UUID
    pub org_id: String,
    pub user_id: String,        // who created it
    pub name: String,           // human label
    pub key_hash: String,       // SHA-256 of key (never store plaintext)
    pub key_prefix: String,     // first 8 chars for identification (e.g., "az_live_")
    pub scopes: Vec<Scope>,     // what this key can do
    pub rate_limit: Option<u32>,// per-key rate limit (req/min)
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

pub enum Scope {
    ChatCompletions,    // /v1/chat/completions
    Models,             // /v1/models
    Agents,             // agent management
    Memory,             // memory read/write
    Privacy,            // privacy endpoints
    Admin,              // org/user/key management
    All,                // everything
}
```

Key format: `az_live_<random_32_chars>` (prefix for identification, stored as SHA-256 hash).

#### A.3 — Auth Middleware Update

Update `crates/agentzero-gateway/src/middleware.rs`:

```rust
// Extract user context from bearer token or API key
pub struct RequestContext {
    pub org_id: String,
    pub user_id: String,
    pub role: Role,
    pub scopes: Vec<Scope>,
    pub request_id: String,
}

// Middleware:
// 1. Extract Authorization header
// 2. Look up API key by prefix + hash
// 3. Check scopes against requested endpoint
// 4. Check rate limit (per-key)
// 5. Set RequestContext in request extensions
```

#### A.4 — Storage

New SQLite tables (via migration framework from Plan 03):

```sql
CREATE TABLE organizations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE users (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations(id),
    email TEXT NOT NULL UNIQUE,
    role TEXT NOT NULL DEFAULT 'viewer',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE api_keys (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL REFERENCES organizations(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    key_hash TEXT NOT NULL UNIQUE,
    key_prefix TEXT NOT NULL,
    scopes TEXT NOT NULL DEFAULT '[]',  -- JSON array
    rate_limit INTEGER,
    expires_at TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT
);
```

Add `org_id` column to `memory` table for tenant isolation.

### Sprint B: Tenant Isolation

#### B.1 — Memory Isolation

All memory queries scoped by `org_id`:
```sql
SELECT * FROM memory WHERE org_id = ?1 AND ...
```

The `MemoryStore` trait methods gain an `org_id` parameter (or `RequestContext` is threaded through).

#### B.2 — Agent Isolation

Agent configurations scoped to organizations. Each org has its own:
- Agent definitions
- Tool permissions
- LLM provider config
- Channel bindings

#### B.3 — Per-Tenant Rate Limiting

Replace global `AtomicU64` rate limiter with per-org/per-key counters:
```rust
struct PerKeyRateLimiter {
    limits: DashMap<String, SlidingWindowCounter>,
}
```

### Sprint C: Management API

#### C.1 — Admin Endpoints

```
POST   /v1/admin/organizations          Create org
GET    /v1/admin/organizations/:id      Get org
POST   /v1/admin/users                  Create user
GET    /v1/admin/users                  List users
PATCH  /v1/admin/users/:id             Update role
POST   /v1/admin/api-keys              Create API key (returns key once)
GET    /v1/admin/api-keys              List keys (no secrets)
DELETE /v1/admin/api-keys/:id          Revoke key
```

All admin endpoints require `Admin` or `Owner` role.

#### C.2 — CLI Commands

```
az org create --name "My Company"
az org list
az user create --email user@example.com --role operator
az user list
az apikey create --name "Production" --scopes chat,models
az apikey list
az apikey revoke <id>
```

### Backward Compatibility

- **Single-tenant mode** (default): When no organizations exist, the system behaves exactly as today. Static bearer token auth continues to work. No org_id filtering applied.
- **Multi-tenant mode**: Activated when the first organization is created. From that point, all requests must present a scoped API key.
- Existing databases: `org_id` column added with empty default. Existing data belongs to "default" org.

## Files to Create/Modify

| File | Action | Sprint |
|------|--------|--------|
| `crates/agentzero-auth/src/identity.rs` | New: User, Org, Role | A |
| `crates/agentzero-auth/src/api_keys.rs` | New: ApiKey, Scope, management | A |
| `crates/agentzero-gateway/src/auth.rs` | API key lookup, scope checking | A |
| `crates/agentzero-gateway/src/middleware.rs` | RequestContext, per-key rate limit | A+B |
| `crates/agentzero-storage/src/memory/migrations.rs` | Add org/user/key tables, org_id column | A |
| `crates/agentzero-storage/src/memory/sqlite.rs` | Scope queries by org_id | B |
| `crates/agentzero-core/src/types.rs` | Add org_id to ToolContext | B |
| `crates/agentzero-gateway/src/handlers.rs` | Admin endpoints | C |
| `crates/agentzero-cli/src/commands/org.rs` | New: org/user/apikey CLI commands | C |

## Estimated Scope

- **Sprint A (Identity + API Keys)**: ~2 weeks, ~400 lines, ~15 tests
- **Sprint B (Tenant Isolation)**: ~2 weeks, ~300 lines, ~10 tests
- **Sprint C (Management API)**: ~1 week, ~500 lines, ~12 tests

Total: ~5 weeks across 3 sprints, ~1200 lines, ~37 tests

## Risks

- **Schema migration complexity**: Adding `org_id` to an existing populated `memory` table requires careful migration. Default to empty string for existing data.
- **Performance**: Per-request API key lookup adds latency. Mitigate: in-memory cache of key hashes with TTL.
- **Breaking change**: Existing bearer token auth must continue working in single-tenant mode. Don't break existing deployments.
- **Scope creep**: OAuth2/OIDC integration, SSO, MFA — these are adjacent features that should be separate plans. This plan covers internal identity only.

## Dependencies on Other Plans

- **Plan 03 (Migrations)**: Must be implemented first — multi-tenancy adds 3 new tables and modifies existing schema.
- **Plan 04 (API Polish)**: Request correlation IDs should include `org_id` and `user_id` for audit trail.
- **Plan 02 (OpenTelemetry)**: OTel spans should include tenant context for per-tenant observability.

# Plan 03: Database Connection Pooling & Migration Framework

## Problem

### Connection Pooling
SQLite memory store uses `Mutex<Connection>` — a single connection with serialized access. Under concurrent load (multiple WebSocket clients, orchestrator agents, channel webhooks), all database operations queue behind one lock. This is the primary throughput bottleneck.

### Migration Framework
Schema changes are ad-hoc `ALTER TABLE` statements with duplicate-column detection (`match err ... "duplicate column"` pattern). There's no version tracking, no rollback capability, and no way to know which migrations have been applied to a given database. This is fragile and error-prone as the schema grows.

## Current State

### Connection handling (`crates/agentzero-storage/src/memory/sqlite.rs`)
```rust
pub struct SqliteMemoryStore {
    conn: Mutex<Connection>,  // single connection, serialized access
}
```

Every method does:
```rust
let conn = self.conn.lock().unwrap();
conn.execute(...);
```

### Migration pattern (same file)
```rust
fn migrate_privacy_columns(conn: &Connection) {
    // Attempt ALTER TABLE, ignore "duplicate column" errors
    if let Err(e) = conn.execute_batch("ALTER TABLE memory ADD COLUMN privacy_boundary ...") {
        let msg = e.to_string();
        if !msg.contains("duplicate column") { panic!(...); }
    }
}
```

Three separate migration functions called in `SqliteMemoryStore::new()`:
1. Table creation (inline in `new()`)
2. `migrate_privacy_columns()` — adds `privacy_boundary`, `source_channel`
3. `migrate_conversation_column()` — adds `conversation_id`

### Dependencies (`crates/agentzero-storage/Cargo.toml`)
- `rusqlite` with `bundled-sqlcipher` feature
- No connection pool crate

## Implementation

### Phase 1: Migration Framework

**New file: `crates/agentzero-storage/src/memory/migrations.rs`**

```rust
struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "create_memory_table",
        sql: "CREATE TABLE IF NOT EXISTS memory (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            tool_name TEXT,
            tool_input TEXT,
            tool_result TEXT,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    },
    Migration {
        version: 2,
        name: "add_privacy_columns",
        sql: "ALTER TABLE memory ADD COLUMN privacy_boundary TEXT NOT NULL DEFAULT '';
              ALTER TABLE memory ADD COLUMN source_channel TEXT NOT NULL DEFAULT '';",
    },
    Migration {
        version: 3,
        name: "add_conversation_id",
        sql: "ALTER TABLE memory ADD COLUMN conversation_id TEXT NOT NULL DEFAULT '';",
    },
];

/// Run all pending migrations. Creates _migrations table if needed.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )"
    )?;

    let applied: HashSet<u32> = conn
        .prepare("SELECT version FROM _migrations")?
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    for migration in MIGRATIONS {
        if !applied.contains(&migration.version) {
            // Split multi-statement SQL and execute each
            for stmt in migration.sql.split(';').filter(|s| !s.trim().is_empty()) {
                // Ignore "duplicate column" for backward compat with pre-migration DBs
                if let Err(e) = conn.execute(stmt.trim(), []) {
                    let msg = e.to_string();
                    if !msg.contains("duplicate column") {
                        return Err(e.into());
                    }
                }
            }
            conn.execute(
                "INSERT INTO _migrations (version, name) VALUES (?1, ?2)",
                params![migration.version, migration.name],
            )?;
        }
    }
    Ok(())
}
```

Key design decisions:
- **Forward-only**: No down migrations. Rollback = restore from backup. This is simpler and safer for SQLite.
- **Idempotent**: Duplicate column errors silently skipped for databases that had ad-hoc migrations pre-framework.
- **Version tracking**: `_migrations` table records what's applied. Underscore prefix avoids collision with user data.
- **Atomic per-migration**: Each migration could be wrapped in a transaction (SQLite supports DDL in transactions).

### Phase 2: Connection Pooling

**Replace `Mutex<Connection>` with `r2d2` pool**

Add to `crates/agentzero-storage/Cargo.toml`:
```toml
r2d2 = "0.8"
r2d2_sqlite = "0.25"
```

Note: `r2d2_sqlite` wraps `rusqlite`. Need to verify it supports the `bundled-sqlcipher` feature passthrough. If not, use `deadpool-sqlite` or a manual pool with `Arc<Mutex<Vec<Connection>>>`.

**Updated struct:**
```rust
pub struct SqliteMemoryStore {
    pool: r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>,
}
```

**Connection initialization hook** (runs on each new connection):
```rust
fn init_connection(conn: &Connection) -> Result<()> {
    if let Some(key) = &encryption_key {
        conn.pragma_update(None, "key", key)?;
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", "5000")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;  // safe with WAL
    Ok(())
}
```

**Pool configuration:**
```rust
let manager = SqliteConnectionManager::file(&db_path)
    .with_init(init_connection);
let pool = r2d2::Pool::builder()
    .max_size(4)       // SQLite WAL: 1 writer + N readers
    .min_idle(Some(1)) // keep at least 1 warm connection
    .build(manager)?;
```

Why `max_size = 4`:
- SQLite WAL mode allows concurrent readers but only one writer
- 4 connections = 1 likely writer + 3 concurrent readers
- More connections waste memory without benefit (SQLite writer lock serializes writes anyway)

**Update all methods:**
```rust
// Before:
let conn = self.conn.lock().unwrap();

// After:
let conn = self.pool.get()?;
```

This is a mechanical find-and-replace across all methods in the file.

### Phase 3: WAL Mode Optimization

Enable WAL mode for concurrent read performance:

```sql
PRAGMA journal_mode=WAL;     -- concurrent readers
PRAGMA busy_timeout=5000;    -- 5s retry on lock contention
PRAGMA synchronous=NORMAL;   -- safe with WAL, faster than FULL
PRAGMA cache_size=-8000;     -- 8MB page cache (default is 2MB)
```

These are set in the connection init hook (Phase 2), so every pooled connection gets them.

### Phase 4: Data Retention

**Config addition** (`crates/agentzero-config/src/model.rs`):
```rust
pub struct MemoryConfig {
    // ... existing fields ...
    pub retention_days: Option<u32>,  // None = keep forever (default)
}
```

**Purge function** (`crates/agentzero-storage/src/memory/sqlite.rs`):
```rust
pub fn purge_old_entries(&self, older_than_days: u32) -> Result<u64> {
    let conn = self.pool.get()?;
    let deleted = conn.execute(
        "DELETE FROM memory WHERE timestamp < datetime('now', ?1)",
        [format!("-{older_than_days} days")],
    )?;
    Ok(deleted as u64)
}
```

**Startup + periodic purge:**
- On `SqliteMemoryStore::new()`: if `retention_days` is set, run purge immediately
- Background task: `tokio::spawn` a loop that purges every 24 hours
- Log: `"Purged {n} memory entries older than {days} days"`

**CLI command:**
```
az memory purge --older-than 90
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `crates/agentzero-storage/Cargo.toml` | Add r2d2, r2d2_sqlite |
| `crates/agentzero-storage/src/memory/sqlite.rs` | Pool, WAL, remove ad-hoc migrations, add purge |
| `crates/agentzero-storage/src/memory/migrations.rs` | New: migration framework |
| `crates/agentzero-storage/src/memory/mod.rs` | Add `pub mod migrations;` |
| `crates/agentzero-config/src/model.rs` | Add `retention_days` to MemoryConfig |
| `crates/agentzero-cli/src/commands/memory.rs` | Add `purge` subcommand (new or existing) |
| `crates/agentzero-cli/src/cli.rs` | Register memory subcommand |

## Tests (~12 new)

### Migration tests
1. Fresh database: all migrations applied, `_migrations` table populated
2. Pre-existing database (no `_migrations` table): migrations detect existing columns, record as applied
3. Partial migration: only new migrations run
4. Version ordering: migrations applied in version order
5. Invalid SQL in migration: returns error, doesn't record as applied

### Pool tests
6. Concurrent reads: 4 threads read simultaneously without blocking
7. Write contention: concurrent writes succeed (one at a time, others wait)
8. Pool exhaustion: more requests than pool size → waits then succeeds
9. Connection reuse: connections returned to pool after use

### Data retention tests
10. Purge deletes old entries, keeps recent ones
11. Purge with no old entries: no error, returns 0
12. `retention_days = None`: no purge on startup

## Verification

1. `cargo test -p agentzero-storage` — all tests pass
2. Existing databases open without data loss (backward compat)
3. `_migrations` table created and populated on first run
4. `PRAGMA journal_mode` returns `wal` on active connection
5. Benchmark: concurrent read throughput improved vs `Mutex<Connection>`
6. `az memory purge --older-than 30` deletes old entries and reports count
7. All workspace tests pass

## Risks

- **r2d2_sqlite + bundled-sqlcipher compatibility**: Need to verify `r2d2_sqlite` correctly passes through the `bundled-sqlcipher` feature. If not, alternative: `deadpool-sqlite` or manual pool.
- **WAL mode + encryption**: Verify WAL mode works correctly with SQLCipher encryption. SQLCipher 4.x supports WAL.
- **Migration ordering**: If two developers add migrations concurrently, version numbers may conflict. Mitigate: use timestamps as versions, or sequential with merge conflict resolution.

use crate::cli::MemoryCommands;
use crate::command_core::{AgentZeroCommand, CommandContext};
use agentzero_config::load as load_config;
use agentzero_core::MemoryStore;
use agentzero_storage::memory::SqliteMemoryStore;
use agentzero_storage::StorageKey;
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;

pub struct MemoryCommand;

#[async_trait]
impl AgentZeroCommand for MemoryCommand {
    type Options = MemoryCommands;

    async fn run(ctx: &CommandContext, opts: Self::Options) -> anyhow::Result<()> {
        match opts {
            MemoryCommands::List {
                limit,
                offset,
                category: _,
                session: _,
                json,
            } => {
                let conn = sqlite_conn_for_cli(ctx)?;
                let mut stmt = conn.prepare(
                    "SELECT id, role, content, created_at
                     FROM memory
                     ORDER BY id DESC
                     LIMIT ?1 OFFSET ?2",
                )?;
                let rows = stmt.query_map(params![limit as i64, offset as i64], |row| {
                    Ok(MemoryRow {
                        id: row.get(0)?,
                        role: row.get(1)?,
                        content: row.get(2)?,
                        created_at: row.get(3)?,
                    })
                })?;

                let mut items = Vec::new();
                for row in rows {
                    items.push(row?);
                }

                if json {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    println!("No memory entries");
                } else {
                    println!("Memory entries ({}):", items.len());
                    for entry in items {
                        println!(
                            "  - #{} [{}] {}",
                            entry.id,
                            entry.role,
                            truncate(&entry.content, 96)
                        );
                    }
                }
            }
            MemoryCommands::Get { key, json } => {
                let conn = sqlite_conn_for_cli(ctx)?;
                let entry = if let Some(key) = key.as_deref() {
                    let pattern = format!("{key}%");
                    conn.query_row(
                        "SELECT id, role, content, created_at
                         FROM memory
                         WHERE CAST(id AS TEXT) LIKE ?1
                            OR role LIKE ?1
                            OR content LIKE ?1
                         ORDER BY id DESC
                         LIMIT 1",
                        [pattern],
                        |row| {
                            Ok(MemoryRow {
                                id: row.get(0)?,
                                role: row.get(1)?,
                                content: row.get(2)?,
                                created_at: row.get(3)?,
                            })
                        },
                    )
                    .optional()?
                } else {
                    conn.query_row(
                        "SELECT id, role, content, created_at
                         FROM memory
                         ORDER BY id DESC
                         LIMIT 1",
                        [],
                        |row| {
                            Ok(MemoryRow {
                                id: row.get(0)?,
                                role: row.get(1)?,
                                content: row.get(2)?,
                                created_at: row.get(3)?,
                            })
                        },
                    )
                    .optional()?
                };

                let Some(entry) = entry else {
                    anyhow::bail!("memory entry not found");
                };

                if json {
                    println!("{}", serde_json::to_string_pretty(&entry)?);
                } else {
                    println!("Memory entry #{}", entry.id);
                    println!("  role: {}", entry.role);
                    println!("  created_at: {}", entry.created_at);
                    println!("  content: {}", entry.content);
                }
            }
            MemoryCommands::Stats { json } => {
                let conn = sqlite_conn_for_cli(ctx)?;
                let total: i64 =
                    conn.query_row("SELECT COUNT(*) FROM memory", [], |row| row.get(0))?;
                let mut by_role = Vec::<RoleCount>::new();
                let mut stmt = conn.prepare(
                    "SELECT role, COUNT(*) as c
                     FROM memory
                     GROUP BY role
                     ORDER BY c DESC, role ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok(RoleCount {
                        role: row.get(0)?,
                        count: row.get(1)?,
                    })
                })?;
                for row in rows {
                    by_role.push(row?);
                }

                let stats = MemoryStats {
                    total_entries: total,
                    by_role,
                };

                if json {
                    println!("{}", serde_json::to_string_pretty(&stats)?);
                } else {
                    println!("Memory stats");
                    println!("  total entries: {}", stats.total_entries);
                    if stats.by_role.is_empty() {
                        println!("  roles: none");
                    } else {
                        println!("  by role:");
                        for role in stats.by_role {
                            println!("    - {}: {}", role.role, role.count);
                        }
                    }
                }
            }
            MemoryCommands::Clear {
                key,
                category: _,
                yes,
                json,
            } => {
                let conn = sqlite_conn_for_cli(ctx)?;
                if key.is_none() && !yes {
                    anyhow::bail!("refusing to clear all memory without --yes");
                }
                let scope = if key.is_some() { "key" } else { "all" };

                let cleared = if let Some(id) = key {
                    let pattern = format!("{id}%");
                    conn.execute(
                        "DELETE FROM memory
                         WHERE CAST(id AS TEXT) LIKE ?1
                            OR role LIKE ?1
                            OR content LIKE ?1",
                        [pattern],
                    )?
                } else {
                    conn.execute("DELETE FROM memory", [])?
                };

                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "cleared": cleared,
                            "scope": scope,
                        })
                    );
                } else {
                    println!(
                        "Cleared {cleared} memory entr{}",
                        if cleared == 1 { "y" } else { "ies" }
                    );
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct MemoryRow {
    id: i64,
    role: String,
    content: String,
    created_at: i64,
}

#[derive(Debug, Serialize)]
struct RoleCount {
    role: String,
    count: i64,
}

#[derive(Debug, Serialize)]
struct MemoryStats {
    total_entries: i64,
    by_role: Vec<RoleCount>,
}

fn truncate(input: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i >= max_chars {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn sqlite_conn_for_cli(ctx: &CommandContext) -> anyhow::Result<Connection> {
    let config = load_config(&ctx.config_path)?;
    if config.memory.backend != "sqlite" {
        anyhow::bail!(
            "memory command currently supports sqlite backend only; configured backend: {}",
            config.memory.backend
        );
    }

    let sqlite_path = resolve_sqlite_path(&ctx.config_path, &config.memory.sqlite_path);
    if let Some(parent) = sqlite_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(&sqlite_path)?;

    #[cfg(feature = "memory-sqlite")]
    {
        let config_dir = ctx
            .config_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let key = StorageKey::from_config_dir(config_dir)?;
        let hex_key: String = key.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
        conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\""))?;
    }

    ensure_memory_schema(&conn)?;
    Ok(conn)
}

fn ensure_memory_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS memory (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL DEFAULT (unixepoch())
        )",
        [],
    )?;
    Ok(())
}

fn resolve_sqlite_path(config_path: &std::path::Path, sqlite_path: &str) -> PathBuf {
    let candidate = PathBuf::from(sqlite_path);
    if candidate.is_absolute() {
        return candidate;
    }

    config_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(candidate)
}

pub async fn build_memory_store(ctx: &CommandContext) -> anyhow::Result<Box<dyn MemoryStore>> {
    let config = load_config(&ctx.config_path)?;
    let backend = config.memory.backend;

    match backend.as_str() {
        "sqlite" => {
            let config_dir = ctx
                .config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            let sqlite_path = resolve_sqlite_path(&ctx.config_path, &config.memory.sqlite_path);
            let key = StorageKey::from_config_dir(config_dir)?;
            Ok(Box::new(SqliteMemoryStore::open(sqlite_path, Some(&key))?))
        }
        "turso" => build_turso_store().await,
        other => Err(anyhow::anyhow!(
            "unsupported AGENTZERO_MEMORY_BACKEND `{other}`; expected `sqlite` or `turso`"
        )),
    }
}

#[cfg(feature = "memory-turso")]
async fn build_turso_store() -> anyhow::Result<Box<dyn MemoryStore>> {
    let settings = agentzero_storage::memory::TursoSettings::from_env()?;
    let store = agentzero_storage::memory::TursoMemoryStore::connect(settings).await?;
    Ok(Box::new(store))
}

#[cfg(not(feature = "memory-turso"))]
async fn build_turso_store() -> anyhow::Result<Box<dyn MemoryStore>> {
    Err(anyhow::anyhow!(
        "turso backend requested but agentzero-cli was built without `memory-turso` feature"
    ))
}

#[cfg(test)]
mod tests {
    use super::{build_memory_store, MemoryCommand};
    use crate::cli::MemoryCommands;
    use crate::command_core::{AgentZeroCommand, CommandContext};
    use agentzero_core::MemoryEntry;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be after unix epoch")
            .as_nanos();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "agentzero-memory-cli-{}-{nanos}-{seq}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    fn write_config(path: &PathBuf, memory_backend: &str, sqlite_path: &str) {
        // Use forward slashes so Windows backslashes don't become TOML escapes
        let safe_path = sqlite_path.replace('\\', "/");
        let config = format!(
            "[memory]\nbackend = \"{memory_backend}\"\nsqlite_path = \"{safe_path}\"\n\n[security]\nallowed_root = \".\"\nallowed_commands = [\"echo\"]\n"
        );
        fs::write(path, config).expect("config should be written");
    }

    #[tokio::test]
    async fn sqlite_backend_stays_usable_with_bad_turso_env() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("agentzero-test.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let prev_url = std::env::var("TURSO_DATABASE_URL").ok();
        let prev_token = std::env::var("TURSO_AUTH_TOKEN").ok();
        std::env::set_var("TURSO_DATABASE_URL", "http://not-supported");
        std::env::set_var("TURSO_AUTH_TOKEN", "bad token");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: config_path.clone(),
        };
        let store = build_memory_store(&ctx)
            .await
            .expect("sqlite backend should build");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "hello".to_string(),
                ..Default::default()
            })
            .await
            .expect("sqlite append should work");
        let recent = store.recent(1).await.expect("sqlite recent should work");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].content, "hello");

        match prev_url {
            Some(v) => std::env::set_var("TURSO_DATABASE_URL", v),
            None => std::env::remove_var("TURSO_DATABASE_URL"),
        }
        match prev_token {
            Some(v) => std::env::set_var("TURSO_AUTH_TOKEN", v),
            None => std::env::remove_var("TURSO_AUTH_TOKEN"),
        }
        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[cfg(not(feature = "memory-turso"))]
    #[tokio::test]
    async fn turso_backend_reports_unavailable_when_feature_disabled() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        write_config(&config_path, "turso", "./ignored.db");

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };
        let result = build_memory_store(&ctx).await;
        let err = match result {
            Ok(_) => panic!("turso feature should be unavailable"),
            Err(err) => err,
        };
        assert!(err
            .to_string()
            .contains("built without `memory-turso` feature"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn memory_list_command_success_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("memory-list.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path: config_path.clone(),
        };
        let store = build_memory_store(&ctx).await.expect("sqlite should build");
        store
            .append(MemoryEntry {
                role: "user".to_string(),
                content: "memory-list-entry".to_string(),
                ..Default::default()
            })
            .await
            .expect("append should succeed");

        MemoryCommand::run(
            &ctx,
            MemoryCommands::List {
                limit: 10,
                offset: 0,
                category: None,
                session: None,
                json: true,
            },
        )
        .await
        .expect("memory list should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn memory_clear_without_yes_fails_negative_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("memory-clear.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };
        let err = MemoryCommand::run(
            &ctx,
            MemoryCommands::Clear {
                key: None,
                category: None,
                yes: false,
                json: false,
            },
        )
        .await
        .expect_err("clear without --yes should fail");
        assert!(err
            .to_string()
            .contains("refusing to clear all memory without --yes"));

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn memory_list_empty_db_success_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("memory-empty.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };

        MemoryCommand::run(
            &ctx,
            MemoryCommands::List {
                limit: 10,
                offset: 0,
                category: None,
                session: None,
                json: true,
            },
        )
        .await
        .expect("list on empty db should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }

    #[tokio::test]
    async fn memory_clear_empty_db_success_path() {
        let dir = temp_dir();
        let config_path = dir.join("agentzero.toml");
        let sqlite_path = dir.join("memory-clear-empty.db");
        write_config(
            &config_path,
            "sqlite",
            sqlite_path.to_str().expect("sqlite path should be utf8"),
        );

        let ctx = CommandContext {
            workspace_root: dir.clone(),
            data_dir: dir.clone(),
            config_path,
        };

        MemoryCommand::run(
            &ctx,
            MemoryCommands::Clear {
                key: None,
                category: None,
                yes: true,
                json: false,
            },
        )
        .await
        .expect("clear on empty db should succeed");

        fs::remove_dir_all(dir).expect("temp dir should be removed");
    }
}

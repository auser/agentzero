//! Database connector template.
//!
//! Supports SQLite (via rusqlite) with schema discovery from `sqlite_master`
//! and `PRAGMA table_info`. Postgres support uses shell-based `psql` commands
//! via the existing `DynamicToolStrategy::Shell`.

use crate::templates::{ConnectorTemplate, ReadRequest, ReadResult, WriteRequest, WriteResult};
use crate::{
    AuthConfig, ConnectorCaps, ConnectorConfig, ConnectorManifest, ConnectorType, EntitySchema,
    FieldDef, FieldType, SyncError,
};
use async_trait::async_trait;

/// Template for database connectors (SQLite, Postgres).
pub struct DatabaseTemplate;

#[async_trait]
impl ConnectorTemplate for DatabaseTemplate {
    fn manifest(&self, config: &ConnectorConfig) -> anyhow::Result<ConnectorManifest> {
        let _db_type = infer_db_type(config);

        Ok(ConnectorManifest {
            name: config.name.clone(),
            connector_type: ConnectorType::Database,
            auth: config.auth.clone(),
            entities: vec![],
            capabilities: ConnectorCaps {
                read: true,
                write: true,
                list: true,
                search: true,
                subscribe: false,
                discover_schema: true,
            },
        })
    }

    async fn discover_schema(&self, config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>> {
        let db_type = infer_db_type(config);

        match db_type {
            DbType::Sqlite => discover_sqlite(config),
            DbType::Postgres => discover_postgres_via_info_schema(config).await,
        }
    }

    async fn read_records(
        &self,
        config: &ConnectorConfig,
        request: &ReadRequest,
    ) -> anyhow::Result<ReadResult> {
        let db_type = infer_db_type(config);
        match db_type {
            DbType::Sqlite => read_sqlite_records(config, request),
            DbType::Postgres => read_postgres_records(config, request).await,
        }
    }

    async fn write_records(
        &self,
        config: &ConnectorConfig,
        request: &WriteRequest,
    ) -> anyhow::Result<WriteResult> {
        let db_type = infer_db_type(config);
        match db_type {
            DbType::Sqlite => write_sqlite_records(config, request),
            DbType::Postgres => write_postgres_records(config, request).await,
        }
    }
}

// ── SQLite read/write ────────────────────────────────────────────────

fn read_sqlite_records(
    config: &ConnectorConfig,
    request: &ReadRequest,
) -> anyhow::Result<ReadResult> {
    let path = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("SQLite connector requires `path` or connection string"))?;

    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let table = &request.entity;
    // Use rowid for cursor-based pagination.
    let (sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match &request.cursor {
        Some(cursor) => {
            let cursor_val: i64 = cursor.parse().unwrap_or(0);
            (
                format!(
                    "SELECT *, rowid as _rowid FROM \"{}\" WHERE rowid > ? ORDER BY rowid LIMIT ?",
                    table
                ),
                vec![
                    Box::new(cursor_val) as Box<dyn rusqlite::types::ToSql>,
                    Box::new(request.batch_size as i64),
                ],
            )
        }
        None => (
            format!(
                "SELECT *, rowid as _rowid FROM \"{}\" ORDER BY rowid LIMIT ?",
                table
            ),
            vec![Box::new(request.batch_size as i64) as Box<dyn rusqlite::types::ToSql>],
        ),
    };

    let mut stmt = conn.prepare(&sql)?;
    let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| &**p).collect();

    let rows: Vec<serde_json::Value> = stmt
        .query_map(param_refs.as_slice(), |row| {
            let mut obj = serde_json::Map::new();
            for (i, col) in column_names.iter().enumerate() {
                let val: rusqlite::types::Value = row.get(i)?;
                let json_val = match val {
                    rusqlite::types::Value::Null => serde_json::Value::Null,
                    rusqlite::types::Value::Integer(n) => serde_json::json!(n),
                    rusqlite::types::Value::Real(f) => serde_json::json!(f),
                    rusqlite::types::Value::Text(s) => serde_json::json!(s),
                    rusqlite::types::Value::Blob(b) => {
                        serde_json::json!(base64::Engine::encode(
                            &base64::engine::general_purpose::STANDARD,
                            &b
                        ))
                    }
                };
                obj.insert(col.clone(), json_val);
            }
            Ok(serde_json::Value::Object(obj))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let next_cursor = if rows.len() == request.batch_size as usize {
        rows.last()
            .and_then(|r| r.get("_rowid"))
            .map(|v| v.to_string())
    } else {
        None
    };

    Ok(ReadResult {
        records: rows,
        next_cursor,
    })
}

fn write_sqlite_records(
    config: &ConnectorConfig,
    request: &WriteRequest,
) -> anyhow::Result<WriteResult> {
    let path = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("SQLite connector requires `path` or connection string"))?;

    let conn = rusqlite::Connection::open(&path)?;
    let table = &request.entity;
    let pk = &request.primary_key;

    let mut written = 0u64;
    let mut errors = Vec::new();

    // Use a transaction for batch writes.
    let tx = conn.unchecked_transaction()?;

    for record in &request.records {
        let Some(obj) = record.as_object() else {
            errors.push(SyncError {
                record_key: "unknown".to_string(),
                message: "record is not a JSON object".to_string(),
            });
            continue;
        };

        let record_key = obj
            .get(pk)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // Build INSERT OR REPLACE (upsert).
        let columns: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        let placeholders: Vec<String> = (0..columns.len()).map(|i| format!("?{}", i + 1)).collect();

        let sql = format!(
            "INSERT OR REPLACE INTO \"{}\" ({}) VALUES ({})",
            table,
            columns
                .iter()
                .map(|c| format!("\"{}\"", c))
                .collect::<Vec<_>>()
                .join(", "),
            placeholders.join(", ")
        );

        let values: Vec<Box<dyn rusqlite::types::ToSql>> = obj
            .values()
            .map(|v| -> Box<dyn rusqlite::types::ToSql> {
                match v {
                    serde_json::Value::Null => Box::new(rusqlite::types::Value::Null),
                    serde_json::Value::Bool(b) => Box::new(*b as i64),
                    serde_json::Value::Number(n) => {
                        if let Some(i) = n.as_i64() {
                            Box::new(i)
                        } else {
                            Box::new(n.as_f64().unwrap_or(0.0))
                        }
                    }
                    serde_json::Value::String(s) => Box::new(s.clone()),
                    other => Box::new(other.to_string()),
                }
            })
            .collect();

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = values.iter().map(|p| &**p).collect();
        match tx.execute(&sql, param_refs.as_slice()) {
            Ok(_) => written += 1,
            Err(e) => {
                errors.push(SyncError {
                    record_key,
                    message: e.to_string(),
                });
            }
        }
    }

    tx.commit()?;

    Ok(WriteResult {
        written,
        skipped: 0,
        errors,
    })
}

// ── Postgres read/write (via psql) ───────────────────────────────────

async fn read_postgres_records(
    config: &ConnectorConfig,
    request: &ReadRequest,
) -> anyhow::Result<ReadResult> {
    let conn_str = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("Postgres connector requires a connection string"))?;

    let cursor_clause = match &request.cursor {
        Some(cursor) => format!("WHERE ctid > '({},0)'::tid", cursor),
        None => String::new(),
    };

    let query = format!(
        "SELECT row_to_json(t) FROM (SELECT *, ctid FROM \"{}\" {} ORDER BY ctid LIMIT {}) t",
        request.entity, cursor_clause, request.batch_size
    );

    let output = tokio::process::Command::new("psql")
        .arg(&conn_str)
        .arg("-t")
        .arg("-A")
        .arg("-c")
        .arg(&query)
        .output()
        .await?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("psql read failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut records = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            records.push(val);
        }
    }

    let next_cursor = if records.len() == request.batch_size as usize {
        // Extract ctid from last record for cursor.
        records
            .last()
            .and_then(|r| r.get("ctid"))
            .map(|v| v.to_string().trim_matches('"').to_string())
    } else {
        None
    };

    Ok(ReadResult {
        records,
        next_cursor,
    })
}

async fn write_postgres_records(
    config: &ConnectorConfig,
    request: &WriteRequest,
) -> anyhow::Result<WriteResult> {
    let conn_str = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("Postgres connector requires a connection string"))?;

    let mut written = 0u64;
    let mut errors = Vec::new();

    for record in &request.records {
        let Some(obj) = record.as_object() else {
            errors.push(SyncError {
                record_key: "unknown".to_string(),
                message: "record is not a JSON object".to_string(),
            });
            continue;
        };

        let record_key = obj
            .get(&request.primary_key)
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let columns: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
        let values: Vec<String> = obj.values().map(escape_pg_value).collect();

        let cols_str = columns
            .iter()
            .map(|c| format!("\"{}\"", c.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(", ");
        let vals_str = values.join(", ");
        let update_str = columns
            .iter()
            .map(|c| {
                let escaped = c.replace('"', "\"\"");
                format!("\"{}\" = EXCLUDED.\"{}\"", escaped, escaped)
            })
            .collect::<Vec<_>>()
            .join(", ");

        let entity_escaped = request.entity.replace('"', "\"\"");
        let pk_escaped = request.primary_key.replace('"', "\"\"");

        let sql = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT (\"{}\") DO UPDATE SET {}",
            entity_escaped, cols_str, vals_str, pk_escaped, update_str
        );

        let output = tokio::process::Command::new("psql")
            .arg(&conn_str)
            .arg("-c")
            .arg(&sql)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => written += 1,
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                errors.push(SyncError {
                    record_key,
                    message: stderr.to_string(),
                });
            }
            Err(e) => {
                errors.push(SyncError {
                    record_key,
                    message: e.to_string(),
                });
            }
        }
    }

    Ok(WriteResult {
        written,
        skipped: 0,
        errors,
    })
}

/// Safely escape a JSON value for embedding in a PostgreSQL SQL statement.
///
/// Uses Postgres dollar-quoting (`$val$...$val$`) for strings to avoid
/// SQL injection via single-quote escaping edge cases.
pub fn escape_pg_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => {
            // Use dollar-quoting to avoid injection. If the string contains
            // `$val$`, use a different tag.
            let tag = if s.contains("$val$") { "$v2$" } else { "$val$" };
            format!("{tag}{s}{tag}")
        }
        // For JSON objects/arrays, cast as jsonb.
        other => {
            let json_str = other.to_string();
            let tag = if json_str.contains("$val$") {
                "$v2$"
            } else {
                "$val$"
            };
            format!("{tag}{json_str}{tag}::jsonb")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DbType {
    Sqlite,
    Postgres,
}

fn infer_db_type(config: &ConnectorConfig) -> DbType {
    // Check explicit type hint.
    if let Some(db_type) = config.settings.get("db_type").and_then(|v| v.as_str()) {
        match db_type {
            "postgres" | "postgresql" => return DbType::Postgres,
            "sqlite" => return DbType::Sqlite,
            _ => {}
        }
    }

    // Infer from connection string.
    let conn_str = connection_string(config).unwrap_or_default();
    if conn_str.starts_with("postgres://") || conn_str.starts_with("postgresql://") {
        DbType::Postgres
    } else {
        DbType::Sqlite
    }
}

fn connection_string(config: &ConnectorConfig) -> Option<String> {
    // Try direct path first (for SQLite file paths).
    if let Some(path) = config.settings.get("path").and_then(|v| v.as_str()) {
        return Some(path.to_string());
    }

    // Try connection_string_env.
    if let AuthConfig::ConnectionString {
        connection_string_env,
    } = &config.auth
    {
        return std::env::var(connection_string_env).ok();
    }

    // Try connection_string in settings.
    if let Some(cs_env) = config
        .settings
        .get("connection_string_env")
        .and_then(|v| v.as_str())
    {
        return std::env::var(cs_env).ok();
    }

    None
}

/// Discover schema from a SQLite database.
fn discover_sqlite(config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>> {
    let path = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("SQLite connector requires `path` or connection string"))?;

    let conn = rusqlite::Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    // Get all user tables.
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )?;
    let table_names: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut entities = Vec::new();

    for table_name in &table_names {
        let mut info_stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table_name))?;
        let fields: Vec<FieldDef> = info_stmt
            .query_map([], |row| {
                let name: String = row.get(1)?;
                let type_str: String = row.get(2)?;
                let notnull: bool = row.get(3)?;
                let pk: i32 = row.get(5)?;
                Ok((name, type_str, notnull, pk))
            })?
            .filter_map(|r| r.ok())
            .map(|(name, type_str, notnull, _pk)| FieldDef {
                name,
                field_type: sqlite_type_to_field_type(&type_str),
                required: notnull,
                description: String::new(),
            })
            .collect();

        // Find primary key from the fields we already parsed.
        let primary_key = {
            let mut pk_stmt = conn.prepare(&format!("PRAGMA table_info(\"{}\")", table_name))?;
            let pk_results: Vec<(String, i32)> = pk_stmt
                .query_map([], |row| {
                    let name: String = row.get(1)?;
                    let pk: i32 = row.get(5)?;
                    Ok((name, pk))
                })?
                .filter_map(|r| r.ok())
                .collect();
            pk_results
                .into_iter()
                .find(|(_, pk)| *pk > 0)
                .map(|(name, _)| name)
                .unwrap_or_else(|| "rowid".to_string())
        };

        entities.push(EntitySchema {
            name: table_name.clone(),
            fields,
            primary_key,
            json_schema: serde_json::json!({}),
        });
    }

    Ok(entities)
}

/// Map SQLite type affinity to FieldType.
fn sqlite_type_to_field_type(type_str: &str) -> FieldType {
    let upper = type_str.to_uppercase();
    if upper.contains("INT") {
        FieldType::Integer
    } else if upper.contains("REAL") || upper.contains("FLOA") || upper.contains("DOUB") {
        FieldType::Number
    } else if upper.contains("BOOL") {
        FieldType::Boolean
    } else if upper.contains("DATE") || upper.contains("TIME") {
        FieldType::DateTime
    } else if upper.contains("BLOB") {
        FieldType::Binary
    } else if upper.contains("JSON") {
        FieldType::Json
    } else {
        // TEXT or unrecognized → String (SQLite's flexible typing).
        FieldType::String
    }
}

/// Discover schema from Postgres via `information_schema`.
///
/// Uses a shell command (`psql`) to query the schema, keeping the
/// implementation dependency-light (no `sqlx` or `tokio-postgres` needed).
async fn discover_postgres_via_info_schema(
    config: &ConnectorConfig,
) -> anyhow::Result<Vec<EntitySchema>> {
    let conn_str = connection_string(config)
        .ok_or_else(|| anyhow::anyhow!("Postgres connector requires a connection string"))?;

    let query = r#"
        SELECT table_name, column_name, data_type, is_nullable,
               COALESCE(
                   (SELECT 'YES' FROM information_schema.table_constraints tc
                    JOIN information_schema.key_column_usage kcu
                    ON tc.constraint_name = kcu.constraint_name
                    WHERE tc.table_name = c.table_name
                      AND kcu.column_name = c.column_name
                      AND tc.constraint_type = 'PRIMARY KEY'),
                   'NO'
               ) as is_pk
        FROM information_schema.columns c
        WHERE table_schema = 'public'
        ORDER BY table_name, ordinal_position
    "#;

    let output = tokio::process::Command::new("psql")
        .arg(&conn_str)
        .arg("-t")
        .arg("-A")
        .arg("-F")
        .arg("|")
        .arg("-c")
        .arg(query)
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run psql: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("psql failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_psql_output(&stdout)
}

/// Parse psql pipe-delimited output into entity schemas.
fn parse_psql_output(output: &str) -> anyhow::Result<Vec<EntitySchema>> {
    let mut tables: std::collections::HashMap<String, (Vec<FieldDef>, String)> =
        std::collections::HashMap::new();

    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() < 5 {
            continue;
        }

        let table_name = parts[0].trim().to_string();
        let column_name = parts[1].trim().to_string();
        let data_type = parts[2].trim();
        let is_nullable = parts[3].trim();
        let is_pk = parts[4].trim();

        let entry = tables
            .entry(table_name)
            .or_insert_with(|| (Vec::new(), "id".to_string()));

        entry.0.push(FieldDef {
            name: column_name.clone(),
            field_type: postgres_type_to_field_type(data_type),
            required: is_nullable == "NO",
            description: String::new(),
        });

        if is_pk == "YES" {
            entry.1 = column_name;
        }
    }

    Ok(tables
        .into_iter()
        .map(|(name, (fields, primary_key))| EntitySchema {
            name,
            fields,
            primary_key,
            json_schema: serde_json::json!({}),
        })
        .collect())
}

/// Map Postgres data type names to FieldType.
fn postgres_type_to_field_type(pg_type: &str) -> FieldType {
    match pg_type {
        "integer" | "bigint" | "smallint" | "serial" | "bigserial" => FieldType::Integer,
        "real" | "double precision" | "numeric" | "decimal" | "money" => FieldType::Number,
        "boolean" => FieldType::Boolean,
        "timestamp without time zone" | "timestamp with time zone" | "date" | "time" => {
            FieldType::DateTime
        }
        "json" | "jsonb" => FieldType::Json,
        "bytea" => FieldType::Binary,
        _ => FieldType::String,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::NamedTempFile;

    fn sqlite_config(path: &str) -> ConnectorConfig {
        let mut settings = HashMap::new();
        settings.insert(
            "path".to_string(),
            serde_json::Value::String(path.to_string()),
        );
        ConnectorConfig {
            name: "test_db".to_string(),
            connector_type: ConnectorType::Database,
            settings,
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: crate::RateLimitConfig::default(),
            pagination: crate::PaginationStrategy::None,
            batch_size: 100,
        }
    }

    #[test]
    fn sqlite_type_mapping() {
        assert_eq!(sqlite_type_to_field_type("INTEGER"), FieldType::Integer);
        assert_eq!(sqlite_type_to_field_type("REAL"), FieldType::Number);
        assert_eq!(sqlite_type_to_field_type("TEXT"), FieldType::String);
        assert_eq!(sqlite_type_to_field_type("BLOB"), FieldType::Binary);
        assert_eq!(sqlite_type_to_field_type("BOOLEAN"), FieldType::Boolean);
        assert_eq!(sqlite_type_to_field_type("DATETIME"), FieldType::DateTime);
        assert_eq!(sqlite_type_to_field_type("JSON"), FieldType::Json);
        assert_eq!(sqlite_type_to_field_type("VARCHAR(255)"), FieldType::String);
    }

    #[test]
    fn postgres_type_mapping() {
        assert_eq!(postgres_type_to_field_type("integer"), FieldType::Integer);
        assert_eq!(
            postgres_type_to_field_type("double precision"),
            FieldType::Number
        );
        assert_eq!(postgres_type_to_field_type("boolean"), FieldType::Boolean);
        assert_eq!(
            postgres_type_to_field_type("timestamp with time zone"),
            FieldType::DateTime
        );
        assert_eq!(postgres_type_to_field_type("jsonb"), FieldType::Json);
        assert_eq!(postgres_type_to_field_type("bytea"), FieldType::Binary);
        assert_eq!(
            postgres_type_to_field_type("character varying"),
            FieldType::String
        );
    }

    #[test]
    fn discover_sqlite_schema() {
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_str().expect("path");

        // Create a test table.
        let conn = rusqlite::Connection::open(path).expect("open");
        conn.execute_batch(
            "CREATE TABLE orders (
                id INTEGER PRIMARY KEY,
                customer_name TEXT NOT NULL,
                total REAL,
                created_at DATETIME,
                metadata JSON
            );
            CREATE TABLE products (
                sku TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                price REAL NOT NULL
            );",
        )
        .expect("create tables");
        drop(conn);

        let config = sqlite_config(path);
        let entities = discover_sqlite(&config).expect("discover");

        assert_eq!(entities.len(), 2);

        let orders = entities
            .iter()
            .find(|e| e.name == "orders")
            .expect("orders");
        assert_eq!(orders.primary_key, "id");
        assert_eq!(orders.fields.len(), 5);

        let id_field = orders.fields.iter().find(|f| f.name == "id").expect("id");
        assert_eq!(id_field.field_type, FieldType::Integer);

        let name_field = orders
            .fields
            .iter()
            .find(|f| f.name == "customer_name")
            .expect("customer_name");
        assert_eq!(name_field.field_type, FieldType::String);
        assert!(name_field.required);

        let products = entities
            .iter()
            .find(|e| e.name == "products")
            .expect("products");
        assert_eq!(products.primary_key, "sku");
    }

    #[test]
    fn infer_db_type_from_settings() {
        let mut config = sqlite_config("/tmp/test.db");
        assert!(matches!(infer_db_type(&config), DbType::Sqlite));

        config.settings.insert(
            "db_type".to_string(),
            serde_json::Value::String("postgres".to_string()),
        );
        assert!(matches!(infer_db_type(&config), DbType::Postgres));
    }

    #[test]
    fn parse_psql_output_basic() {
        let output = "\
users|id|integer|NO|YES\n\
users|name|character varying|NO|NO\n\
users|email|character varying|YES|NO\n\
orders|id|bigint|NO|YES\n\
orders|total|numeric|YES|NO\n\
orders|created_at|timestamp with time zone|YES|NO\n";

        let entities = parse_psql_output(output).expect("parse");
        assert_eq!(entities.len(), 2);

        let users = entities.iter().find(|e| e.name == "users").expect("users");
        assert_eq!(users.primary_key, "id");
        assert_eq!(users.fields.len(), 3);

        let orders = entities
            .iter()
            .find(|e| e.name == "orders")
            .expect("orders");
        assert_eq!(orders.fields.len(), 3);
        let created = orders
            .fields
            .iter()
            .find(|f| f.name == "created_at")
            .expect("created_at");
        assert_eq!(created.field_type, FieldType::DateTime);
    }

    #[test]
    fn manifest_basic() {
        let config = sqlite_config("/tmp/test.db");
        let template = DatabaseTemplate;
        let manifest = template.manifest(&config).expect("manifest");
        assert_eq!(manifest.name, "test_db");
        assert!(manifest.capabilities.discover_schema);
        assert!(manifest.capabilities.write);
    }
}

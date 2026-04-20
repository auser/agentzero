//! End-to-end integration tests for the connector framework.
//!
//! Tests verify that data actually flows between connectors using
//! the read → transform → write pipeline.

use agentzero_connectors::registry::ConnectorRegistry;
use agentzero_connectors::sync_engine;
use agentzero_connectors::templates::{ReadRequest, WriteRequest};
use agentzero_connectors::*;
use std::collections::HashMap;

fn csv_config(name: &str, path: &str) -> ConnectorConfig {
    let mut settings = HashMap::new();
    settings.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    ConnectorConfig {
        name: name.to_string(),
        connector_type: ConnectorType::File,
        settings,
        auth: AuthConfig::None,
        privacy_boundary: String::new(),
        rate_limit: RateLimitConfig::default(),
        pagination: PaginationStrategy::None,
        batch_size: 100,
    }
}

fn sqlite_config(name: &str, path: &str) -> ConnectorConfig {
    let mut settings = HashMap::new();
    settings.insert(
        "path".to_string(),
        serde_json::Value::String(path.to_string()),
    );
    ConnectorConfig {
        name: name.to_string(),
        connector_type: ConnectorType::Database,
        settings,
        auth: AuthConfig::None,
        privacy_boundary: String::new(),
        rate_limit: RateLimitConfig::default(),
        pagination: PaginationStrategy::None,
        batch_size: 100,
    }
}

#[tokio::test]
async fn csv_to_sqlite_sync() {
    let dir = tempfile::tempdir().expect("tmpdir");

    // Create source CSV.
    let csv_path = dir.path().join("products.csv");
    std::fs::write(
        &csv_path,
        "id,name,price\n1,Widget,9.99\n2,Gadget,19.99\n3,Doohickey,4.50\n",
    )
    .expect("write csv");

    // Create target SQLite database.
    let db_path = dir.path().join("target.db");
    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    conn.execute_batch(
        "CREATE TABLE products (
            product_id INTEGER PRIMARY KEY,
            product_name TEXT NOT NULL,
            amount REAL
        );",
    )
    .expect("create table");
    drop(conn);

    // Set up registry.
    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![
        csv_config("csv_source", csv_path.to_str().expect("path")),
        sqlite_config("db_target", db_path.to_str().expect("path")),
    ]);

    // Discover both schemas.
    let (csv_entities, _) = registry.discover("csv_source").await.expect("discover csv");
    let (db_entities, _) = registry.discover("db_target").await.expect("discover db");

    assert!(!csv_entities.is_empty(), "CSV should have entities");
    assert!(!db_entities.is_empty(), "DB should have entities");

    // Create a data link with field mappings.
    let link = DataLink {
        id: "test-link-1".to_string(),
        name: "csv_to_db".to_string(),
        source: DataEndpoint {
            connector: "csv_source".to_string(),
            entity: "products".to_string(),
        },
        target: DataEndpoint {
            connector: "db_target".to_string(),
            entity: "products".to_string(),
        },
        field_mappings: vec![
            FieldMapping {
                source_field: "id".to_string(),
                target_field: "product_id".to_string(),
                transform: None,
            },
            FieldMapping {
                source_field: "name".to_string(),
                target_field: "product_name".to_string(),
                transform: None,
            },
            FieldMapping {
                source_field: "price".to_string(),
                target_field: "amount".to_string(),
                transform: None,
            },
        ],
        sync_mode: SyncMode::OnDemand,
        transform: None,
        last_sync_cursor: None,
        last_sync_at: 0,
    };
    registry.upsert_link(link.clone());

    // Read from source.
    let read_result = registry
        .read_records(
            "csv_source",
            &ReadRequest {
                entity: "products".to_string(),
                cursor: None,
                batch_size: 100,
            },
        )
        .await
        .expect("read");

    assert_eq!(read_result.records.len(), 3);

    // Transform records.
    let (transformed, errors) = sync_engine::transform_batch(&link, &read_result.records);
    assert!(errors.is_empty(), "no transform errors: {:?}", errors);
    assert_eq!(transformed.len(), 3);

    // Verify field mapping worked.
    assert_eq!(transformed[0]["product_id"], 1);
    assert_eq!(transformed[0]["product_name"], "Widget");
    assert_eq!(transformed[0]["amount"], 9.99);

    // Write to target.
    let write_result = registry
        .write_records(
            "db_target",
            &WriteRequest {
                entity: "products".to_string(),
                records: transformed,
                primary_key: "product_id".to_string(),
            },
        )
        .await
        .expect("write");

    assert_eq!(write_result.written, 3);
    assert!(write_result.errors.is_empty());

    // Verify data landed in SQLite.
    let conn = rusqlite::Connection::open(&db_path).expect("open db");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM products", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 3);

    let (name, amount): (String, f64) = conn
        .query_row(
            "SELECT product_name, amount FROM products WHERE product_id = 2",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("query");
    assert_eq!(name, "Gadget");
    assert!((amount - 19.99).abs() < 0.001);
}

#[tokio::test]
async fn idempotent_upsert() {
    let dir = tempfile::tempdir().expect("tmpdir");

    // Create a JSON source.
    let json_path = dir.path().join("orders.json");
    std::fs::write(
        &json_path,
        r#"[{"id": 1, "status": "pending"}, {"id": 2, "status": "shipped"}]"#,
    )
    .expect("write json");

    // Create target JSON.
    let target_path = dir.path().join("target.json");
    std::fs::write(&target_path, "[]").expect("write target");

    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![
        csv_config("src", json_path.to_str().expect("path")),
        csv_config("dst", target_path.to_str().expect("path")),
    ]);

    // Override configs to be JSON type (reusing csv_config but file extension determines behavior).
    let link = DataLink {
        id: "upsert-test".to_string(),
        name: "upsert".to_string(),
        source: DataEndpoint {
            connector: "src".to_string(),
            entity: "orders".to_string(),
        },
        target: DataEndpoint {
            connector: "dst".to_string(),
            entity: "orders".to_string(),
        },
        field_mappings: vec![
            FieldMapping {
                source_field: "id".to_string(),
                target_field: "id".to_string(),
                transform: None,
            },
            FieldMapping {
                source_field: "status".to_string(),
                target_field: "status".to_string(),
                transform: None,
            },
        ],
        sync_mode: SyncMode::OnDemand,
        transform: None,
        last_sync_cursor: None,
        last_sync_at: 0,
    };

    // First sync: read from source, write to target.
    let read_result = registry
        .read_records(
            "src",
            &ReadRequest {
                entity: "orders".to_string(),
                cursor: None,
                batch_size: 100,
            },
        )
        .await
        .expect("read");
    let (transformed, _) = sync_engine::transform_batch(&link, &read_result.records);
    let w1 = registry
        .write_records(
            "dst",
            &WriteRequest {
                entity: "orders".to_string(),
                records: transformed.clone(),
                primary_key: "id".to_string(),
            },
        )
        .await
        .expect("write 1");
    assert_eq!(w1.written, 2);

    // Second sync (idempotent): same data should upsert without duplicates.
    let w2 = registry
        .write_records(
            "dst",
            &WriteRequest {
                entity: "orders".to_string(),
                records: transformed,
                primary_key: "id".to_string(),
            },
        )
        .await
        .expect("write 2");
    assert_eq!(w2.written, 2);

    // Verify no duplicates — target should still have exactly 2 records.
    let target_content = std::fs::read_to_string(&target_path).expect("read target");
    let target_records: Vec<serde_json::Value> =
        serde_json::from_str(&target_content).expect("parse");
    assert_eq!(
        target_records.len(),
        2,
        "upsert should not create duplicates"
    );
}

#[tokio::test]
async fn sqlite_read_with_pagination() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let db_path = dir.path().join("paginated.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open");
    conn.execute_batch(
        "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
         INSERT INTO items VALUES (1, 'a');
         INSERT INTO items VALUES (2, 'b');
         INSERT INTO items VALUES (3, 'c');
         INSERT INTO items VALUES (4, 'd');
         INSERT INTO items VALUES (5, 'e');",
    )
    .expect("seed");
    drop(conn);

    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![sqlite_config("db", db_path.to_str().expect("path"))]);

    // Read first batch of 2.
    let r1 = registry
        .read_records(
            "db",
            &ReadRequest {
                entity: "items".to_string(),
                cursor: None,
                batch_size: 2,
            },
        )
        .await
        .expect("read 1");
    assert_eq!(r1.records.len(), 2);
    assert!(r1.next_cursor.is_some(), "should have next cursor");

    // Read second batch.
    let r2 = registry
        .read_records(
            "db",
            &ReadRequest {
                entity: "items".to_string(),
                cursor: r1.next_cursor,
                batch_size: 2,
            },
        )
        .await
        .expect("read 2");
    assert_eq!(r2.records.len(), 2);
    assert!(r2.next_cursor.is_some());

    // Read third batch (only 1 remaining).
    let r3 = registry
        .read_records(
            "db",
            &ReadRequest {
                entity: "items".to_string(),
                cursor: r2.next_cursor,
                batch_size: 2,
            },
        )
        .await
        .expect("read 3");
    assert_eq!(r3.records.len(), 1);
    assert!(r3.next_cursor.is_none(), "no more records");
}

#[tokio::test]
async fn schema_drift_blocks_sync_validation() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let csv_path = dir.path().join("data.csv");
    std::fs::write(&csv_path, "id,name\n1,test\n").expect("write");

    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![csv_config("src", csv_path.to_str().expect("path"))]);

    // Discover the schema.
    let _ = registry.discover("src").await.expect("discover");

    // Create a link that references a field that exists.
    let link = DataLink {
        id: "drift-test".to_string(),
        name: "drift".to_string(),
        source: DataEndpoint {
            connector: "src".to_string(),
            entity: "data".to_string(),
        },
        target: DataEndpoint {
            connector: "src".to_string(),
            entity: "data".to_string(),
        },
        field_mappings: vec![FieldMapping {
            source_field: "removed_field".to_string(),
            target_field: "name".to_string(),
            transform: None,
        }],
        sync_mode: SyncMode::OnDemand,
        transform: None,
        last_sync_cursor: None,
        last_sync_at: 0,
    };
    registry.upsert_link(link.clone());

    // Validate should catch the missing field.
    let errors = registry.validate_link_mappings(&link);
    assert!(!errors.is_empty(), "should report missing field");
    assert!(errors[0].contains("removed_field"));
}

#[tokio::test]
async fn persistence_survives_registry_reload() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let data_dir = dir.path().join("agentzero");
    std::fs::create_dir_all(&data_dir).expect("mkdir");

    // Create registry with persistence and add a link.
    {
        let mut reg = ConnectorRegistry::with_persistence(&data_dir).expect("create registry");
        reg.load_configs(vec![csv_config("src", "/tmp/dummy.csv")]);

        let link = DataLink {
            id: "persist-test".to_string(),
            name: "persisted link".to_string(),
            source: DataEndpoint {
                connector: "src".to_string(),
                entity: "orders".to_string(),
            },
            target: DataEndpoint {
                connector: "dst".to_string(),
                entity: "orders".to_string(),
            },
            field_mappings: vec![FieldMapping {
                source_field: "id".to_string(),
                target_field: "order_id".to_string(),
                transform: None,
            }],
            sync_mode: SyncMode::OnDemand,
            transform: None,
            last_sync_cursor: Some("42".to_string()),
            last_sync_at: 1000,
        };
        reg.upsert_link(link);
    }
    // Registry dropped, store written.

    // Re-open registry — link should be loaded from encrypted store.
    let reg2 = ConnectorRegistry::with_persistence(&data_dir).expect("reload registry");
    let loaded = reg2.link("persist-test");
    assert!(loaded.is_some(), "link should survive reload");
    let link = loaded.expect("link");
    assert_eq!(link.name, "persisted link");
    assert_eq!(link.last_sync_cursor.as_deref(), Some("42"));
    assert_eq!(link.field_mappings.len(), 1);
    assert_eq!(link.field_mappings[0].source_field, "id");
}

#[tokio::test]
async fn csv_with_quoted_commas_roundtrip() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let csv_path = dir.path().join("quoted.csv");
    std::fs::write(
        &csv_path,
        "id,name,address\n1,\"Smith, John\",\"123 Main St, Apt 4\"\n2,Jane,\"456 Oak\"\n",
    )
    .expect("write");

    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![csv_config("src", csv_path.to_str().expect("path"))]);

    // Read records — quoted fields should be parsed correctly.
    let result = registry
        .read_records(
            "src",
            &ReadRequest {
                entity: "quoted".to_string(),
                cursor: None,
                batch_size: 100,
            },
        )
        .await
        .expect("read");

    assert_eq!(result.records.len(), 2);
    assert_eq!(result.records[0]["name"], "Smith, John");
    assert_eq!(result.records[0]["address"], "123 Main St, Apt 4");

    // Write back to a new file and verify roundtrip.
    let target_path = dir.path().join("output.csv");
    std::fs::write(&target_path, "id,name,address\n").expect("init target");

    let mut reg2 = ConnectorRegistry::new();
    reg2.load_configs(vec![csv_config("dst", target_path.to_str().expect("path"))]);

    let write_result = reg2
        .write_records(
            "dst",
            &WriteRequest {
                entity: "output".to_string(),
                records: result.records,
                primary_key: "id".to_string(),
            },
        )
        .await
        .expect("write");

    assert_eq!(write_result.written, 2);
    assert!(write_result.errors.is_empty());

    // Read back from the written file.
    let readback = reg2
        .read_records(
            "dst",
            &ReadRequest {
                entity: "output".to_string(),
                cursor: None,
                batch_size: 100,
            },
        )
        .await
        .expect("readback");

    assert_eq!(readback.records.len(), 2);
    assert_eq!(readback.records[0]["name"], "Smith, John");
}

#[tokio::test]
async fn sqlite_write_is_transactional() {
    let dir = tempfile::tempdir().expect("tmpdir");
    let db_path = dir.path().join("tx.db");

    let conn = rusqlite::Connection::open(&db_path).expect("open");
    conn.execute_batch("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);")
        .expect("create table");
    drop(conn);

    let mut registry = ConnectorRegistry::new();
    registry.load_configs(vec![sqlite_config("db", db_path.to_str().expect("path"))]);

    // Write 3 records — all should succeed in one transaction.
    let records = vec![
        serde_json::json!({"id": 1, "name": "a"}),
        serde_json::json!({"id": 2, "name": "b"}),
        serde_json::json!({"id": 3, "name": "c"}),
    ];

    let result = registry
        .write_records(
            "db",
            &WriteRequest {
                entity: "items".to_string(),
                records,
                primary_key: "id".to_string(),
            },
        )
        .await
        .expect("write");

    assert_eq!(result.written, 3);
    assert!(result.errors.is_empty());

    // Upsert: update id=2, insert id=4.
    let upsert = vec![
        serde_json::json!({"id": 2, "name": "b-updated"}),
        serde_json::json!({"id": 4, "name": "d"}),
    ];

    let result2 = registry
        .write_records(
            "db",
            &WriteRequest {
                entity: "items".to_string(),
                records: upsert,
                primary_key: "id".to_string(),
            },
        )
        .await
        .expect("upsert");

    assert_eq!(result2.written, 2);

    // Verify final state.
    let conn = rusqlite::Connection::open(&db_path).expect("open");
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .expect("count");
    assert_eq!(count, 4, "should have 4 rows after upsert");

    let name: String = conn
        .query_row("SELECT name FROM items WHERE id = 2", [], |row| row.get(0))
        .expect("query");
    assert_eq!(name, "b-updated", "id=2 should be updated");
}

#[test]
fn postgres_sql_escaping() {
    // Verify that the escape function handles SQL injection attempts.
    use agentzero_connectors::templates::database::escape_pg_value;

    let normal = escape_pg_value(&serde_json::json!("hello"));
    assert!(normal.starts_with("$val$"));
    assert!(normal.ends_with("$val$"));

    let injection = escape_pg_value(&serde_json::json!("'; DROP TABLE users; --"));
    // Should be dollar-quoted, not single-quoted.
    assert!(injection.contains("$val$"));
    assert!(!injection.contains("''; DROP TABLE"));

    let null_val = escape_pg_value(&serde_json::Value::Null);
    assert_eq!(null_val, "NULL");

    let number = escape_pg_value(&serde_json::json!(42));
    assert_eq!(number, "42");
}

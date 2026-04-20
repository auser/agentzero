//! File connector template.
//!
//! Supports CSV, JSON, and JSONL files with schema inference from
//! headers (CSV) or first record (JSON/JSONL).

use crate::templates::{ConnectorTemplate, ReadRequest, ReadResult, WriteRequest, WriteResult};
use crate::{
    AuthConfig, ConnectorCaps, ConnectorConfig, ConnectorManifest, ConnectorType, EntitySchema,
    FieldDef, FieldType, SyncError,
};
use async_trait::async_trait;
use std::path::Path;

/// Template for file-based connectors (CSV, JSON, JSONL).
pub struct FileTemplate;

#[async_trait]
impl ConnectorTemplate for FileTemplate {
    fn manifest(&self, config: &ConnectorConfig) -> anyhow::Result<ConnectorManifest> {
        let _path = config
            .settings
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("file connector requires `path`"))?;

        Ok(ConnectorManifest {
            name: config.name.clone(),
            connector_type: ConnectorType::File,
            auth: AuthConfig::None,
            entities: vec![],
            capabilities: ConnectorCaps {
                read: true,
                write: true,
                list: false,
                search: false,
                subscribe: false,
                discover_schema: true,
            },
        })
    }

    async fn discover_schema(&self, config: &ConnectorConfig) -> anyhow::Result<Vec<EntitySchema>> {
        let path = config
            .settings
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("file connector requires `path`"))?;

        let file_type = infer_file_type(path);

        match file_type {
            FileType::Csv => discover_csv(path),
            FileType::Json => discover_json(path).await,
            FileType::Jsonl => discover_jsonl(path).await,
        }
    }

    async fn read_records(
        &self,
        config: &ConnectorConfig,
        request: &ReadRequest,
    ) -> anyhow::Result<ReadResult> {
        let path = config
            .settings
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("file connector requires `path`"))?;

        let file_type = infer_file_type(path);
        let all_records = match file_type {
            FileType::Csv => read_csv_records(path)?,
            FileType::Json => read_json_records(path).await?,
            FileType::Jsonl => read_jsonl_records(path).await?,
        };

        // Apply cursor-based pagination: skip records up to cursor position.
        let start_idx = match &request.cursor {
            Some(cursor) => cursor.parse::<usize>().unwrap_or(0),
            None => 0,
        };

        let batch: Vec<serde_json::Value> = all_records
            .into_iter()
            .skip(start_idx)
            .take(request.batch_size as usize)
            .collect();

        let next_cursor = if batch.len() == request.batch_size as usize {
            Some((start_idx + batch.len()).to_string())
        } else {
            None
        };

        Ok(ReadResult {
            records: batch,
            next_cursor,
        })
    }

    async fn write_records(
        &self,
        config: &ConnectorConfig,
        request: &WriteRequest,
    ) -> anyhow::Result<WriteResult> {
        let path = config
            .settings
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("file connector requires `path`"))?;

        let file_type = infer_file_type(path);
        match file_type {
            FileType::Csv => write_csv_records(path, &request.records, &request.primary_key),
            FileType::Json => {
                write_json_records(path, &request.records, &request.primary_key).await
            }
            FileType::Jsonl => {
                write_jsonl_records(path, &request.records, &request.primary_key).await
            }
        }
    }
}

// ── File read implementations ────────────────────────────────────────

fn read_csv_records(path: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let content = std::fs::read_to_string(path)?;
    let mut lines = content.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("CSV file is empty"))?;
    let headers: Vec<String> = parse_csv_line(header_line);

    let mut records = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let values = parse_csv_line(line);
        let mut obj = serde_json::Map::new();
        for (i, header) in headers.iter().enumerate() {
            let val = values.get(i).map(|s| s.as_str()).unwrap_or("");
            // Try to parse as number/bool, fall back to string.
            if let Ok(n) = val.parse::<i64>() {
                obj.insert(header.clone(), serde_json::json!(n));
            } else if let Ok(n) = val.parse::<f64>() {
                obj.insert(header.clone(), serde_json::json!(n));
            } else if val == "true" || val == "false" {
                obj.insert(
                    header.clone(),
                    serde_json::json!(val.parse::<bool>().unwrap_or(false)),
                );
            } else {
                obj.insert(header.clone(), serde_json::json!(val));
            }
        }
        records.push(serde_json::Value::Object(obj));
    }
    Ok(records)
}

/// Parse a CSV line respecting quoted fields (handles commas inside quotes).
fn parse_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    // Escaped quote ("").
                    current.push('"');
                    chars.next();
                } else {
                    // End of quoted field.
                    in_quotes = false;
                }
            } else {
                current.push(ch);
            }
        } else {
            match ch {
                ',' => {
                    fields.push(current.trim().to_string());
                    current = String::new();
                }
                '"' => {
                    in_quotes = true;
                }
                _ => {
                    current.push(ch);
                }
            }
        }
    }
    fields.push(current.trim().to_string());
    fields
}

async fn read_json_records(path: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let content = tokio::fs::read_to_string(path).await?;
    let value: serde_json::Value = serde_json::from_str(&content)?;
    match value {
        serde_json::Value::Array(arr) => Ok(arr),
        obj @ serde_json::Value::Object(_) => Ok(vec![obj]),
        _ => anyhow::bail!("JSON file must contain an array or object"),
    }
}

async fn read_jsonl_records(path: &str) -> anyhow::Result<Vec<serde_json::Value>> {
    let content = tokio::fs::read_to_string(path).await?;
    let mut records = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        records.push(serde_json::from_str(line)?);
    }
    Ok(records)
}

// ── File write implementations (upsert semantics) ────────────────────

fn write_csv_records(
    path: &str,
    records: &[serde_json::Value],
    primary_key: &str,
) -> anyhow::Result<WriteResult> {
    // Read existing records, merge with new ones (upsert by primary key).
    let mut existing: Vec<serde_json::Value> = read_csv_records(path).unwrap_or_default();

    let mut written = 0u64;
    let mut errors = Vec::new();

    for record in records {
        let Some(pk_val) = record.get(primary_key) else {
            errors.push(SyncError {
                record_key: "unknown".to_string(),
                message: format!("missing primary key `{primary_key}`"),
            });
            continue;
        };

        // Find and replace existing record with same PK, or append.
        if let Some(pos) = existing
            .iter()
            .position(|r| r.get(primary_key) == Some(pk_val))
        {
            existing[pos] = record.clone();
        } else {
            existing.push(record.clone());
        }
        written += 1;
    }

    // Collect all headers from all records.
    let mut headers: Vec<String> = Vec::new();
    for rec in &existing {
        if let Some(obj) = rec.as_object() {
            for key in obj.keys() {
                if !headers.contains(key) {
                    headers.push(key.clone());
                }
            }
        }
    }

    // Write CSV.
    let mut output = headers.join(",") + "\n";
    for rec in &existing {
        let row: Vec<String> = headers
            .iter()
            .map(|h| {
                rec.get(h)
                    .map(|v| match v {
                        serde_json::Value::String(s) => {
                            if s.contains(',') || s.contains('"') {
                                format!("\"{}\"", s.replace('"', "\"\""))
                            } else {
                                s.clone()
                            }
                        }
                        serde_json::Value::Null => String::new(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default()
            })
            .collect();
        output.push_str(&row.join(","));
        output.push('\n');
    }
    std::fs::write(path, output)?;

    Ok(WriteResult {
        written,
        skipped: 0,
        errors,
    })
}

async fn write_json_records(
    path: &str,
    records: &[serde_json::Value],
    primary_key: &str,
) -> anyhow::Result<WriteResult> {
    let mut existing: Vec<serde_json::Value> = read_json_records(path).await.unwrap_or_default();

    let mut written = 0u64;
    let mut errors = Vec::new();

    for record in records {
        let Some(pk_val) = record.get(primary_key) else {
            errors.push(SyncError {
                record_key: "unknown".to_string(),
                message: format!("missing primary key `{primary_key}`"),
            });
            continue;
        };

        if let Some(pos) = existing
            .iter()
            .position(|r| r.get(primary_key) == Some(pk_val))
        {
            existing[pos] = record.clone();
        } else {
            existing.push(record.clone());
        }
        written += 1;
    }

    let json = serde_json::to_string_pretty(&existing)?;
    tokio::fs::write(path, json).await?;

    Ok(WriteResult {
        written,
        skipped: 0,
        errors,
    })
}

async fn write_jsonl_records(
    path: &str,
    records: &[serde_json::Value],
    primary_key: &str,
) -> anyhow::Result<WriteResult> {
    let mut existing: Vec<serde_json::Value> = read_jsonl_records(path).await.unwrap_or_default();

    let mut written = 0u64;
    let mut errors = Vec::new();

    for record in records {
        let Some(pk_val) = record.get(primary_key) else {
            errors.push(SyncError {
                record_key: "unknown".to_string(),
                message: format!("missing primary key `{primary_key}`"),
            });
            continue;
        };

        if let Some(pos) = existing
            .iter()
            .position(|r| r.get(primary_key) == Some(pk_val))
        {
            existing[pos] = record.clone();
        } else {
            existing.push(record.clone());
        }
        written += 1;
    }

    let mut output = String::new();
    for rec in &existing {
        output.push_str(&serde_json::to_string(rec)?);
        output.push('\n');
    }
    tokio::fs::write(path, output).await?;

    Ok(WriteResult {
        written,
        skipped: 0,
        errors,
    })
}

#[derive(Debug, Clone, Copy)]
enum FileType {
    Csv,
    Json,
    Jsonl,
}

fn infer_file_type(path: &str) -> FileType {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "csv" | "tsv" => FileType::Csv,
        "jsonl" | "ndjson" => FileType::Jsonl,
        _ => FileType::Json,
    }
}

/// Discover schema from a CSV file by reading headers and first data row.
fn discover_csv(path: &str) -> anyhow::Result<Vec<EntitySchema>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read CSV file `{path}`: {e}"))?;

    let mut lines = content.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("CSV file is empty"))?;

    let headers = parse_csv_line(header_line);

    // Try to infer types from first data row.
    let first_row: Vec<String> = lines.next().map(parse_csv_line).unwrap_or_default();

    let fields: Vec<FieldDef> = headers
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let field_type = first_row
                .get(i)
                .map(|v| infer_json_type_from_str(v))
                .unwrap_or(FieldType::String);
            FieldDef {
                name: name.to_string(),
                field_type,
                required: false,
                description: String::new(),
            }
        })
        .collect();

    // Use first column as primary key by default.
    let id_default = "id".to_string();
    let primary_key = headers.first().unwrap_or(&id_default).clone();

    let entity_name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("records")
        .to_string();

    Ok(vec![EntitySchema {
        name: entity_name,
        fields,
        primary_key,
        json_schema: serde_json::json!({}),
    }])
}

/// Discover schema from a JSON file (expects an array of objects).
async fn discover_json(path: &str) -> anyhow::Result<Vec<EntitySchema>> {
    let content = tokio::fs::read_to_string(path).await?;
    let value: serde_json::Value = serde_json::from_str(&content)?;

    let first_record = match &value {
        serde_json::Value::Array(arr) => arr.first(),
        serde_json::Value::Object(_) => Some(&value),
        _ => None,
    };

    let Some(record) = first_record else {
        return Ok(vec![]);
    };

    let entity = schema_from_json_object(record, path);
    Ok(vec![entity])
}

/// Discover schema from a JSONL file (one JSON object per line).
async fn discover_jsonl(path: &str) -> anyhow::Result<Vec<EntitySchema>> {
    let content = tokio::fs::read_to_string(path).await?;
    let first_line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("JSONL file is empty"))?;

    let value: serde_json::Value = serde_json::from_str(first_line)?;
    let entity = schema_from_json_object(&value, path);
    Ok(vec![entity])
}

/// Build an EntitySchema from a JSON object by inspecting its keys and value types.
fn schema_from_json_object(obj: &serde_json::Value, path: &str) -> EntitySchema {
    let fields: Vec<FieldDef> = obj
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(key, val)| FieldDef {
                    name: key.clone(),
                    field_type: infer_json_value_type(val),
                    required: false,
                    description: String::new(),
                })
                .collect()
        })
        .unwrap_or_default();

    let primary_key = fields
        .iter()
        .find(|f| f.name == "id" || f.name == "_id")
        .map(|f| f.name.clone())
        .or_else(|| fields.first().map(|f| f.name.clone()))
        .unwrap_or_else(|| "id".to_string());

    let entity_name = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("records")
        .to_string();

    EntitySchema {
        name: entity_name,
        fields,
        primary_key,
        json_schema: serde_json::json!({}),
    }
}

/// Infer FieldType from a JSON value.
fn infer_json_value_type(val: &serde_json::Value) -> FieldType {
    match val {
        serde_json::Value::Bool(_) => FieldType::Boolean,
        serde_json::Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                FieldType::Integer
            } else {
                FieldType::Number
            }
        }
        serde_json::Value::String(s) => {
            // Try to detect datetime strings.
            if looks_like_datetime(s) {
                FieldType::DateTime
            } else {
                FieldType::String
            }
        }
        serde_json::Value::Object(_) => FieldType::Json,
        serde_json::Value::Array(_) => FieldType::Json,
        serde_json::Value::Null => FieldType::String,
    }
}

/// Infer FieldType from a string value (CSV cells).
fn infer_json_type_from_str(s: &str) -> FieldType {
    if s.is_empty() {
        return FieldType::String;
    }
    if s == "true" || s == "false" {
        return FieldType::Boolean;
    }
    if s.parse::<i64>().is_ok() {
        return FieldType::Integer;
    }
    if s.parse::<f64>().is_ok() {
        return FieldType::Number;
    }
    if looks_like_datetime(s) {
        return FieldType::DateTime;
    }
    FieldType::String
}

/// Simple heuristic for datetime-like strings.
fn looks_like_datetime(s: &str) -> bool {
    // ISO 8601 pattern: YYYY-MM-DD or YYYY-MM-DDTHH:MM:SS
    let bytes = s.as_bytes();
    bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[0..4].iter().all(|b| b.is_ascii_digit())
        && bytes[5..7].iter().all(|b| b.is_ascii_digit())
        && bytes[8..10].iter().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn file_config(path: &str) -> ConnectorConfig {
        let mut settings = HashMap::new();
        settings.insert(
            "path".to_string(),
            serde_json::Value::String(path.to_string()),
        );
        ConnectorConfig {
            name: "test_file".to_string(),
            connector_type: ConnectorType::File,
            settings,
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: crate::RateLimitConfig::default(),
            pagination: crate::PaginationStrategy::None,
            batch_size: 100,
        }
    }

    #[test]
    fn infer_file_type_from_extension() {
        assert!(matches!(infer_file_type("data.csv"), FileType::Csv));
        assert!(matches!(infer_file_type("data.tsv"), FileType::Csv));
        assert!(matches!(infer_file_type("data.json"), FileType::Json));
        assert!(matches!(infer_file_type("data.jsonl"), FileType::Jsonl));
        assert!(matches!(infer_file_type("data.ndjson"), FileType::Jsonl));
        assert!(matches!(infer_file_type("data.txt"), FileType::Json));
    }

    #[test]
    fn discover_csv_basic() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let csv_path = dir.path().join("products.csv");
        std::fs::write(
            &csv_path,
            "id,name,price,active\n1,Widget,9.99,true\n2,Gadget,19.99,false\n",
        )
        .expect("write");

        let _config = file_config(csv_path.to_str().expect("path"));
        let entities = discover_csv(csv_path.to_str().expect("path")).expect("discover");

        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.name, "products");
        assert_eq!(entity.primary_key, "id");
        assert_eq!(entity.fields.len(), 4);

        let id_field = entity.fields.iter().find(|f| f.name == "id").expect("id");
        assert_eq!(id_field.field_type, FieldType::Integer);

        let price = entity
            .fields
            .iter()
            .find(|f| f.name == "price")
            .expect("price");
        assert_eq!(price.field_type, FieldType::Number);

        let active = entity
            .fields
            .iter()
            .find(|f| f.name == "active")
            .expect("active");
        assert_eq!(active.field_type, FieldType::Boolean);
    }

    #[tokio::test]
    async fn discover_json_array() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let json_path = dir.path().join("orders.json");
        std::fs::write(
            &json_path,
            r#"[{"id": 1, "total": 42.5, "created_at": "2024-01-15T10:30:00Z"}]"#,
        )
        .expect("write");

        let entities = discover_json(json_path.to_str().expect("path"))
            .await
            .expect("discover");

        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.name, "orders");
        assert_eq!(entity.fields.len(), 3);

        let total = entity
            .fields
            .iter()
            .find(|f| f.name == "total")
            .expect("total");
        assert_eq!(total.field_type, FieldType::Number);

        let created = entity
            .fields
            .iter()
            .find(|f| f.name == "created_at")
            .expect("created_at");
        assert_eq!(created.field_type, FieldType::DateTime);
    }

    #[tokio::test]
    async fn discover_jsonl_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let jsonl_path = dir.path().join("events.jsonl");
        std::fs::write(
            &jsonl_path,
            "{\"id\": \"evt_1\", \"type\": \"click\", \"count\": 5}\n{\"id\": \"evt_2\", \"type\": \"view\", \"count\": 10}\n",
        )
        .expect("write");

        let entities = discover_jsonl(jsonl_path.to_str().expect("path"))
            .await
            .expect("discover");

        assert_eq!(entities.len(), 1);
        let entity = &entities[0];
        assert_eq!(entity.name, "events");
        assert_eq!(entity.fields.len(), 3);

        let count = entity
            .fields
            .iter()
            .find(|f| f.name == "count")
            .expect("count");
        assert_eq!(count.field_type, FieldType::Integer);
    }

    #[test]
    fn type_inference_from_strings() {
        assert_eq!(infer_json_type_from_str("42"), FieldType::Integer);
        assert_eq!(infer_json_type_from_str("3.14"), FieldType::Number);
        assert_eq!(infer_json_type_from_str("true"), FieldType::Boolean);
        assert_eq!(infer_json_type_from_str("false"), FieldType::Boolean);
        assert_eq!(infer_json_type_from_str("2024-01-15"), FieldType::DateTime);
        assert_eq!(infer_json_type_from_str("hello"), FieldType::String);
        assert_eq!(infer_json_type_from_str(""), FieldType::String);
    }

    #[test]
    fn looks_like_datetime_patterns() {
        assert!(looks_like_datetime("2024-01-15"));
        assert!(looks_like_datetime("2024-01-15T10:30:00Z"));
        assert!(looks_like_datetime("2024-12-31T23:59:59+05:00"));
        assert!(!looks_like_datetime("hello"));
        assert!(!looks_like_datetime("12345"));
        assert!(!looks_like_datetime("2024"));
    }

    #[test]
    fn manifest_requires_path() {
        let config = ConnectorConfig {
            name: "test".to_string(),
            connector_type: ConnectorType::File,
            settings: HashMap::new(),
            auth: AuthConfig::None,
            privacy_boundary: String::new(),
            rate_limit: crate::RateLimitConfig::default(),
            pagination: crate::PaginationStrategy::None,
            batch_size: 100,
        };
        let template = FileTemplate;
        assert!(template.manifest(&config).is_err());
    }

    #[test]
    fn json_value_type_inference() {
        use serde_json::json;
        assert_eq!(infer_json_value_type(&json!(true)), FieldType::Boolean);
        assert_eq!(infer_json_value_type(&json!(42)), FieldType::Integer);
        assert_eq!(infer_json_value_type(&json!(3.15)), FieldType::Number);
        assert_eq!(infer_json_value_type(&json!("hello")), FieldType::String);
        assert_eq!(
            infer_json_value_type(&json!("2024-01-15T10:00:00Z")),
            FieldType::DateTime
        );
        assert_eq!(infer_json_value_type(&json!({"a": 1})), FieldType::Json);
        assert_eq!(infer_json_value_type(&json!([1, 2])), FieldType::Json);
        assert_eq!(infer_json_value_type(&json!(null)), FieldType::String);
    }

    #[test]
    fn parse_csv_line_handles_quoted_fields() {
        let line = r#"1,"Smith, John","New York, NY",true"#;
        let fields = parse_csv_line(line);
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], "1");
        assert_eq!(fields[1], "Smith, John");
        assert_eq!(fields[2], "New York, NY");
        assert_eq!(fields[3], "true");
    }

    #[test]
    fn parse_csv_line_handles_escaped_quotes() {
        let line = r#""hello ""world""",simple"#;
        let fields = parse_csv_line(line);
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0], r#"hello "world""#);
        assert_eq!(fields[1], "simple");
    }

    #[test]
    fn parse_csv_line_empty_fields() {
        let fields = parse_csv_line("a,,b,");
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0], "a");
        assert_eq!(fields[1], "");
        assert_eq!(fields[2], "b");
        assert_eq!(fields[3], "");
    }

    #[test]
    fn csv_roundtrip_with_commas_in_values() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let csv_path = dir.path().join("addresses.csv");
        std::fs::write(
            &csv_path,
            "id,name,address\n1,\"Smith, John\",\"123 Main St, Apt 4\"\n2,Jane,\"456 Oak Ave\"\n",
        )
        .expect("write");

        let records = read_csv_records(csv_path.to_str().expect("path")).expect("read");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["name"], "Smith, John");
        assert_eq!(records[0]["address"], "123 Main St, Apt 4");
        assert_eq!(records[1]["name"], "Jane");
    }
}

//! Sync engine: executes data links by reading from source, applying field
//! mappings, and writing to target.

use crate::{DataLink, FieldMapping, SyncError, SyncResult};
use serde_json::Value;

/// Apply field mappings to transform a source record into a target record.
///
/// Each `FieldMapping` copies `source_field` → `target_field`. Fields in the
/// source that are not mapped are dropped. Fields in the mapping that are
/// missing from the source produce a `null` in the target.
pub fn apply_mappings(source: &Value, mappings: &[FieldMapping]) -> Value {
    let Some(source_obj) = source.as_object() else {
        return Value::Null;
    };

    let mut target = serde_json::Map::new();
    for mapping in mappings {
        let value = source_obj
            .get(&mapping.source_field)
            .cloned()
            .unwrap_or(Value::Null);
        target.insert(mapping.target_field.clone(), value);
    }

    Value::Object(target)
}

/// Execute a sync: reads source records, applies mappings, produces target records.
///
/// This is the pure transformation layer — it does not perform I/O. Callers
/// (the `data_sync` tool) handle reading from and writing to connectors.
pub fn transform_batch(link: &DataLink, source_records: &[Value]) -> (Vec<Value>, Vec<SyncError>) {
    let mut transformed = Vec::with_capacity(source_records.len());
    let mut errors = Vec::new();

    for (idx, record) in source_records.iter().enumerate() {
        let record_key = record
            .as_object()
            .and_then(|o| o.get(&link.source.entity))
            .or_else(|| record.get("id"))
            .map(|v| v.to_string())
            .unwrap_or_else(|| format!("record_{idx}"));

        let mapped = apply_mappings(record, &link.field_mappings);

        if mapped.is_null() {
            errors.push(SyncError {
                record_key,
                message: "source record is not a JSON object".to_string(),
            });
            continue;
        }

        transformed.push(mapped);
    }

    (transformed, errors)
}

/// Build a `SyncResult` summary from batch results.
pub fn build_result(
    link_id: &str,
    records_read: u64,
    records_written: u64,
    records_skipped: u64,
    errors: Vec<SyncError>,
    cursor: Option<String>,
) -> SyncResult {
    SyncResult {
        link_id: link_id.to_string(),
        records_read,
        records_written,
        records_skipped,
        records_failed: errors.len() as u64,
        errors,
        cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldMapping;
    use serde_json::json;

    #[test]
    fn apply_mappings_basic() {
        let source = json!({
            "id": 42,
            "total_price": "99.99",
            "discount_code": "SAVE10"
        });
        let mappings = vec![
            FieldMapping {
                source_field: "id".to_string(),
                target_field: "order_id".to_string(),
                transform: None,
            },
            FieldMapping {
                source_field: "total_price".to_string(),
                target_field: "amount".to_string(),
                transform: None,
            },
        ];

        let result = apply_mappings(&source, &mappings);
        assert_eq!(result["order_id"], 42);
        assert_eq!(result["amount"], "99.99");
        // Unmapped field should NOT appear in target.
        assert!(result.get("discount_code").is_none());
    }

    #[test]
    fn apply_mappings_missing_source_field() {
        let source = json!({"id": 1});
        let mappings = vec![FieldMapping {
            source_field: "nonexistent".to_string(),
            target_field: "target_field".to_string(),
            transform: None,
        }];

        let result = apply_mappings(&source, &mappings);
        assert_eq!(result["target_field"], Value::Null);
    }

    #[test]
    fn apply_mappings_non_object_source() {
        let source = json!("just a string");
        let mappings = vec![FieldMapping {
            source_field: "x".to_string(),
            target_field: "y".to_string(),
            transform: None,
        }];

        let result = apply_mappings(&source, &mappings);
        assert_eq!(result, Value::Null);
    }

    #[test]
    fn transform_batch_processes_records() {
        let link = crate::DataLink {
            id: "l1".to_string(),
            name: "test".to_string(),
            source: crate::DataEndpoint {
                connector: "src".to_string(),
                entity: "orders".to_string(),
            },
            target: crate::DataEndpoint {
                connector: "dst".to_string(),
                entity: "orders".to_string(),
            },
            field_mappings: vec![FieldMapping {
                source_field: "id".to_string(),
                target_field: "order_id".to_string(),
                transform: None,
            }],
            sync_mode: crate::SyncMode::OnDemand,
            transform: None,
            last_sync_cursor: None,
            last_sync_at: 0,
        };

        let records = vec![json!({"id": 1}), json!({"id": 2}), json!("invalid")];

        let (transformed, errors) = transform_batch(&link, &records);
        assert_eq!(transformed.len(), 2);
        assert_eq!(errors.len(), 1);
        assert_eq!(transformed[0]["order_id"], 1);
        assert_eq!(transformed[1]["order_id"], 2);
    }

    #[test]
    fn build_result_summary() {
        let result = build_result(
            "l1",
            100,
            97,
            0,
            vec![SyncError {
                record_key: "42".to_string(),
                message: "dup".to_string(),
            }],
            Some("100".to_string()),
        );
        assert_eq!(result.records_read, 100);
        assert_eq!(result.records_written, 97);
        assert_eq!(result.records_failed, 1);
        assert_eq!(result.cursor.as_deref(), Some("100"));
    }
}

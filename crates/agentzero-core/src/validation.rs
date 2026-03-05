//! Lightweight JSON Schema validator for tool input schemas.
//!
//! Supports the subset of JSON Schema used by tool definitions:
//! - `type`: "object", "string", "number", "integer", "boolean", "array", "null"
//! - `required`: array of required field names (for objects)
//! - `properties`: map of field name → sub-schema (for objects)
//! - `items`: schema for array items
//! - `enum`: list of allowed values

use serde_json::Value;

/// Validate a JSON value against a JSON Schema.
///
/// Returns `Ok(())` if the value conforms to the schema, or `Err(errors)` with
/// a list of human-readable validation error messages.
pub fn validate_json(value: &Value, schema: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    validate_inner(value, schema, "", &mut errors);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_inner(value: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
    // Empty schema or boolean true accepts anything.
    if schema.is_boolean() || (schema.is_object() && schema.as_object().unwrap().is_empty()) {
        return;
    }

    let schema_obj = match schema.as_object() {
        Some(obj) => obj,
        None => return,
    };

    // Check `enum` constraint.
    if let Some(enum_values) = schema_obj.get("enum").and_then(|v| v.as_array()) {
        if !enum_values.contains(value) {
            let allowed: Vec<String> = enum_values.iter().map(|v| v.to_string()).collect();
            errors.push(format!(
                "{}: value {} is not one of [{}]",
                display_path(path),
                value,
                allowed.join(", ")
            ));
        }
    }

    // Check `type` constraint.
    if let Some(expected_type) = schema_obj.get("type").and_then(|v| v.as_str()) {
        if !type_matches(value, expected_type) {
            errors.push(format!(
                "{}: expected type \"{}\", got {}",
                display_path(path),
                expected_type,
                json_type_name(value)
            ));
            return; // No point checking sub-constraints if type is wrong.
        }
    }

    // Object-specific checks.
    if value.is_object() {
        let obj = value.as_object().unwrap();

        // Check `required` fields.
        if let Some(required) = schema_obj.get("required").and_then(|v| v.as_array()) {
            for req in required {
                if let Some(field_name) = req.as_str() {
                    if !obj.contains_key(field_name) {
                        errors.push(format!(
                            "{}: missing required field \"{}\"",
                            display_path(path),
                            field_name
                        ));
                    }
                }
            }
        }

        // Recurse into `properties`.
        if let Some(properties) = schema_obj.get("properties").and_then(|v| v.as_object()) {
            for (prop_name, prop_schema) in properties {
                if let Some(prop_value) = obj.get(prop_name) {
                    let child_path = if path.is_empty() {
                        prop_name.clone()
                    } else {
                        format!("{}.{}", path, prop_name)
                    };
                    validate_inner(prop_value, prop_schema, &child_path, errors);
                }
            }
        }
    }

    // Array-specific checks.
    if let Some(arr) = value.as_array() {
        if let Some(items_schema) = schema_obj.get("items") {
            for (i, item) in arr.iter().enumerate() {
                let child_path = format!("{}[{}]", path, i);
                validate_inner(item, items_schema, &child_path, errors);
            }
        }
    }
}

fn type_matches(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "null" => value.is_null(),
        _ => true, // Unknown type — don't reject.
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn display_path(path: &str) -> &str {
    if path.is_empty() {
        "$"
    } else {
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_schema_accepts_anything() {
        assert!(validate_json(&json!(42), &json!({})).is_ok());
        assert!(validate_json(&json!("hello"), &json!({})).is_ok());
        assert!(validate_json(&json!(null), &json!({})).is_ok());
    }

    #[test]
    fn type_string_valid() {
        let schema = json!({"type": "string"});
        assert!(validate_json(&json!("hello"), &schema).is_ok());
    }

    #[test]
    fn type_string_invalid() {
        let schema = json!({"type": "string"});
        let result = validate_json(&json!(42), &schema);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("expected type \"string\""));
    }

    #[test]
    fn type_object_valid() {
        let schema = json!({"type": "object"});
        assert!(validate_json(&json!({"a": 1}), &schema).is_ok());
    }

    #[test]
    fn type_integer_valid() {
        let schema = json!({"type": "integer"});
        assert!(validate_json(&json!(42), &schema).is_ok());
    }

    #[test]
    fn type_integer_rejects_float() {
        let schema = json!({"type": "integer"});
        let result = validate_json(&json!(2.5), &schema);
        assert!(result.is_err());
    }

    #[test]
    fn type_number_accepts_float_and_int() {
        let schema = json!({"type": "number"});
        assert!(validate_json(&json!(42), &schema).is_ok());
        assert!(validate_json(&json!(2.5), &schema).is_ok());
    }

    #[test]
    fn type_boolean_valid() {
        let schema = json!({"type": "boolean"});
        assert!(validate_json(&json!(true), &schema).is_ok());
    }

    #[test]
    fn type_array_valid() {
        let schema = json!({"type": "array"});
        assert!(validate_json(&json!([1, 2, 3]), &schema).is_ok());
    }

    #[test]
    fn type_null_valid() {
        let schema = json!({"type": "null"});
        assert!(validate_json(&json!(null), &schema).is_ok());
    }

    #[test]
    fn required_fields_present() {
        let schema = json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        assert!(validate_json(&json!({"name": "Alice", "age": 30}), &schema).is_ok());
    }

    #[test]
    fn required_field_missing() {
        let schema = json!({
            "type": "object",
            "required": ["name", "age"],
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer"}
            }
        });
        let result = validate_json(&json!({"name": "Alice"}), &schema);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("missing required field \"age\""));
    }

    #[test]
    fn nested_object_validation() {
        let schema = json!({
            "type": "object",
            "properties": {
                "address": {
                    "type": "object",
                    "required": ["city"],
                    "properties": {
                        "city": {"type": "string"},
                        "zip": {"type": "string"}
                    }
                }
            }
        });
        // Valid
        assert!(validate_json(&json!({"address": {"city": "NYC"}}), &schema).is_ok());
        // Invalid: city is wrong type
        let result = validate_json(&json!({"address": {"city": 123}}), &schema);
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].contains("address.city"));
    }

    #[test]
    fn array_items_validation() {
        let schema = json!({
            "type": "array",
            "items": {"type": "string"}
        });
        assert!(validate_json(&json!(["a", "b", "c"]), &schema).is_ok());

        let result = validate_json(&json!(["a", 42, "c"]), &schema);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("[1]"));
    }

    #[test]
    fn enum_validation_valid() {
        let schema = json!({
            "type": "string",
            "enum": ["red", "green", "blue"]
        });
        assert!(validate_json(&json!("red"), &schema).is_ok());
    }

    #[test]
    fn enum_validation_invalid() {
        let schema = json!({
            "type": "string",
            "enum": ["red", "green", "blue"]
        });
        let result = validate_json(&json!("yellow"), &schema);
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].contains("not one of"));
    }

    #[test]
    fn multiple_errors_reported() {
        let schema = json!({
            "type": "object",
            "required": ["a", "b"],
            "properties": {
                "a": {"type": "string"},
                "b": {"type": "integer"}
            }
        });
        // Missing both required fields
        let result = validate_json(&json!({}), &schema);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().len(), 2);
    }

    #[test]
    fn extra_properties_allowed() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        });
        // Extra field "extra" should be accepted (no additionalProperties restriction)
        assert!(validate_json(&json!({"name": "Alice", "extra": true}), &schema).is_ok());
    }

    #[test]
    fn wrong_type_at_root_stops_early() {
        let schema = json!({
            "type": "object",
            "required": ["name"],
            "properties": {"name": {"type": "string"}}
        });
        let result = validate_json(&json!("not an object"), &schema);
        assert!(result.is_err());
        // Should only report the type mismatch, not the missing field
        assert_eq!(result.unwrap_err().len(), 1);
    }
}

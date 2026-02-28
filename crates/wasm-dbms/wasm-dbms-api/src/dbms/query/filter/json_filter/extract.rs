//! JSON value extraction utilities.
//!
//! This module provides functionality for extracting values from JSON structures
//! at specified paths and converting them to DBMS value types.

use serde_json::Value as JsonValue;

use super::path::PathSegment;
use crate::dbms::types::Json;
use crate::dbms::value::Value;

/// Extracts a value from a JSON structure at the specified path.
///
/// Traverses the JSON structure following the path segments and returns
/// the value at that location, if it exists.
///
/// # Arguments
///
/// * `json` - The JSON structure to extract from.
/// * `segments` - The path segments to follow.
///
/// # Returns
///
/// `Some(Value)` if the path exists and contains a value, `None` otherwise.
///
/// # Type Conversion
///
/// When extracting values, JSON types are converted to DBMS types:
///
/// | JSON Type | DBMS Value |
/// |-----------|------------|
/// | `null` | `Value::Null` |
/// | `true`/`false` | `Value::Boolean` |
/// | Integer number | `Value::Int64` |
/// | Float number | `Value::Decimal` |
/// | String | `Value::Text` |
/// | Array | `Value::Json` |
/// | Object | `Value::Json` |
pub fn extract_at_path(json: &Json, segments: &[PathSegment]) -> Option<Value> {
    let mut current = json.value();

    for segment in segments {
        match segment {
            PathSegment::Key(key) => {
                current = current.as_object()?.get(key)?;
            }
            PathSegment::Index(idx) => {
                current = current.as_array()?.get(*idx)?;
            }
        }
    }

    Some(json_value_to_dbms_value(current))
}

/// Converts a `serde_json::Value` to a DBMS `Value`.
///
/// # Type Mapping
///
/// | JSON Type | DBMS Value |
/// |-----------|------------|
/// | `null` | `Value::Null` |
/// | `true`/`false` | `Value::Boolean` |
/// | Integer number (fits i64) | `Value::Int64` |
/// | Float number | `Value::Decimal` |
/// | String | `Value::Text` |
/// | Array | `Value::Json` |
/// | Object | `Value::Json` |
pub fn json_value_to_dbms_value(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(b) => Value::Boolean((*b).into()),
        JsonValue::Number(n) => {
            // Try to convert to i64 first (most common case for integers)
            if let Some(i) = n.as_i64() {
                return Value::Int64(i.into());
            }
            // Try u64 next
            if let Some(u) = n.as_u64() {
                // If it fits in i64, use Int64
                if u <= i64::MAX as u64 {
                    return Value::Int64((u as i64).into());
                }
                // Otherwise, convert to Decimal
                return Value::Decimal(rust_decimal::Decimal::from(u).into());
            }
            // Fall back to f64 -> Decimal
            if let Some(f) = n.as_f64() {
                // Try to convert f64 to Decimal
                if let Ok(d) = rust_decimal::Decimal::try_from(f) {
                    return Value::Decimal(d.into());
                }
            }
            // Fallback: shouldn't happen for valid JSON numbers
            Value::Null
        }
        JsonValue::String(s) => Value::Text(s.clone().into()),
        // Arrays and objects are wrapped as JSON
        JsonValue::Array(_) | JsonValue::Object(_) => Value::Json(Json::from(value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // Helper to create Json from serde_json::Value
    fn j(v: JsonValue) -> Json {
        Json::from(v)
    }

    // ===== extract_at_path Tests =====

    #[test]
    fn test_extract_root_key() {
        let json = j(json!({"name": "Alice"}));
        let segments = vec![PathSegment::Key("name".to_string())];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Text("Alice".into())));
    }

    #[test]
    fn test_extract_nested_key() {
        let json = j(json!({"user": {"name": "Bob"}}));
        let segments = vec![
            PathSegment::Key("user".to_string()),
            PathSegment::Key("name".to_string()),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Text("Bob".into())));
    }

    #[test]
    fn test_extract_deeply_nested() {
        let json = j(json!({"a": {"b": {"c": {"d": 42}}}}));
        let segments = vec![
            PathSegment::Key("a".to_string()),
            PathSegment::Key("b".to_string()),
            PathSegment::Key("c".to_string()),
            PathSegment::Key("d".to_string()),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Int64(42.into())));
    }

    #[test]
    fn test_extract_array_index() {
        let json = j(json!({"items": [10, 20, 30]}));
        let segments = vec![PathSegment::Key("items".to_string()), PathSegment::Index(1)];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Int64(20.into())));
    }

    #[test]
    fn test_extract_array_of_objects() {
        let json = j(json!({"users": [{"name": "Alice"}, {"name": "Bob"}]}));
        let segments = vec![
            PathSegment::Key("users".to_string()),
            PathSegment::Index(1),
            PathSegment::Key("name".to_string()),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Text("Bob".into())));
    }

    #[test]
    fn test_extract_nested_array() {
        let json = j(json!({"matrix": [[1, 2], [3, 4]]}));
        let segments = vec![
            PathSegment::Key("matrix".to_string()),
            PathSegment::Index(1),
            PathSegment::Index(0),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Int64(3.into())));
    }

    #[test]
    fn test_extract_root_array() {
        let json = j(json!([100, 200, 300]));
        let segments = vec![PathSegment::Index(2)];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Int64(300.into())));
    }

    #[test]
    fn test_extract_missing_key() {
        let json = j(json!({"name": "Alice"}));
        let segments = vec![PathSegment::Key("email".to_string())];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_missing_nested_key() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let segments = vec![
            PathSegment::Key("user".to_string()),
            PathSegment::Key("email".to_string()),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_out_of_bounds_index() {
        let json = j(json!({"items": [1, 2, 3]}));
        let segments = vec![
            PathSegment::Key("items".to_string()),
            PathSegment::Index(10),
        ];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_key_on_array() {
        let json = j(json!([1, 2, 3]));
        let segments = vec![PathSegment::Key("name".to_string())];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_index_on_object() {
        let json = j(json!({"name": "Alice"}));
        let segments = vec![PathSegment::Index(0)];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_empty_path() {
        let json = j(json!({"name": "Alice"}));
        let segments: Vec<PathSegment> = vec![];

        let result = extract_at_path(&json, &segments);
        // Empty path extracts the entire JSON
        assert!(result.is_some());
        assert!(matches!(result, Some(Value::Json(_))));
    }

    // ===== json_value_to_dbms_value Tests =====

    #[test]
    fn test_convert_null() {
        let value = json_value_to_dbms_value(&JsonValue::Null);
        assert_eq!(value, Value::Null);
    }

    #[test]
    fn test_convert_bool_true() {
        let value = json_value_to_dbms_value(&json!(true));
        assert_eq!(value, Value::Boolean(true.into()));
    }

    #[test]
    fn test_convert_bool_false() {
        let value = json_value_to_dbms_value(&json!(false));
        assert_eq!(value, Value::Boolean(false.into()));
    }

    #[test]
    fn test_convert_positive_integer() {
        let value = json_value_to_dbms_value(&json!(42));
        assert_eq!(value, Value::Int64(42.into()));
    }

    #[test]
    fn test_convert_negative_integer() {
        let value = json_value_to_dbms_value(&json!(-100));
        assert_eq!(value, Value::Int64((-100).into()));
    }

    #[test]
    fn test_convert_zero() {
        let value = json_value_to_dbms_value(&json!(0));
        assert_eq!(value, Value::Int64(0.into()));
    }

    #[test]
    fn test_convert_large_integer() {
        let value = json_value_to_dbms_value(&json!(i64::MAX));
        assert_eq!(value, Value::Int64(i64::MAX.into()));
    }

    #[test]
    fn test_convert_float() {
        let value = json_value_to_dbms_value(&json!(3.14));
        assert!(matches!(value, Value::Decimal(_)));
    }

    #[test]
    fn test_convert_negative_float() {
        let value = json_value_to_dbms_value(&json!(-2.718));
        assert!(matches!(value, Value::Decimal(_)));
    }

    #[test]
    fn test_convert_string() {
        let value = json_value_to_dbms_value(&json!("hello"));
        assert_eq!(value, Value::Text("hello".into()));
    }

    #[test]
    fn test_convert_empty_string() {
        let value = json_value_to_dbms_value(&json!(""));
        assert_eq!(value, Value::Text("".into()));
    }

    #[test]
    fn test_convert_array() {
        let json_val = json!([1, 2, 3]);
        let value = json_value_to_dbms_value(&json_val);
        assert!(matches!(value, Value::Json(_)));
    }

    #[test]
    fn test_convert_empty_array() {
        let json_val = json!([]);
        let value = json_value_to_dbms_value(&json_val);
        assert!(matches!(value, Value::Json(_)));
    }

    #[test]
    fn test_convert_object() {
        let json_val = json!({"key": "value"});
        let value = json_value_to_dbms_value(&json_val);
        assert!(matches!(value, Value::Json(_)));
    }

    #[test]
    fn test_convert_empty_object() {
        let json_val = json!({});
        let value = json_value_to_dbms_value(&json_val);
        assert!(matches!(value, Value::Json(_)));
    }

    #[test]
    fn test_convert_complex_nested() {
        let json_val = json!({"users": [{"name": "Alice"}]});
        let value = json_value_to_dbms_value(&json_val);
        assert!(matches!(value, Value::Json(_)));
    }

    // ===== Extraction with type conversion Tests =====

    #[test]
    fn test_extract_null_value() {
        let json = j(json!({"value": null}));
        let segments = vec![PathSegment::Key("value".to_string())];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Null));
    }

    #[test]
    fn test_extract_boolean_value() {
        let json = j(json!({"active": true}));
        let segments = vec![PathSegment::Key("active".to_string())];

        let result = extract_at_path(&json, &segments);
        assert_eq!(result, Some(Value::Boolean(true.into())));
    }

    #[test]
    fn test_extract_nested_object_as_json() {
        let json = j(json!({"data": {"nested": {"key": "value"}}}));
        let segments = vec![
            PathSegment::Key("data".to_string()),
            PathSegment::Key("nested".to_string()),
        ];

        let result = extract_at_path(&json, &segments);
        assert!(matches!(result, Some(Value::Json(_))));
    }

    #[test]
    fn test_extract_nested_array_as_json() {
        let json = j(json!({"items": [[1, 2], [3, 4]]}));
        let segments = vec![PathSegment::Key("items".to_string()), PathSegment::Index(0)];

        let result = extract_at_path(&json, &segments);
        assert!(matches!(result, Some(Value::Json(_))));
    }
}

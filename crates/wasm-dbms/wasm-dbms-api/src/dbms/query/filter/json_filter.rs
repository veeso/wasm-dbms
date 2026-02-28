//! This module contains utilities for filtering JSON data in the DBMS API.
//!
//! It provides JSON-specific filter operations including structural containment,
//! path-based value extraction, and key existence checks.

mod contains;
mod extract;
pub mod path;

use serde::{Deserialize, Serialize};

use self::contains::json_contains;
use self::extract::extract_at_path;
use self::path::parse_path;
use crate::dbms::query::QueryResult;
use crate::prelude::{Json, Value};

/// Represents comparison operations for JSON values.
///
/// Used with [`JsonFilter::Extract`] to compare extracted values against targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum JsonCmp {
    /// Equal comparison.
    Eq(Value),
    /// Not equal comparison.
    Ne(Value),
    /// Greater than comparison.
    Gt(Value),
    /// Less than comparison.
    Lt(Value),
    /// Greater than or equal comparison.
    Ge(Value),
    /// Less than or equal comparison.
    Le(Value),
    /// Check if value is in the provided list.
    In(Vec<Value>),
    /// Check if value is null or path doesn't exist.
    IsNull,
    /// Check if value is not null.
    NotNull,
}

impl JsonCmp {
    /// Matches the given JSON `value` against the comparison operation.
    ///
    /// In case the provided value is `None`, and the comparison is `IsNull`, it returns `true`.
    /// Otherwise, it returns `false` for `None` values.
    ///
    /// # Arguments
    ///
    /// * `value` - An optional JSON value to compare.
    ///
    /// # Returns
    ///
    /// `true` if the value matches the comparison operation, `false` otherwise.
    pub fn matches(&self, value: Option<Value>) -> bool {
        match (value, self) {
            (None, JsonCmp::IsNull) => true,
            (None, _) => false,
            (Some(v), JsonCmp::IsNull) => v.is_null(),
            (Some(v), JsonCmp::NotNull) => !v.is_null(),
            (Some(v), JsonCmp::Eq(target)) => v == *target,
            (Some(v), JsonCmp::Ne(target)) => v != *target,
            (Some(v), JsonCmp::Gt(target)) => v > *target,
            (Some(v), JsonCmp::Lt(target)) => v < *target,
            (Some(v), JsonCmp::Ge(target)) => v >= *target,
            (Some(v), JsonCmp::Le(target)) => v <= *target,
            (Some(v), JsonCmp::In(list)) => list.contains(&v),
        }
    }
}

/// JSON-specific filter operations.
///
/// Provides three types of filtering on JSON columns:
/// - **Contains**: Structural containment check (PostgreSQL `@>` style)
/// - **Extract**: Extract value at path and apply comparison
/// - **HasKey**: Check if a path exists in the JSON
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum JsonFilter {
    /// Structural containment: column's JSON contains the pattern.
    ///
    /// Implements PostgreSQL `@>` style containment:
    /// - Objects: all key-value pairs in pattern exist in target (recursive)
    /// - Arrays: all elements in pattern exist in target (order-independent)
    /// - Primitives: must be equal
    Contains(Json),
    /// Extract a value at the specified JSON path and apply a comparison operation.
    ///
    /// Paths use dot notation with bracket array indices: `user.items[0].name`
    Extract(String, JsonCmp),
    /// Check whether a path/key exists in the JSON structure.
    ///
    /// Paths use dot notation with bracket array indices: `user.items[0].name`
    HasKey(String),
}

impl JsonFilter {
    /// Matches the JSON value against this filter.
    ///
    /// # Arguments
    ///
    /// * `json` - The JSON value to match against.
    ///
    /// # Returns
    ///
    /// `Ok(true)` if the JSON matches the filter, `Ok(false)` otherwise.
    /// Returns `Err` if the path is invalid.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::InvalidQuery`] if the path syntax is invalid.
    pub fn matches(&self, json: &Json) -> QueryResult<bool> {
        match self {
            JsonFilter::Contains(pattern) => Ok(json_contains(json.value(), pattern.value())),
            JsonFilter::Extract(path, cmp) => {
                let segments = parse_path(path)?;
                let extracted = extract_at_path(json, &segments);
                Ok(cmp.matches(extracted))
            }
            JsonFilter::HasKey(path) => {
                let segments = parse_path(path)?;
                Ok(extract_at_path(json, &segments).is_some())
            }
        }
    }

    /// Creates a `Contains` filter with the given JSON pattern.
    pub fn contains(pattern: Json) -> Self {
        JsonFilter::Contains(pattern)
    }

    /// Creates an `Extract` filter with an equality comparison.
    pub fn extract_eq(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Eq(value))
    }

    /// Creates an `Extract` filter with a not-equal comparison.
    pub fn extract_ne(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Ne(value))
    }

    /// Creates an `Extract` filter with a greater-than comparison.
    pub fn extract_gt(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Gt(value))
    }

    /// Creates an `Extract` filter with a less-than comparison.
    pub fn extract_lt(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Lt(value))
    }

    /// Creates an `Extract` filter with a greater-than-or-equal comparison.
    pub fn extract_ge(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Ge(value))
    }

    /// Creates an `Extract` filter with a less-than-or-equal comparison.
    pub fn extract_le(path: &str, value: Value) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::Le(value))
    }

    /// Creates an `Extract` filter with an `In` comparison.
    pub fn extract_in(path: &str, values: Vec<Value>) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::In(values))
    }

    /// Creates an `Extract` filter checking for null.
    pub fn extract_is_null(path: &str) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::IsNull)
    }

    /// Creates an `Extract` filter checking for not null.
    pub fn extract_not_null(path: &str) -> Self {
        JsonFilter::Extract(path.to_string(), JsonCmp::NotNull)
    }

    /// Creates a `HasKey` filter.
    pub fn has_key(path: &str) -> Self {
        JsonFilter::HasKey(path.to_string())
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde_json::json;

    use super::*;
    use crate::dbms::query::QueryError;

    // Helper to create Json from serde_json::Value
    fn j(v: serde_json::Value) -> Json {
        Json::from(v)
    }

    // ===== JsonCmp Tests =====

    #[test]
    fn test_cmp_eq_matches() {
        let cmp = JsonCmp::Eq(Value::Int64(42.into()));
        assert!(cmp.matches(Some(Value::Int64(42.into()))));
        assert!(!cmp.matches(Some(Value::Int64(43.into()))));
    }

    #[test]
    fn test_cmp_ne_matches() {
        let cmp = JsonCmp::Ne(Value::Int64(42.into()));
        assert!(cmp.matches(Some(Value::Int64(43.into()))));
        assert!(!cmp.matches(Some(Value::Int64(42.into()))));
    }

    #[test]
    fn test_cmp_gt_matches() {
        let cmp = JsonCmp::Gt(Value::Int64(10.into()));
        assert!(cmp.matches(Some(Value::Int64(20.into()))));
        assert!(!cmp.matches(Some(Value::Int64(10.into()))));
        assert!(!cmp.matches(Some(Value::Int64(5.into()))));
    }

    #[test]
    fn test_cmp_lt_matches() {
        let cmp = JsonCmp::Lt(Value::Int64(10.into()));
        assert!(cmp.matches(Some(Value::Int64(5.into()))));
        assert!(!cmp.matches(Some(Value::Int64(10.into()))));
        assert!(!cmp.matches(Some(Value::Int64(20.into()))));
    }

    #[test]
    fn test_cmp_ge_matches() {
        let cmp = JsonCmp::Ge(Value::Int64(10.into()));
        assert!(cmp.matches(Some(Value::Int64(20.into()))));
        assert!(cmp.matches(Some(Value::Int64(10.into()))));
        assert!(!cmp.matches(Some(Value::Int64(5.into()))));
    }

    #[test]
    fn test_cmp_le_matches() {
        let cmp = JsonCmp::Le(Value::Int64(10.into()));
        assert!(cmp.matches(Some(Value::Int64(5.into()))));
        assert!(cmp.matches(Some(Value::Int64(10.into()))));
        assert!(!cmp.matches(Some(Value::Int64(20.into()))));
    }

    #[test]
    fn test_cmp_in_matches() {
        let cmp = JsonCmp::In(vec![
            Value::Int64(1.into()),
            Value::Int64(2.into()),
            Value::Int64(3.into()),
        ]);
        assert!(cmp.matches(Some(Value::Int64(2.into()))));
        assert!(!cmp.matches(Some(Value::Int64(4.into()))));
    }

    #[test]
    fn test_cmp_is_null_matches() {
        let cmp = JsonCmp::IsNull;
        assert!(cmp.matches(None));
        assert!(cmp.matches(Some(Value::Null)));
        assert!(!cmp.matches(Some(Value::Int64(42.into()))));
    }

    #[test]
    fn test_cmp_not_null_matches() {
        let cmp = JsonCmp::NotNull;
        assert!(cmp.matches(Some(Value::Int64(42.into()))));
        assert!(!cmp.matches(Some(Value::Null)));
        assert!(!cmp.matches(None));
    }

    #[test]
    fn test_cmp_none_value() {
        assert!(!JsonCmp::Eq(Value::Int64(42.into())).matches(None));
        assert!(!JsonCmp::Ne(Value::Int64(42.into())).matches(None));
        assert!(!JsonCmp::Gt(Value::Int64(42.into())).matches(None));
        assert!(!JsonCmp::Lt(Value::Int64(42.into())).matches(None));
        assert!(JsonCmp::IsNull.matches(None));
        assert!(!JsonCmp::NotNull.matches(None));
    }

    // ===== JsonFilter::Contains Tests =====

    #[test]
    fn test_filter_contains_object() {
        let json = j(json!({"a": 1, "b": 2}));
        let filter = JsonFilter::contains(j(json!({"a": 1})));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_contains_object_no_match() {
        let json = j(json!({"a": 1}));
        let filter = JsonFilter::contains(j(json!({"a": 1, "b": 2})));
        assert!(!filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_contains_array() {
        let json = j(json!([1, 2, 3]));
        let filter = JsonFilter::contains(j(json!([3, 1])));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_contains_nested() {
        let json = j(json!({"user": {"name": "Alice", "age": 30}}));
        let filter = JsonFilter::contains(j(json!({"user": {"name": "Alice"}})));
        assert!(filter.matches(&json).unwrap());
    }

    // ===== JsonFilter::Extract Tests =====

    #[test]
    fn test_filter_extract_eq() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::extract_eq("user.name", Value::Text("Alice".into()));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_eq_no_match() {
        let json = j(json!({"user": {"name": "Bob"}}));
        let filter = JsonFilter::extract_eq("user.name", Value::Text("Alice".into()));
        assert!(!filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_gt_number() {
        let json = j(json!({"user": {"age": 25}}));
        let filter = JsonFilter::extract_gt("user.age", Value::Int64(18.into()));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_array_index() {
        let json = j(json!({"items": [10, 20, 30]}));
        let filter = JsonFilter::extract_eq("items[1]", Value::Int64(20.into()));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_complex_path() {
        let json = j(json!({"users": [{"name": "Alice"}, {"name": "Bob"}]}));
        let filter = JsonFilter::extract_eq("users[0].name", Value::Text("Alice".into()));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_missing_path() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::extract_eq("user.email", Value::Text("alice@example.com".into()));
        assert!(!filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_is_null_missing() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::extract_is_null("user.email");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_is_null_explicit() {
        let json = j(json!({"user": {"name": null}}));
        let filter = JsonFilter::extract_is_null("user.name");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_not_null() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::extract_not_null("user.name");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_in_list() {
        let json = j(json!({"status": "active"}));
        let filter = JsonFilter::extract_in(
            "status",
            vec![Value::Text("active".into()), Value::Text("pending".into())],
        );
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_extract_invalid_path() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::Extract("user.".to_string(), JsonCmp::IsNull);
        let result = filter.matches(&json);
        assert!(result.is_err());
        assert!(matches!(result, Err(QueryError::InvalidQuery(_))));
    }

    // ===== JsonFilter::HasKey Tests =====

    #[test]
    fn test_filter_has_key_exists() {
        let json = j(json!({"user": {"name": "Alice", "email": "alice@example.com"}}));
        let filter = JsonFilter::has_key("user.email");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_not_exists() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::has_key("user.email");
        assert!(!filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_nested() {
        let json = j(json!({"a": {"b": {"c": 1}}}));
        let filter = JsonFilter::has_key("a.b.c");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_array_index() {
        let json = j(json!({"items": [1, 2, 3]}));
        let filter = JsonFilter::has_key("items[1]");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_array_index_out_of_bounds() {
        let json = j(json!({"items": [1, 2, 3]}));
        let filter = JsonFilter::has_key("items[10]");
        assert!(!filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_null_value() {
        let json = j(json!({"user": {"name": null}}));
        let filter = JsonFilter::has_key("user.name");
        // Path exists even though value is null
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_has_key_invalid_path() {
        let json = j(json!({"user": {"name": "Alice"}}));
        let filter = JsonFilter::HasKey("".to_string());
        let result = filter.matches(&json);
        assert!(result.is_err());
    }

    // ===== Builder Methods Tests =====

    #[test]
    fn test_contains_builder() {
        let filter = JsonFilter::contains(j(json!({"a": 1})));
        assert!(matches!(filter, JsonFilter::Contains(_)));
    }

    #[test]
    fn test_extract_eq_builder() {
        let filter = JsonFilter::extract_eq("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Eq(_))));
    }

    #[test]
    fn test_extract_ne_builder() {
        let filter = JsonFilter::extract_ne("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Ne(_))));
    }

    #[test]
    fn test_extract_gt_builder() {
        let filter = JsonFilter::extract_gt("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Gt(_))));
    }

    #[test]
    fn test_extract_lt_builder() {
        let filter = JsonFilter::extract_lt("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Lt(_))));
    }

    #[test]
    fn test_extract_ge_builder() {
        let filter = JsonFilter::extract_ge("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Ge(_))));
    }

    #[test]
    fn test_extract_le_builder() {
        let filter = JsonFilter::extract_le("path", Value::Int64(42.into()));
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::Le(_))));
    }

    #[test]
    fn test_extract_in_builder() {
        let filter = JsonFilter::extract_in("path", vec![Value::Int64(1.into())]);
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::In(_))));
    }

    #[test]
    fn test_extract_is_null_builder() {
        let filter = JsonFilter::extract_is_null("path");
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::IsNull)));
    }

    #[test]
    fn test_extract_not_null_builder() {
        let filter = JsonFilter::extract_not_null("path");
        assert!(matches!(filter, JsonFilter::Extract(_, JsonCmp::NotNull)));
    }

    #[test]
    fn test_has_key_builder() {
        let filter = JsonFilter::has_key("path");
        assert!(matches!(filter, JsonFilter::HasKey(_)));
    }

    // ===== Integration Tests =====

    #[test]
    fn test_filter_from_plan_example_1() {
        // Filter where data.user.name = "Alice"
        let json = j(json!({"user": {"name": "Alice", "age": 30}}));
        let filter = JsonFilter::extract_eq("user.name", Value::Text("Alice".into()));
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_from_plan_example_2() {
        // Filter where data contains {"active": true}
        let json = j(json!({"active": true, "name": "Test"}));
        let filter = JsonFilter::contains(Json::from_str(r#"{"active": true}"#).unwrap());
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_filter_from_plan_example_3() {
        // Filter where data has "email" key
        let json = j(json!({"email": "alice@example.com", "name": "Alice"}));
        let filter = JsonFilter::has_key("email");
        assert!(filter.matches(&json).unwrap());
    }

    #[test]
    fn test_serde_roundtrip_json_filter() {
        let filter = JsonFilter::extract_eq("user.name", Value::Text("Alice".into()));
        let serialized = serde_json::to_string(&filter).unwrap();
        let deserialized: JsonFilter = serde_json::from_str(&serialized).unwrap();
        assert_eq!(filter, deserialized);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_roundtrip_json_filter() {
        let filter = JsonFilter::extract_gt("age", Value::Int64(18.into()));
        let encoded = candid::encode_one(&filter).unwrap();
        let decoded: JsonFilter = candid::decode_one(&encoded).unwrap();
        assert_eq!(filter, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_roundtrip_json_cmp() {
        let cmp = JsonCmp::In(vec![Value::Int64(1.into()), Value::Int64(2.into())]);
        let encoded = candid::encode_one(&cmp).unwrap();
        let decoded: JsonCmp = candid::decode_one(&encoded).unwrap();
        assert_eq!(cmp, decoded);
    }
}

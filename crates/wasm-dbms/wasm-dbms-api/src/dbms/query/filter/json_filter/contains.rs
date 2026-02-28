//! JSON structural containment utilities.
//!
//! This module provides PostgreSQL `@>` style structural containment checks
//! for JSON values.

use serde_json::Value as JsonValue;

/// Checks if `haystack` structurally contains `needle`.
///
/// This implements PostgreSQL `@>` style containment semantics:
///
/// - **Objects**: All key-value pairs in `needle` must exist in `haystack` (recursive).
/// - **Arrays**: All elements in `needle` must exist in `haystack` (order-independent).
/// - **Primitives**: Must be strictly equal.
///
/// # Examples
///
/// | Haystack | Needle | Result |
/// |----------|--------|--------|
/// | `{"a": 1, "b": 2}` | `{"a": 1}` | `true` |
/// | `{"a": 1}` | `{"a": 1, "b": 2}` | `false` |
/// | `{"user": {"name": "Alice", "age": 30}}` | `{"user": {"name": "Alice"}}` | `true` |
/// | `[1, 2, 3]` | `[3, 1]` | `true` |
/// | `[1, 2]` | `[1, 2, 3]` | `false` |
///
/// # Arguments
///
/// * `haystack` - The JSON value to search in.
/// * `needle` - The JSON pattern to search for.
///
/// # Returns
///
/// `true` if `haystack` contains `needle`, `false` otherwise.
pub fn json_contains(haystack: &JsonValue, needle: &JsonValue) -> bool {
    match (haystack, needle) {
        // Object containment: all key-value pairs in needle must exist in haystack
        (JsonValue::Object(h_map), JsonValue::Object(n_map)) => {
            for (key, n_value) in n_map {
                match h_map.get(key) {
                    Some(h_value) => {
                        if !json_contains(h_value, n_value) {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            true
        }

        // Array containment: all elements in needle must exist in haystack (order-independent)
        (JsonValue::Array(h_arr), JsonValue::Array(n_arr)) => {
            for n_elem in n_arr {
                // Check if any element in haystack contains this needle element
                let found = h_arr.iter().any(|h_elem| json_contains(h_elem, n_elem));
                if !found {
                    return false;
                }
            }
            true
        }

        // Special case: if needle is a primitive, check if haystack array contains it
        (JsonValue::Array(h_arr), needle) if !needle.is_array() && !needle.is_object() => {
            h_arr.iter().any(|h_elem| json_contains(h_elem, needle))
        }

        // Primitive equality
        (JsonValue::Null, JsonValue::Null) => true,
        (JsonValue::Bool(h), JsonValue::Bool(n)) => h == n,
        (JsonValue::Number(h), JsonValue::Number(n)) => h == n,
        (JsonValue::String(h), JsonValue::String(n)) => h == n,

        // Type mismatch - no containment
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // ===== Object Containment Tests =====

    #[test]
    fn test_object_exact_match() {
        let haystack = json!({"a": 1});
        let needle = json!({"a": 1});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_superset_contains_subset() {
        let haystack = json!({"a": 1, "b": 2});
        let needle = json!({"a": 1});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_superset_contains_empty() {
        let haystack = json!({"a": 1, "b": 2});
        let needle = json!({});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_subset_does_not_contain_superset() {
        let haystack = json!({"a": 1});
        let needle = json!({"a": 1, "b": 2});
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_missing_key() {
        let haystack = json!({"a": 1});
        let needle = json!({"b": 1});
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_different_value() {
        let haystack = json!({"a": 1});
        let needle = json!({"a": 2});
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_nested_containment() {
        let haystack = json!({"user": {"name": "Alice", "age": 30}});
        let needle = json!({"user": {"name": "Alice"}});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_deeply_nested() {
        let haystack = json!({"a": {"b": {"c": {"d": 1, "e": 2}}}});
        let needle = json!({"a": {"b": {"c": {"d": 1}}}});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_object_nested_missing_key() {
        let haystack = json!({"user": {"name": "Alice"}});
        let needle = json!({"user": {"name": "Alice", "email": "alice@example.com"}});
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_empty_object_contains_empty() {
        let haystack = json!({});
        let needle = json!({});
        assert!(json_contains(&haystack, &needle));
    }

    // ===== Array Containment Tests =====

    #[test]
    fn test_array_exact_match() {
        let haystack = json!([1, 2, 3]);
        let needle = json!([1, 2, 3]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_superset_contains_subset() {
        let haystack = json!([1, 2, 3]);
        let needle = json!([1, 2]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_order_independent() {
        let haystack = json!([1, 2, 3]);
        let needle = json!([3, 1]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_superset_contains_empty() {
        let haystack = json!([1, 2, 3]);
        let needle = json!([]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_subset_does_not_contain_superset() {
        let haystack = json!([1, 2]);
        let needle = json!([1, 2, 3]);
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_missing_element() {
        let haystack = json!([1, 2, 3]);
        let needle = json!([4]);
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_with_duplicates_haystack() {
        let haystack = json!([1, 1, 2]);
        let needle = json!([1]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_with_duplicates_needle() {
        // Haystack has one 1, needle asks for two 1s
        // Each needle element needs to find a containing element in haystack
        // Both needle 1s can match the same haystack 1
        let haystack = json!([1, 2, 3]);
        let needle = json!([1, 1]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_empty_array_contains_empty() {
        let haystack = json!([]);
        let needle = json!([]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_of_objects() {
        let haystack = json!([{"a": 1}, {"b": 2}]);
        let needle = json!([{"a": 1}]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_of_objects_partial_match() {
        let haystack = json!([{"a": 1, "b": 2}, {"c": 3}]);
        let needle = json!([{"a": 1}]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_of_objects_no_match() {
        let haystack = json!([{"a": 1}, {"b": 2}]);
        let needle = json!([{"c": 3}]);
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_nested_arrays() {
        let haystack = json!([[1, 2], [3, 4]]);
        let needle = json!([[1, 2]]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_nested_arrays_partial() {
        let haystack = json!([[1, 2, 3], [4, 5]]);
        let needle = json!([[1, 2]]);
        assert!(json_contains(&haystack, &needle));
    }

    // ===== Primitive Containment Tests =====

    #[test]
    fn test_null_equals_null() {
        assert!(json_contains(&json!(null), &json!(null)));
    }

    #[test]
    fn test_bool_true_equals_true() {
        assert!(json_contains(&json!(true), &json!(true)));
    }

    #[test]
    fn test_bool_false_equals_false() {
        assert!(json_contains(&json!(false), &json!(false)));
    }

    #[test]
    fn test_bool_true_not_equals_false() {
        assert!(!json_contains(&json!(true), &json!(false)));
    }

    #[test]
    fn test_number_integer_equals() {
        assert!(json_contains(&json!(42), &json!(42)));
    }

    #[test]
    fn test_number_integer_not_equals() {
        assert!(!json_contains(&json!(42), &json!(43)));
    }

    #[test]
    fn test_number_float_equals() {
        assert!(json_contains(&json!(3.14), &json!(3.14)));
    }

    #[test]
    fn test_number_negative() {
        assert!(json_contains(&json!(-10), &json!(-10)));
    }

    #[test]
    fn test_string_equals() {
        assert!(json_contains(&json!("hello"), &json!("hello")));
    }

    #[test]
    fn test_string_not_equals() {
        assert!(!json_contains(&json!("hello"), &json!("world")));
    }

    #[test]
    fn test_empty_string_equals() {
        assert!(json_contains(&json!(""), &json!("")));
    }

    // ===== Type Mismatch Tests =====

    #[test]
    fn test_object_does_not_contain_array() {
        assert!(!json_contains(&json!({"a": 1}), &json!([1])));
    }

    #[test]
    fn test_array_does_not_contain_object() {
        assert!(!json_contains(&json!([1, 2]), &json!({"a": 1})));
    }

    #[test]
    fn test_string_does_not_contain_number() {
        assert!(!json_contains(&json!("42"), &json!(42)));
    }

    #[test]
    fn test_number_does_not_contain_string() {
        assert!(!json_contains(&json!(42), &json!("42")));
    }

    #[test]
    fn test_null_does_not_contain_bool() {
        assert!(!json_contains(&json!(null), &json!(false)));
    }

    #[test]
    fn test_bool_does_not_contain_null() {
        assert!(!json_contains(&json!(false), &json!(null)));
    }

    // ===== Mixed Nesting Tests =====

    #[test]
    fn test_object_with_array_value() {
        let haystack = json!({"items": [1, 2, 3], "name": "test"});
        let needle = json!({"items": [1, 2]});
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_with_object_elements() {
        let haystack = json!([{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]);
        let needle = json!([{"id": 1}]);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_complex_nested_structure() {
        let haystack = json!({
            "users": [
                {"name": "Alice", "roles": ["admin", "user"]},
                {"name": "Bob", "roles": ["user"]}
            ],
            "metadata": {"version": 1}
        });
        let needle = json!({
            "users": [{"name": "Alice", "roles": ["admin"]}]
        });
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_complex_nested_structure_fail() {
        let haystack = json!({
            "users": [
                {"name": "Alice", "roles": ["user"]},
                {"name": "Bob", "roles": ["user"]}
            ]
        });
        let needle = json!({
            "users": [{"name": "Alice", "roles": ["admin"]}]
        });
        assert!(!json_contains(&haystack, &needle));
    }

    // ===== Array containing primitive needle =====

    #[test]
    fn test_array_contains_primitive_number() {
        let haystack = json!([1, 2, 3]);
        let needle = json!(2);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_contains_primitive_string() {
        let haystack = json!(["a", "b", "c"]);
        let needle = json!("b");
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_does_not_contain_primitive() {
        let haystack = json!([1, 2, 3]);
        let needle = json!(4);
        assert!(!json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_contains_null() {
        let haystack = json!([1, null, 3]);
        let needle = json!(null);
        assert!(json_contains(&haystack, &needle));
    }

    #[test]
    fn test_array_contains_boolean() {
        let haystack = json!([true, false, 1]);
        let needle = json!(false);
        assert!(json_contains(&haystack, &needle));
    }
}

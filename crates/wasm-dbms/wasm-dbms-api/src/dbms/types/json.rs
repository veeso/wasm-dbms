//! This module exposes the [`Json`] data type used in the ic-dbms system.

use std::cmp::Ordering;
use std::hash::Hash;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::memory::{DataSize, DecodeError, MSize, PageOffset};
use crate::prelude::{DEFAULT_ALIGNMENT, DataType, Encode};

/// Bytes reserved to store the length of the JSON string.
const LEN_SIZE: MSize = 2;

/// Json data type for the DBMS.
///
/// It contains a JSON object represented using [`serde_json::Value`].
///
/// A default instance of [`Json`] is `null`.
///
/// # Ordering
///
/// Json values are ordered using a type-based hierarchical ordering scheme:
///
/// 1. **Type precedence**: `Null < Bool < Number < String < Array < Object`
/// 2. **Within same type**:
///    - Null: all equal
///    - Bool: `false < true`
///    - Number: numeric comparison (integers compared as i64, floats as f64)
///    - String: lexicographic comparison
///    - Array: element-wise lexicographic comparison
///    - Object: keys sorted alphabetically, then key-value pairs compared lexicographically
///
/// This ordering is deterministic and suitable for indexing and sorting operations,
/// though comparing values of different types may not be semantically meaningful.
#[derive(Clone, Debug, Eq)]
pub struct Json {
    /// JSON value
    value: Value,
    /// String representation cache
    repr: String,
}

impl Default for Json {
    fn default() -> Self {
        Self {
            value: Value::Null,
            repr: "null".to_string(),
        }
    }
}

impl From<Value> for Json {
    fn from(value: Value) -> Self {
        let repr = value.to_string();
        Self { value, repr }
    }
}

impl Json {
    /// Returns a reference to the underlying JSON value.
    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl PartialEq for Json {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl std::hash::Hash for Json {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        hash_value(&self.value, state);
    }
}

/// Recursively hashes a JSON value.
fn hash_value<H: std::hash::Hasher>(v: &Value, state: &mut H) {
    // Hash the type discriminant first
    std::mem::discriminant(v).hash(state);

    match v {
        Value::Null => {}
        Value::Bool(b) => b.hash(state),
        Value::Number(n) => {
            // Hash the string representation for consistent hashing across int/float
            n.to_string().hash(state);
        }
        Value::String(s) => s.hash(state),
        Value::Array(arr) => {
            arr.len().hash(state);
            for elem in arr {
                hash_value(elem, state);
            }
        }
        Value::Object(obj) => {
            obj.len().hash(state);
            // Sort keys for consistent hashing
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            for key in keys {
                key.hash(state);
                hash_value(obj.get(key).unwrap(), state);
            }
        }
    }
}

impl PartialOrd for Json {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Json {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_value(&self.value, &other.value)
    }
}

/// Returns the type precedence order for a JSON value.
///
/// Type ordering: Null < Bool < Number < String < Array < Object
fn type_order(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

/// Compares two JSON values using hierarchical type-based ordering.
fn cmp_value(a: &Value, b: &Value) -> Ordering {
    let type_ord = type_order(a).cmp(&type_order(b));
    if type_ord != Ordering::Equal {
        return type_ord;
    }

    // Same type, compare within type
    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => cmp_number(a, b),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Array(a), Value::Array(b)) => cmp_array(a, b),
        (Value::Object(a), Value::Object(b)) => cmp_object(a, b),
        // Unreachable because type_order already ensured same types
        _ => unreachable!(),
    }
}

/// Compares two JSON numbers.
///
/// Integers are compared as i64, floats as f64.
/// When comparing int to float, both are converted to f64.
fn cmp_number(a: &serde_json::Number, b: &serde_json::Number) -> Ordering {
    // Try integer comparison first (most common case)
    if let (Some(a_i64), Some(b_i64)) = (a.as_i64(), b.as_i64()) {
        return a_i64.cmp(&b_i64);
    }

    // Try unsigned integer comparison
    if let (Some(a_u64), Some(b_u64)) = (a.as_u64(), b.as_u64()) {
        return a_u64.cmp(&b_u64);
    }

    // Fall back to float comparison
    let a_f64 = a.as_f64().unwrap_or(f64::NAN);
    let b_f64 = b.as_f64().unwrap_or(f64::NAN);

    // Use total_cmp for deterministic NaN handling
    a_f64.total_cmp(&b_f64)
}

/// Compares two JSON arrays element-wise.
fn cmp_array(a: &[Value], b: &[Value]) -> Ordering {
    for (a_elem, b_elem) in a.iter().zip(b.iter()) {
        let ord = cmp_value(a_elem, b_elem);
        if ord != Ordering::Equal {
            return ord;
        }
    }
    // All compared elements are equal, compare lengths
    a.len().cmp(&b.len())
}

/// Compares two JSON objects by sorted keys and their values.
fn cmp_object(a: &serde_json::Map<String, Value>, b: &serde_json::Map<String, Value>) -> Ordering {
    // Collect and sort keys from both objects
    let mut a_keys: Vec<&String> = a.keys().collect();
    let mut b_keys: Vec<&String> = b.keys().collect();
    a_keys.sort();
    b_keys.sort();

    // Compare key-value pairs
    for (a_key, b_key) in a_keys.iter().zip(b_keys.iter()) {
        // Compare keys first
        let key_ord = a_key.cmp(b_key);
        if key_ord != Ordering::Equal {
            return key_ord;
        }

        // Keys are equal, compare values
        let a_val = a.get(*a_key).unwrap();
        let b_val = b.get(*b_key).unwrap();
        let val_ord = cmp_value(a_val, b_val);
        if val_ord != Ordering::Equal {
            return val_ord;
        }
    }

    // All compared pairs are equal, compare by number of keys
    a.len().cmp(&b.len())
}

#[cfg(feature = "candid")]
impl candid::CandidType for Json {
    fn _ty() -> candid::types::Type {
        candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Text))
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        serializer.serialize_text(&self.repr)
    }
}

impl Serialize for Json {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.repr)
    }
}

impl<'de> Deserialize<'de> for Json {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // deserialize as string (matching the Serialize and CandidType impls)
        let s: String = serde::Deserialize::deserialize(deserializer)?;
        // parse json_filter
        let value: Value = serde_json::from_str(&s).map_err(serde::de::Error::custom)?;
        Ok(Self { value, repr: s })
    }
}

impl Encode for Json {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        // put 2 bytes for length
        let len = self.repr.len() as u16;
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(self.repr.as_bytes());
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < LEN_SIZE as usize {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        // read length
        let str_len = {
            let mut len_bytes = [0u8; LEN_SIZE as usize];
            len_bytes.copy_from_slice(&data[0..LEN_SIZE as usize]);
            u16::from_le_bytes(len_bytes) as usize
        };

        if data.len() < 2 + str_len {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let string_bytes = &data[2..2 + str_len];
        let string = String::from_utf8(string_bytes.to_vec())?;
        // parse
        let value: Value = serde_json::from_str(&string).map_err(|e| {
            crate::memory::MemoryError::DecodeError(DecodeError::InvalidJson(e.to_string()))
        })?;

        Ok(Self {
            value,
            repr: string,
        })
    }

    fn size(&self) -> MSize {
        self.repr.len() as MSize + LEN_SIZE
    }
}

impl FromStr for Json {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value: Value = serde_json::from_str(s)?;
        Ok(Json {
            value,
            repr: s.to_string(),
        })
    }
}

impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.repr)
    }
}

impl DataType for Json {}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use serde_json::json;

    use super::*;

    // Helper to create Json from serde_json::Value
    fn j(v: Value) -> Json {
        Json::from(v)
    }

    // ===================
    // Type precedence tests
    // ===================

    #[test]
    fn test_type_order_null_is_smallest() {
        assert!(j(json!(null)) < j(json!(false)));
        assert!(j(json!(null)) < j(json!(0)));
        assert!(j(json!(null)) < j(json!("")));
        assert!(j(json!(null)) < j(json!([])));
        assert!(j(json!(null)) < j(json!({})));
    }

    #[test]
    fn test_type_order_bool_less_than_number() {
        assert!(j(json!(true)) < j(json!(0)));
        assert!(j(json!(false)) < j(json!(-999)));
    }

    #[test]
    fn test_type_order_number_less_than_string() {
        assert!(j(json!(999999)) < j(json!("")));
        assert!(j(json!(-1)) < j(json!("a")));
    }

    #[test]
    fn test_type_order_string_less_than_array() {
        assert!(j(json!("zzz")) < j(json!([])));
        assert!(j(json!("")) < j(json!([1, 2, 3])));
    }

    #[test]
    fn test_type_order_array_less_than_object() {
        assert!(j(json!([1, 2, 3])) < j(json!({})));
        assert!(j(json!([])) < j(json!({"a": 1})));
    }

    // ===================
    // Null comparisons
    // ===================

    #[test]
    fn test_null_equals_null() {
        assert_eq!(j(json!(null)).cmp(&j(json!(null))), Ordering::Equal);
        assert_eq!(j(json!(null)), j(json!(null)));
    }

    // ===================
    // Bool comparisons
    // ===================

    #[test]
    fn test_bool_false_less_than_true() {
        assert!(j(json!(false)) < j(json!(true)));
    }

    #[test]
    fn test_bool_equals() {
        assert_eq!(j(json!(false)), j(json!(false)));
        assert_eq!(j(json!(true)), j(json!(true)));
    }

    // ===================
    // Number comparisons
    // ===================

    #[test]
    fn test_number_positive_integers() {
        assert!(j(json!(1)) < j(json!(2)));
        assert!(j(json!(0)) < j(json!(100)));
        assert_eq!(j(json!(42)), j(json!(42)));
    }

    #[test]
    fn test_number_negative_integers() {
        assert!(j(json!(-10)) < j(json!(-5)));
        assert!(j(json!(-100)) < j(json!(0)));
        assert_eq!(j(json!(-42)), j(json!(-42)));
    }

    #[test]
    fn test_number_floats() {
        assert!(j(json!(1.5)) < j(json!(2.5)));
        assert!(j(json!(-1.5)) < j(json!(1.5)));
        assert_eq!(j(json!(3.14)), j(json!(3.14)));
    }

    #[test]
    fn test_number_int_vs_float() {
        assert!(j(json!(1)) < j(json!(1.5)));
        assert!(j(json!(1.5)) < j(json!(2)));
        assert_eq!(j(json!(2.0)), j(json!(2.0)));
    }

    #[test]
    fn test_number_large_values() {
        assert!(j(json!(i64::MAX - 1)) < j(json!(i64::MAX)));
        assert!(j(json!(i64::MIN)) < j(json!(i64::MIN + 1)));
    }

    // ===================
    // String comparisons
    // ===================

    #[test]
    fn test_string_lexicographic() {
        assert!(j(json!("a")) < j(json!("b")));
        assert!(j(json!("abc")) < j(json!("abd")));
        assert!(j(json!("")) < j(json!("a")));
    }

    #[test]
    fn test_string_prefix() {
        assert!(j(json!("ab")) < j(json!("abc")));
        assert!(j(json!("")) < j(json!("x")));
    }

    #[test]
    fn test_string_equals() {
        assert_eq!(j(json!("hello")), j(json!("hello")));
        assert_eq!(j(json!("")), j(json!("")));
    }

    #[test]
    fn test_string_case_sensitive() {
        assert!(j(json!("A")) < j(json!("a"))); // ASCII: 'A' (65) < 'a' (97)
        assert!(j(json!("Z")) < j(json!("a")));
    }

    // ===================
    // Array comparisons
    // ===================

    #[test]
    fn test_array_element_wise() {
        assert!(j(json!([1, 2])) < j(json!([1, 3])));
        assert!(j(json!([1, 2])) < j(json!([2, 1])));
        assert!(j(json!(["a"])) < j(json!(["b"])));
    }

    #[test]
    fn test_array_prefix() {
        assert!(j(json!([1])) < j(json!([1, 2])));
        assert!(j(json!([])) < j(json!([1])));
    }

    #[test]
    fn test_array_equals() {
        assert_eq!(j(json!([1, 2, 3])), j(json!([1, 2, 3])));
        assert_eq!(j(json!([])), j(json!([])));
    }

    #[test]
    fn test_array_nested() {
        assert!(j(json!([[1]])) < j(json!([[2]])));
        assert!(j(json!([[1, 2]])) < j(json!([[1, 3]])));
    }

    #[test]
    fn test_array_mixed_types() {
        // Within array, type ordering applies
        assert!(j(json!([null])) < j(json!([false])));
        assert!(j(json!([1])) < j(json!(["a"])));
    }

    // ===================
    // Object comparisons
    // ===================

    #[test]
    fn test_object_by_sorted_keys() {
        // "a" < "b" lexicographically
        assert!(j(json!({"a": 1})) < j(json!({"b": 1})));
    }

    #[test]
    fn test_object_same_key_different_value() {
        assert!(j(json!({"x": 1})) < j(json!({"x": 2})));
        assert!(j(json!({"x": "a"})) < j(json!({"x": "b"})));
    }

    #[test]
    fn test_object_fewer_keys_is_smaller() {
        assert!(j(json!({"a": 1})) < j(json!({"a": 1, "b": 2})));
        assert!(j(json!({})) < j(json!({"a": 1})));
    }

    #[test]
    fn test_object_equals() {
        assert_eq!(j(json!({"a": 1, "b": 2})), j(json!({"a": 1, "b": 2})));
        assert_eq!(j(json!({})), j(json!({})));
    }

    #[test]
    fn test_object_key_order_independent() {
        // Objects with same keys in different order should be equal
        let a = Json::from_str(r#"{"a": 1, "b": 2}"#).unwrap();
        let b = Json::from_str(r#"{"b": 2, "a": 1}"#).unwrap();
        assert_eq!(a.cmp(&b), Ordering::Equal);
    }

    #[test]
    fn test_object_nested() {
        assert!(j(json!({"x": {"a": 1}})) < j(json!({"x": {"a": 2}})));
        assert!(j(json!({"x": {"a": 1}})) < j(json!({"x": {"b": 1}})));
    }

    // ===================
    // Complex nested comparisons
    // ===================

    #[test]
    fn test_deeply_nested_structure() {
        let a = json!({"level1": {"level2": {"level3": [1, 2, 3]}}});
        let b = json!({"level1": {"level2": {"level3": [1, 2, 4]}}});
        assert!(j(a) < j(b));
    }

    #[test]
    fn test_array_of_objects() {
        let a = json!([{"a": 1}, {"b": 2}]);
        let b = json!([{"a": 1}, {"b": 3}]);
        assert!(j(a) < j(b));
    }

    #[test]
    fn test_object_with_arrays() {
        let a = json!({"items": [1, 2]});
        let b = json!({"items": [1, 3]});
        assert!(j(a) < j(b));
    }

    // ===================
    // Ord trait properties
    // ===================

    #[test]
    fn test_reflexive() {
        let values = vec![
            json!(null),
            json!(true),
            json!(42),
            json!("test"),
            json!([1, 2]),
            json!({"a": 1}),
        ];

        for v in values {
            let jv = j(v);
            assert_eq!(jv.cmp(&jv), Ordering::Equal);
        }
    }

    #[test]
    fn test_antisymmetric() {
        let a = j(json!(1));
        let b = j(json!(2));

        assert_eq!(a.cmp(&b), Ordering::Less);
        assert_eq!(b.cmp(&a), Ordering::Greater);
    }

    #[test]
    fn test_transitive() {
        let a = j(json!(1));
        let b = j(json!(2));
        let c = j(json!(3));

        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    // ===================
    // PartialOrd consistency
    // ===================

    #[test]
    fn test_partial_ord_consistent_with_ord() {
        let values = vec![
            json!(null),
            json!(false),
            json!(true),
            json!(-1),
            json!(0),
            json!(1),
            json!(""),
            json!("a"),
            json!([]),
            json!([1]),
            json!({}),
            json!({"a": 1}),
        ];

        for a in &values {
            for b in &values {
                let ja = j(a.clone());
                let jb = j(b.clone());
                assert_eq!(ja.partial_cmp(&jb), Some(ja.cmp(&jb)));
            }
        }
    }

    // ===================
    // Sorting test
    // ===================

    #[test]
    fn test_sorting() {
        let mut values = [
            j(json!({"a": 1})),
            j(json!([1, 2])),
            j(json!("hello")),
            j(json!(42)),
            j(json!(true)),
            j(json!(null)),
        ];

        values.sort();

        assert_eq!(values[0], j(json!(null)));
        assert_eq!(values[1], j(json!(true)));
        assert_eq!(values[2], j(json!(42)));
        assert_eq!(values[3], j(json!("hello")));
        assert_eq!(values[4], j(json!([1, 2])));
        assert_eq!(values[5], j(json!({"a": 1})));
    }

    // ===================
    // Edge cases
    // ===================

    #[test]
    fn test_empty_vs_non_empty() {
        assert!(j(json!("")) < j(json!("a")));
        assert!(j(json!([])) < j(json!([null])));
        assert!(j(json!({})) < j(json!({"": null})));
    }

    #[test]
    fn test_zero_values() {
        assert_eq!(j(json!(0)), j(json!(0)));
        assert!(j(json!(0)) < j(json!(1)));
        assert!(j(json!(-1)) < j(json!(0)));
    }

    // ===================
    // Hash tests
    // ===================

    #[test]
    fn test_hash_equal_values_same_hash() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_json(j: &Json) -> u64 {
            let mut hasher = DefaultHasher::new();
            j.hash(&mut hasher);
            hasher.finish()
        }

        // Equal values should have equal hashes
        assert_eq!(hash_json(&j(json!(null))), hash_json(&j(json!(null))));
        assert_eq!(hash_json(&j(json!(true))), hash_json(&j(json!(true))));
        assert_eq!(hash_json(&j(json!(42))), hash_json(&j(json!(42))));
        assert_eq!(hash_json(&j(json!("test"))), hash_json(&j(json!("test"))));
        assert_eq!(hash_json(&j(json!([1, 2]))), hash_json(&j(json!([1, 2]))));
        assert_eq!(
            hash_json(&j(json!({"a": 1}))),
            hash_json(&j(json!({"a": 1})))
        );
    }

    #[test]
    fn test_hash_object_key_order_independent() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        fn hash_json(j: &Json) -> u64 {
            let mut hasher = DefaultHasher::new();
            j.hash(&mut hasher);
            hasher.finish()
        }

        // Objects with same keys in different order should have the same hash
        let a = Json::from_str(r#"{"a": 1, "b": 2}"#).unwrap();
        let b = Json::from_str(r#"{"b": 2, "a": 1}"#).unwrap();
        assert_eq!(hash_json(&a), hash_json(&b));
    }

    #[test]
    fn test_hash_in_hashset() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(j(json!(null)));
        set.insert(j(json!(true)));
        set.insert(j(json!(42)));
        set.insert(j(json!("hello")));
        set.insert(j(json!([1, 2, 3])));
        set.insert(j(json!({"key": "value"})));

        assert!(set.contains(&j(json!(null))));
        assert!(set.contains(&j(json!(true))));
        assert!(set.contains(&j(json!(42))));
        assert!(set.contains(&j(json!("hello"))));
        assert!(set.contains(&j(json!([1, 2, 3]))));
        assert!(set.contains(&j(json!({"key": "value"}))));

        // Different values should not be in the set
        assert!(!set.contains(&j(json!(false))));
        assert!(!set.contains(&j(json!(43))));
        assert!(!set.contains(&j(json!("world"))));
    }

    #[test]
    fn test_hash_in_hashmap() {
        use std::collections::HashMap;

        let mut map: HashMap<Json, i32> = HashMap::new();
        map.insert(j(json!({"id": 1})), 100);
        map.insert(j(json!({"id": 2})), 200);

        assert_eq!(map.get(&j(json!({"id": 1}))), Some(&100));
        assert_eq!(map.get(&j(json!({"id": 2}))), Some(&200));
        assert_eq!(map.get(&j(json!({"id": 3}))), None);
    }

    // ===================
    // Default tests
    // ===================

    #[test]
    fn test_default_is_null() {
        let json = Json::default();
        assert_eq!(json, j(json!(null)));
        assert_eq!(json.to_string(), "null");
    }

    // ===================
    // From<Value> tests
    // ===================

    #[test]
    fn test_from_value_null() {
        let json = Json::from(Value::Null);
        assert_eq!(json.to_string(), "null");
    }

    #[test]
    fn test_from_value_bool() {
        let json_true = Json::from(Value::Bool(true));
        let json_false = Json::from(Value::Bool(false));
        assert_eq!(json_true.to_string(), "true");
        assert_eq!(json_false.to_string(), "false");
    }

    #[test]
    fn test_from_value_number() {
        let json_int = Json::from(json!(42));
        let json_float = Json::from(json!(3.14));
        let json_neg = Json::from(json!(-100));
        assert_eq!(json_int.to_string(), "42");
        assert_eq!(json_float.to_string(), "3.14");
        assert_eq!(json_neg.to_string(), "-100");
    }

    #[test]
    fn test_from_value_string() {
        let json = Json::from(json!("hello world"));
        assert_eq!(json.to_string(), "\"hello world\"");
    }

    #[test]
    fn test_from_value_array() {
        let json = Json::from(json!([1, 2, 3]));
        assert_eq!(json.to_string(), "[1,2,3]");
    }

    #[test]
    fn test_from_value_object() {
        let json = Json::from(json!({"key": "value"}));
        assert_eq!(json.to_string(), r#"{"key":"value"}"#);
    }

    // ===================
    // Display tests
    // ===================

    #[test]
    fn test_display_various_types() {
        assert_eq!(format!("{}", j(json!(null))), "null");
        assert_eq!(format!("{}", j(json!(true))), "true");
        assert_eq!(format!("{}", j(json!(42))), "42");
        assert_eq!(format!("{}", j(json!("test"))), "\"test\"");
        assert_eq!(format!("{}", j(json!([1, 2]))), "[1,2]");
        assert_eq!(format!("{}", j(json!({"a": 1}))), r#"{"a":1}"#);
    }

    // ===================
    // FromStr tests
    // ===================

    #[test]
    fn test_from_str_null() {
        let json: Json = "null".parse().unwrap();
        assert_eq!(json, j(json!(null)));
    }

    #[test]
    fn test_from_str_bool() {
        let json_true: Json = "true".parse().unwrap();
        let json_false: Json = "false".parse().unwrap();
        assert_eq!(json_true, j(json!(true)));
        assert_eq!(json_false, j(json!(false)));
    }

    #[test]
    fn test_from_str_number() {
        let json_int: Json = "42".parse().unwrap();
        let json_float: Json = "3.14".parse().unwrap();
        let json_neg: Json = "-100".parse().unwrap();
        assert_eq!(json_int, j(json!(42)));
        assert_eq!(json_float, j(json!(3.14)));
        assert_eq!(json_neg, j(json!(-100)));
    }

    #[test]
    fn test_from_str_string() {
        let json: Json = r#""hello""#.parse().unwrap();
        assert_eq!(json, j(json!("hello")));
    }

    #[test]
    fn test_from_str_array() {
        let json: Json = "[1, 2, 3]".parse().unwrap();
        assert_eq!(json, j(json!([1, 2, 3])));
    }

    #[test]
    fn test_from_str_object() {
        let json: Json = r#"{"key": "value"}"#.parse().unwrap();
        assert_eq!(json, j(json!({"key": "value"})));
    }

    #[test]
    fn test_from_str_complex() {
        let json: Json = r#"{"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}"#
            .parse()
            .unwrap();
        assert_eq!(
            json,
            j(json!({"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}]}))
        );
    }

    #[test]
    fn test_from_str_invalid_json() {
        let result: Result<Json, _> = "not valid json_filter".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_from_str_incomplete_json() {
        let result: Result<Json, _> = r#"{"key":"#.parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_from_str_empty_string() {
        let result: Result<Json, _> = "".parse();
        assert!(result.is_err());
    }

    // ===================
    // Clone tests
    // ===================

    #[test]
    fn test_clone() {
        let original = j(json!({"nested": {"array": [1, 2, 3]}}));
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    // ===================
    // Debug tests
    // ===================

    #[test]
    fn test_debug() {
        let json = j(json!({"key": "value"}));
        let debug_str = format!("{:?}", json);
        assert!(debug_str.contains("Json"));
        assert!(debug_str.contains("value"));
    }

    // ===================
    // Encode/Decode tests
    // ===================

    #[test]
    fn test_encode_decode_null() {
        let original = j(json!(null));
        let encoded = original.encode();
        let decoded = Json::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_bool() {
        let original_true = j(json!(true));
        let original_false = j(json!(false));

        let decoded_true = Json::decode(original_true.encode()).unwrap();
        let decoded_false = Json::decode(original_false.encode()).unwrap();

        assert_eq!(original_true, decoded_true);
        assert_eq!(original_false, decoded_false);
    }

    #[test]
    fn test_encode_decode_number() {
        let test_cases = vec![
            json!(0),
            json!(42),
            json!(-100),
            json!(3.14),
            json!(-2.718),
            json!(i64::MAX),
            json!(i64::MIN),
        ];

        for value in test_cases {
            let original = j(value);
            let encoded = original.encode();
            let decoded = Json::decode(encoded).unwrap();
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn test_encode_decode_string() {
        let test_cases = vec![
            json!(""),
            json!("hello"),
            json!("hello world"),
            json!("special chars: \n\t\"\\"),
            json!("unicode: 日本語 🎉"),
        ];

        for value in test_cases {
            let original = j(value);
            let encoded = original.encode();
            let decoded = Json::decode(encoded).unwrap();
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn test_encode_decode_array() {
        let test_cases = vec![
            json!([]),
            json!([1]),
            json!([1, 2, 3]),
            json!([1, "two", true, null]),
            json!([[1, 2], [3, 4]]),
        ];

        for value in test_cases {
            let original = j(value);
            let encoded = original.encode();
            let decoded = Json::decode(encoded).unwrap();
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn test_encode_decode_object() {
        let test_cases = vec![
            json!({}),
            json!({"a": 1}),
            json!({"a": 1, "b": 2}),
            json!({"nested": {"key": "value"}}),
            json!({"mixed": [1, 2], "bool": true, "null": null}),
        ];

        for value in test_cases {
            let original = j(value);
            let encoded = original.encode();
            let decoded = Json::decode(encoded).unwrap();
            assert_eq!(original, decoded);
        }
    }

    #[test]
    fn test_encode_decode_complex() {
        let original = j(json!({
            "users": [
                {"id": 1, "name": "Alice", "active": true},
                {"id": 2, "name": "Bob", "active": false}
            ],
            "metadata": {
                "total": 2,
                "page": 1
            },
            "tags": ["rust", "json_filter", "dbms"]
        }));

        let encoded = original.encode();
        let decoded = Json::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_decode_error_too_short_empty() {
        use crate::memory::{DecodeError, MemoryError};

        let result = Json::decode(std::borrow::Cow::Borrowed(&[]));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_decode_error_too_short_one_byte() {
        use crate::memory::{DecodeError, MemoryError};

        let result = Json::decode(std::borrow::Cow::Borrowed(&[0x01]));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_decode_error_length_mismatch() {
        use crate::memory::{DecodeError, MemoryError};

        // Says length is 10, but only has 2 bytes of data
        let data = vec![0x0A, 0x00, 0x61, 0x62]; // length=10, but only "ab"
        let result = Json::decode(std::borrow::Cow::Owned(data));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_decode_error_invalid_json() {
        use crate::memory::{DecodeError, MemoryError};

        // Valid UTF-8 but invalid JSON
        let invalid_json = b"not json_filter";
        let len = invalid_json.len() as u16;
        let mut data = len.to_le_bytes().to_vec();
        data.extend_from_slice(invalid_json);

        let result = Json::decode(std::borrow::Cow::Owned(data));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::InvalidJson(_)))
        ));
    }

    #[test]
    fn test_decode_error_invalid_utf8() {
        use crate::memory::{DecodeError, MemoryError};

        // Invalid UTF-8 sequence
        let invalid_utf8 = vec![0xFF, 0xFE];
        let len = invalid_utf8.len() as u16;
        let mut data = len.to_le_bytes().to_vec();
        data.extend_from_slice(&invalid_utf8);

        let result = Json::decode(std::borrow::Cow::Owned(data));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::Utf8Error(_)))
        ));
    }

    // ===================
    // Size tests
    // ===================

    #[test]
    fn test_size_null() {
        let json = j(json!(null));
        // "null" = 4 bytes + 2 bytes for length
        assert_eq!(json.size(), 6);
    }

    #[test]
    fn test_size_bool() {
        let json_true = j(json!(true));
        let json_false = j(json!(false));
        // "true" = 4 bytes + 2, "false" = 5 bytes + 2
        assert_eq!(json_true.size(), 6);
        assert_eq!(json_false.size(), 7);
    }

    #[test]
    fn test_size_string() {
        let json = j(json!("hello"));
        // "\"hello\"" = 7 bytes + 2
        assert_eq!(json.size(), 9);
    }

    #[test]
    fn test_size_object() {
        let json = j(json!({"a": 1}));
        // {"a":1} = 7 bytes + 2
        assert_eq!(json.size(), 9);
    }

    // ===================
    // Encode constants tests
    // ===================

    #[test]
    fn test_encode_size_is_dynamic() {
        use crate::memory::DataSize;
        assert_eq!(Json::SIZE, DataSize::Dynamic);
    }

    #[test]
    fn test_encode_alignment() {
        use crate::memory::DEFAULT_ALIGNMENT;
        assert_eq!(Json::ALIGNMENT, DEFAULT_ALIGNMENT);
    }

    // ===================
    // Serde tests
    // ===================

    #[test]
    fn test_serde_serialize() {
        let json = j(json!({"key": "value"}));
        let serialized = serde_json::to_string(&json).unwrap();
        assert_eq!(serialized, r#""{\"key\":\"value\"}""#);
    }

    // ===================
    // Candid tests
    // ===================

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_null() {
        let original = j(json!(null));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_bool() {
        let original = j(json!(true));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_number() {
        let original = j(json!(42));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_string() {
        let original = j(json!("hello world"));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_array() {
        let original = j(json!([1, 2, 3]));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_object() {
        let original = j(json!({"key": "value"}));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_candid_encode_decode_complex() {
        let original = j(json!({
            "users": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ],
            "count": 2,
            "active": true
        }));
        let buf = candid::encode_one(&original).expect("Candid encoding failed");
        let decoded: Json = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original, decoded);
    }

    // ===================
    // Value conversion tests
    // ===================

    #[test]
    fn test_into_value() {
        use crate::dbms::value::Value as DbmsValue;

        let json = j(json!({"test": 123}));
        let value: DbmsValue = json.clone().into();

        assert!(matches!(value, DbmsValue::Json(_)));
        assert_eq!(value.as_json(), Some(&json));
    }

    #[test]
    fn test_value_type_name() {
        use crate::dbms::value::Value as DbmsValue;

        let json = j(json!(null));
        let value: DbmsValue = json.into();
        assert_eq!(value.type_name(), "Json");
    }
}

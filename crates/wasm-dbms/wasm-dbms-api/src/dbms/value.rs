use std::str::FromStr;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::types;

/// A generic wrapper enum to hold any DBMS value.
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum Value {
    Blob(types::Blob),
    Boolean(types::Boolean),
    Date(types::Date),
    DateTime(types::DateTime),
    Decimal(types::Decimal),
    Int8(types::Int8),
    Int16(types::Int16),
    Int32(types::Int32),
    Int64(types::Int64),
    Json(types::Json),
    Null,
    Text(types::Text),
    Uint8(types::Uint8),
    Uint16(types::Uint16),
    Uint32(types::Uint32),
    Uint64(types::Uint64),
    Uuid(types::Uuid),
    Custom(crate::dbms::custom_value::CustomValue),
}

impl FromStr for Value {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::Text(s.into()))
    }
}

// macro rules for implementing From trait for Value enum variants
macro_rules! impl_conv_for_value {
    ($variant:ident, $ty:ty, $name:ident, $test_name:ident) => {
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Value::$variant(value)
            }
        }

        impl Value {
            /// Attempts to extract a reference to the inner value if it matches the variant.
            pub fn $name(&self) -> Option<&$ty> {
                if let Value::$variant(v) = self {
                    Some(v)
                } else {
                    None
                }
            }
        }

        #[cfg(test)]
        mod $test_name {
            use super::*;

            #[test]
            fn test_value_conversion() {
                let value_instance: $ty = Default::default();
                let value: Value = value_instance.clone().into();
                assert_eq!(value.$name(), Some(&value_instance));
            }
        }
    };
}

macro_rules! value_from_primitive {
    ($variant:ident, $primitive:ty, $test_name:ident) => {
        value_from_primitive!($variant, $primitive, $test_name, Default::default());
    };

    ($variant:ident, $primitive:ty, $test_name:ident, $default_value:expr) => {
        impl From<$primitive> for Value {
            fn from(value: $primitive) -> Self {
                Value::$variant($crate::prelude::$variant(value.into()))
            }
        }

        #[cfg(test)]
        mod $test_name {
            use super::*;

            #[test]
            fn test_value_from_primitive() {
                let primitive_value: $primitive = $default_value;
                if let Value::$variant(inner_value) = Value::from(primitive_value.clone()) {
                    assert_eq!(inner_value.0, primitive_value);
                } else {
                    panic!("Value variant does not match");
                }
            }
        }
    };
}

// implement conversions for all Value variants
impl_conv_for_value!(Blob, types::Blob, as_blob, tests_blob);
impl_conv_for_value!(Boolean, types::Boolean, as_boolean, tests_boolean);
impl_conv_for_value!(Date, types::Date, as_date, tests_date);
impl_conv_for_value!(DateTime, types::DateTime, as_datetime, tests_datetime);
impl_conv_for_value!(Decimal, types::Decimal, as_decimal, tests_decimal);
impl_conv_for_value!(Int8, types::Int8, as_int8, tests_int8);
impl_conv_for_value!(Int16, types::Int16, as_int16, tests_int16);
impl_conv_for_value!(Int32, types::Int32, as_int32, tests_int32);
impl_conv_for_value!(Int64, types::Int64, as_int64, tests_int64);
impl_conv_for_value!(Json, types::Json, as_json, tests_json);
impl_conv_for_value!(Text, types::Text, as_text, tests_text);
impl_conv_for_value!(Uint8, types::Uint8, as_uint8, tests_uint8);
impl_conv_for_value!(Uint16, types::Uint16, as_uint16, tests_uint16);
impl_conv_for_value!(Uint32, types::Uint32, as_uint32, tests_uint32);
impl_conv_for_value!(Uint64, types::Uint64, as_uint64, tests_uint64);
impl_conv_for_value!(Uuid, types::Uuid, as_uuid, tests_uuid);

// from inner values of types
value_from_primitive!(Blob, &[u8], tests_blob_primitive_slice);
value_from_primitive!(Blob, Vec<u8>, tests_blob_primitive);
value_from_primitive!(Boolean, bool, tests_boolean_primitive);
value_from_primitive!(Decimal, rust_decimal::Decimal, tests_decimal_primitive);
value_from_primitive!(Int8, i8, tests_int8_primitive);
value_from_primitive!(Int16, i16, tests_int16_primitive);
value_from_primitive!(Int32, i32, tests_int32_primitive);
value_from_primitive!(Int64, i64, tests_int64_primitive);
value_from_primitive!(Uint8, u8, tests_uint8_primitive);
value_from_primitive!(Uint16, u16, tests_uint16_primitive);
value_from_primitive!(Uint32, u32, tests_uint32_primitive);
value_from_primitive!(Uint64, u64, tests_uint64_primitive);
value_from_primitive!(Text, String, tests_text_primitive_string);
value_from_primitive!(Text, &str, tests_text_primitive_str);
value_from_primitive!(Uuid, uuid::Uuid, tests_uuid_primitive);

impl Value {
    /// Checks if the value is [`Value::Null`].
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns the type name of the value as a string.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Blob(_) => "Blob",
            Value::Boolean(_) => "Boolean",
            Value::Date(_) => "Date",
            Value::DateTime(_) => "DateTime",
            Value::Decimal(_) => "Decimal",
            Value::Int8(_) => "Int8",
            Value::Int16(_) => "Int16",
            Value::Int32(_) => "Int32",
            Value::Int64(_) => "Int64",
            Value::Json(_) => "Json",
            Value::Null => "Null",
            Value::Text(_) => "Text",
            Value::Uint8(_) => "Uint8",
            Value::Uint16(_) => "Uint16",
            Value::Uint32(_) => "Uint32",
            Value::Uint64(_) => "Uint64",
            Value::Uuid(_) => "Uuid",
            Value::Custom(cv) => {
                // Cache custom type names to avoid repeated allocations.
                // The number of unique type tags is bounded at compile time,
                // so the map grows to a fixed size. A maximum of 64 entries
                // is enforced as a safety guard against unbounded memory usage
                // on the IC, where heap is a scarce resource.
                const MAX_CACHE_ENTRIES: usize = 64;
                static CACHE: OnceLock<
                    std::sync::Mutex<std::collections::HashMap<String, &'static str>>,
                > = OnceLock::new();
                let cache =
                    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
                let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
                if map.len() >= MAX_CACHE_ENTRIES && !map.contains_key(&cv.type_tag) {
                    return "Custom(?)";
                }
                map.entry(cv.type_tag.clone()).or_insert_with(|| {
                    let s = format!("Custom({})", cv.type_tag);
                    s.leak()
                })
            }
        }
    }

    /// Returns reference to the inner [`CustomValue`] if this is a `Custom` variant.
    pub fn as_custom(&self) -> Option<&crate::dbms::custom_value::CustomValue> {
        match self {
            Value::Custom(v) => Some(v),
            _ => None,
        }
    }

    /// Attempts to decode a `Custom` variant into a concrete [`CustomDataType`](crate::dbms::types::CustomDataType).
    ///
    /// Returns `None` if this is not a `Custom` variant, the type tag doesn't
    /// match, or decoding fails.
    pub fn as_custom_type<T: crate::dbms::types::CustomDataType>(&self) -> Option<T> {
        self.as_custom()
            .filter(|cv| cv.type_tag == T::TYPE_TAG)
            .and_then(|cv| T::decode(std::borrow::Cow::Borrowed(&cv.encoded)).ok())
    }
}

#[cfg(test)]
mod tests {

    use uuid::Uuid;

    use super::*;

    #[test]
    fn test_null() {
        let int_value: Value = types::Int32(42).into();
        assert!(!int_value.is_null());

        let null_value = Value::Null;
        assert!(null_value.is_null());
    }

    #[test]
    fn test_value_conversion_blob() {
        let blob = types::Blob(vec![1, 2, 3]);
        let value: Value = blob.clone().into();
        assert_eq!(value.as_blob(), Some(&blob));
    }

    #[test]
    fn test_value_conversion_boolean() {
        let boolean = types::Boolean(true);
        let value: Value = boolean.into();
        assert_eq!(value.as_boolean(), Some(&boolean));
    }

    #[test]
    fn test_value_conversion_date() {
        let date = types::Date {
            year: 2023,
            month: 3,
            day: 15,
        }; // Example date
        let value: Value = date.into();
        assert_eq!(value.as_date(), Some(&date));
    }

    #[test]
    fn test_value_conversion_datetime() {
        let datetime = types::DateTime {
            year: 2023,
            month: 3,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone_offset_minutes: 0,
        }; // Example datetime
        let value: Value = datetime.into();
        assert_eq!(value.as_datetime(), Some(&datetime));
    }

    #[test]
    fn test_value_conversion_decimal() {
        let decimal = types::Decimal(rust_decimal::Decimal::new(12345, 2)); // 123.45
        let value: Value = decimal.into();
        assert_eq!(value.as_decimal(), Some(&decimal));
    }

    #[test]
    fn test_value_conversion_int32() {
        let int32 = types::Int32(1234567890);
        let value: Value = int32.into();
        assert_eq!(value.as_int32(), Some(&int32));
    }

    #[test]
    fn test_value_conversion_int64() {
        let int64 = types::Int64(1234567890);
        let value: Value = int64.into();
        assert_eq!(value.as_int64(), Some(&int64));
    }

    #[test]
    fn test_value_conversion_text() {
        let text = types::Text("Hello, World!".to_string());
        let value: Value = text.clone().into();
        assert_eq!(value.as_text(), Some(&text));
    }

    #[test]
    fn test_value_conversion_uint32() {
        let uint32 = types::Uint32(123456);
        let value: Value = uint32.into();
        assert_eq!(value.as_uint32(), Some(&uint32));
    }

    #[test]
    fn test_value_conversion_uint64() {
        let uint64 = types::Uint64(12345678901234);
        let value: Value = uint64.into();
        assert_eq!(value.as_uint64(), Some(&uint64));
    }

    #[test]
    fn test_value_conversion_uuid() {
        let uuid = types::Uuid(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("failed to parse uuid"),
        );
        let value: Value = uuid.clone().into();
        assert_eq!(value.as_uuid(), Some(&uuid));
    }

    #[test]
    fn test_value_type_name() {
        let int_value: Value = types::Int32(42).into();
        assert_eq!(int_value.type_name(), "Int32");

        let text_value: Value = types::Text("Hello".to_string()).into();
        assert_eq!(text_value.type_name(), "Text");

        let null_value = Value::Null;
        assert_eq!(null_value.type_name(), "Null");
    }

    #[test]
    fn test_value_from_str() {
        let str_value = "Hello, DBMS!";

        let value = Value::from_str(str_value).unwrap();
        assert_eq!(value.as_text().unwrap().0, str_value);
    }

    #[test]
    fn test_should_create_custom_value() {
        let cv = crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01],
            display: "Admin".to_string(),
        };
        let value = Value::Custom(cv.clone());
        assert_eq!(value.as_custom(), Some(&cv));
    }

    #[test]
    fn test_should_return_none_for_non_custom() {
        let value = Value::Null;
        assert_eq!(value.as_custom(), None);
    }

    #[test]
    fn test_should_compare_custom_values() {
        let a = Value::Custom(crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01],
            display: "Admin".to_string(),
        });
        let b = Value::Custom(crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01],
            display: "Admin".to_string(),
        });
        assert_eq!(a, b);
    }

    #[test]
    fn test_should_order_custom_after_builtin() {
        let builtin = Value::Uuid(types::Uuid::default());
        let custom = Value::Custom(crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01],
            display: "Admin".to_string(),
        });
        assert!(builtin < custom);
    }

    #[test]
    fn test_should_get_custom_type_name() {
        let cv = Value::Custom(crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01],
            display: "Admin".to_string(),
        });
        assert_eq!(cv.type_name(), "Custom(role)");
    }
}

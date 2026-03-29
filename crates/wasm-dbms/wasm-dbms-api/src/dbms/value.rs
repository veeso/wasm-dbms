mod discriminant;

use std::borrow::Cow;
use std::str::FromStr;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::types;
use crate::memory::{
    DEFAULT_ALIGNMENT, DataSize, DecodeError, Encode, MSize, MemoryError, MemoryResult, PageOffset,
};

/// A generic wrapper enum to hold any DBMS value.
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

/// Encodes a [`Value`] as `[discriminant: u8] + [inner_type.encode()]`.
///
/// For `Null`, only the discriminant byte is written.
/// For `Custom`, the encoding is `[discriminant] + [tag_len: u16 LE] + [tag_bytes] + [data_len: u16 LE] + [encoded_bytes]`.
impl Encode for Value {
    const SIZE: DataSize = DataSize::Dynamic;
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> Cow<'_, [u8]> {
        match self {
            Value::Blob(v) => encode_with_discriminant(discriminant::BLOB, v.encode()),
            Value::Boolean(v) => encode_with_discriminant(discriminant::BOOLEAN, v.encode()),
            Value::Date(v) => encode_with_discriminant(discriminant::DATE, v.encode()),
            Value::DateTime(v) => encode_with_discriminant(discriminant::DATE_TIME, v.encode()),
            Value::Decimal(v) => encode_with_discriminant(discriminant::DECIMAL, v.encode()),
            Value::Int8(v) => encode_with_discriminant(discriminant::INT8, v.encode()),
            Value::Int16(v) => encode_with_discriminant(discriminant::INT16, v.encode()),
            Value::Int32(v) => encode_with_discriminant(discriminant::INT32, v.encode()),
            Value::Int64(v) => encode_with_discriminant(discriminant::INT64, v.encode()),
            Value::Json(v) => encode_with_discriminant(discriminant::JSON, v.encode()),
            Value::Null => Cow::Owned(vec![discriminant::NULL]),
            Value::Text(v) => encode_with_discriminant(discriminant::TEXT, v.encode()),
            Value::Uint8(v) => encode_with_discriminant(discriminant::UINT8, v.encode()),
            Value::Uint16(v) => encode_with_discriminant(discriminant::UINT16, v.encode()),
            Value::Uint32(v) => encode_with_discriminant(discriminant::UINT32, v.encode()),
            Value::Uint64(v) => encode_with_discriminant(discriminant::UINT64, v.encode()),
            Value::Uuid(v) => encode_with_discriminant(discriminant::UUID, v.encode()),
            Value::Custom(cv) => {
                let tag_bytes = cv.type_tag.as_bytes();
                let tag_len = tag_bytes.len() as u16;
                let data_len = cv.encoded.len() as u16;
                let total = 1 + 2 + tag_bytes.len() + 2 + cv.encoded.len();
                let mut buf = Vec::with_capacity(total);
                buf.push(discriminant::CUSTOM);
                buf.extend_from_slice(&tag_len.to_le_bytes());
                buf.extend_from_slice(tag_bytes);
                buf.extend_from_slice(&data_len.to_le_bytes());
                buf.extend_from_slice(&cv.encoded);
                Cow::Owned(buf)
            }
        }
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
        if data.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let disc = data[0];
        let rest = Cow::Owned(data[1..].to_vec());

        match disc {
            discriminant::BLOB => types::Blob::decode(rest).map(Value::Blob),
            discriminant::BOOLEAN => types::Boolean::decode(rest).map(Value::Boolean),
            discriminant::DATE => types::Date::decode(rest).map(Value::Date),
            discriminant::DATE_TIME => types::DateTime::decode(rest).map(Value::DateTime),
            discriminant::DECIMAL => types::Decimal::decode(rest).map(Value::Decimal),
            discriminant::INT8 => types::Int8::decode(rest).map(Value::Int8),
            discriminant::INT16 => types::Int16::decode(rest).map(Value::Int16),
            discriminant::INT32 => types::Int32::decode(rest).map(Value::Int32),
            discriminant::INT64 => types::Int64::decode(rest).map(Value::Int64),
            discriminant::JSON => types::Json::decode(rest).map(Value::Json),
            discriminant::NULL => Ok(Value::Null),
            discriminant::TEXT => types::Text::decode(rest).map(Value::Text),
            discriminant::UINT8 => types::Uint8::decode(rest).map(Value::Uint8),
            discriminant::UINT16 => types::Uint16::decode(rest).map(Value::Uint16),
            discriminant::UINT32 => types::Uint32::decode(rest).map(Value::Uint32),
            discriminant::UINT64 => types::Uint64::decode(rest).map(Value::Uint64),
            discriminant::UUID => types::Uuid::decode(rest).map(Value::Uuid),
            discriminant::CUSTOM => decode_custom_value(&data[1..]),
            other => Err(MemoryError::DecodeError(DecodeError::InvalidDiscriminant(
                other,
            ))),
        }
    }

    fn size(&self) -> MSize {
        1 + match self {
            Value::Blob(v) => Encode::size(v),
            Value::Boolean(v) => Encode::size(v),
            Value::Date(v) => Encode::size(v),
            Value::DateTime(v) => Encode::size(v),
            Value::Decimal(v) => Encode::size(v),
            Value::Int8(v) => Encode::size(v),
            Value::Int16(v) => Encode::size(v),
            Value::Int32(v) => Encode::size(v),
            Value::Int64(v) => Encode::size(v),
            Value::Json(v) => Encode::size(v),
            Value::Null => 0,
            Value::Text(v) => Encode::size(v),
            Value::Uint8(v) => Encode::size(v),
            Value::Uint16(v) => Encode::size(v),
            Value::Uint32(v) => Encode::size(v),
            Value::Uint64(v) => Encode::size(v),
            Value::Uuid(v) => Encode::size(v),
            Value::Custom(cv) => {
                // tag_len(2) + tag_bytes + data_len(2) + encoded_bytes
                (2 + cv.type_tag.len() + 2 + cv.encoded.len()) as MSize
            }
        }
    }
}

/// Prepends the discriminant byte to an already-encoded inner value.
fn encode_with_discriminant(disc: u8, inner: Cow<[u8]>) -> Cow<'static, [u8]> {
    let mut buf = Vec::with_capacity(1 + inner.len());
    buf.push(disc);
    buf.extend_from_slice(&inner);
    Cow::Owned(buf)
}

/// Decodes a [`CustomValue`](crate::dbms::custom_value::CustomValue) from the bytes after the discriminant.
fn decode_custom_value(data: &[u8]) -> MemoryResult<Value> {
    if data.len() < 2 {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let tag_len = u16::from_le_bytes([data[0], data[1]]) as usize;
    if data.len() < 2 + tag_len + 2 {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let type_tag = String::from_utf8(data[2..2 + tag_len].to_vec())?;
    let data_offset = 2 + tag_len;
    let data_len = u16::from_le_bytes([data[data_offset], data[data_offset + 1]]) as usize;
    if data.len() < data_offset + 2 + data_len {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let encoded = data[data_offset + 2..data_offset + 2 + data_len].to_vec();
    Ok(Value::Custom(crate::dbms::custom_value::CustomValue {
        type_tag,
        encoded,
        display: String::new(),
    }))
}

/// Encodes a `Vec<Value>` as `[count: u32 LE] + [for each value: [size: u32 LE] + value.encode()]`.
///
/// This is used as the key type for B-tree indexes, supporting both single-column
/// and composite indexes uniformly.
impl Encode for Vec<Value> {
    const SIZE: DataSize = DataSize::Dynamic;
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> Cow<'_, [u8]> {
        let count = self.len() as u32;
        let mut buf = Vec::new();
        buf.extend_from_slice(&count.to_le_bytes());
        for value in self {
            let encoded = Encode::encode(value);
            let size = encoded.len() as u32;
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&encoded);
        }
        Cow::Owned(buf)
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
        if data.len() < 4 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
        let mut offset = 4;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            if offset + 4 > data.len() {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let size = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;
            if offset + size > data.len() {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let value = Value::decode(Cow::Owned(data[offset..offset + size].to_vec()))?;
            values.push(value);
            offset += size;
        }
        Ok(values)
    }

    fn size(&self) -> MSize {
        let mut total: MSize = 4; // count
        for value in self {
            total += 4 + Encode::size(value); // size prefix + encoded value
        }
        total
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

    // -- Encode round-trip tests for Value --

    #[test]
    fn test_encode_decode_null() {
        let original = Value::Null;
        let encoded = Encode::encode(&original);
        assert_eq!(encoded.len(), 1);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_uint32() {
        let original = Value::Uint32(types::Uint32(42));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_text() {
        let original = Value::Text(types::Text("hello index".to_string()));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_blob() {
        let original = Value::Blob(types::Blob(vec![0xDE, 0xAD, 0xBE, 0xEF]));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_boolean() {
        let original = Value::Boolean(types::Boolean(true));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_date() {
        let original = Value::Date(types::Date {
            year: 2026,
            month: 3,
            day: 29,
        });
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_datetime() {
        let original = Value::DateTime(types::DateTime {
            year: 2026,
            month: 3,
            day: 29,
            hour: 14,
            minute: 30,
            second: 0,
            microsecond: 0,
            timezone_offset_minutes: 60,
        });
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_decimal() {
        let original = Value::Decimal(types::Decimal(rust_decimal::Decimal::new(12345, 2)));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_int8() {
        let original = Value::Int8(types::Int8(-42));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_int16() {
        let original = Value::Int16(types::Int16(-1000));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_int32() {
        let original = Value::Int32(types::Int32(-100_000));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_int64() {
        let original = Value::Int64(types::Int64(-9_000_000_000));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_uint8() {
        let original = Value::Uint8(types::Uint8(255));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_uint16() {
        let original = Value::Uint16(types::Uint16(60_000));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_uint64() {
        let original = Value::Uint64(types::Uint64(18_446_744_073_709_551_615));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_uuid() {
        let original = Value::Uuid(types::Uuid(
            Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
        ));
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_custom() {
        let original = Value::Custom(crate::dbms::custom_value::CustomValue {
            type_tag: "role".to_string(),
            encoded: vec![0x01, 0x02],
            display: "Admin".to_string(),
        });
        let encoded = Encode::encode(&original);
        let decoded = Value::decode(encoded).unwrap();
        // Display is not preserved through encoding
        assert_eq!(decoded.as_custom().unwrap().type_tag, "role");
        assert_eq!(decoded.as_custom().unwrap().encoded, vec![0x01, 0x02]);
    }

    #[test]
    fn test_encode_decode_invalid_discriminant() {
        let data = Cow::Owned(vec![0xFF]);
        let result = Value::decode(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encode_decode_empty_data() {
        let data: Cow<[u8]> = Cow::Owned(vec![]);
        let result = Value::decode(data);
        assert!(result.is_err());
    }

    #[test]
    fn test_value_size_matches_encoded_length() {
        let values = vec![
            Value::Null,
            Value::Uint32(types::Uint32(42)),
            Value::Text(types::Text("test".to_string())),
            Value::Boolean(types::Boolean(false)),
        ];
        for value in &values {
            let encoded = Encode::encode(value);
            assert_eq!(
                Encode::size(value) as usize,
                encoded.len(),
                "size mismatch for {value:?}"
            );
        }
    }

    // -- Encode round-trip tests for Vec<Value> --

    #[test]
    fn test_encode_decode_vec_single_value() {
        let original = vec![Value::Uint32(types::Uint32(99))];
        let encoded = Encode::encode(&original);
        let decoded = Vec::<Value>::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_vec_composite() {
        let original = vec![
            Value::Text(types::Text("alice".to_string())),
            Value::Uint32(types::Uint32(30)),
        ];
        let encoded = Encode::encode(&original);
        let decoded = Vec::<Value>::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_encode_decode_vec_empty() {
        let original: Vec<Value> = vec![];
        let encoded = Encode::encode(&original);
        let decoded = Vec::<Value>::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_vec_value_size_matches_encoded_length() {
        let original = vec![
            Value::Text(types::Text("hello".to_string())),
            Value::Null,
            Value::Int64(types::Int64(-1)),
        ];
        let encoded = Encode::encode(&original);
        assert_eq!(Encode::size(&original) as usize, encoded.len());
    }
}

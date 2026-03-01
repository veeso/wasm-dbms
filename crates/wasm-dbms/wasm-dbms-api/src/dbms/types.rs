//! This module exposes the data types used in the DBMS.

use serde::{Deserialize, Serialize};

use crate::dbms::value::Value;
use crate::memory::Encode;

mod blob;
mod boolean;
mod date;
mod datetime;
mod decimal;
mod integers;
mod json;
mod nullable;
mod text;
mod uuid;

pub use self::blob::Blob;
pub use self::boolean::Boolean;
pub use self::date::Date;
pub use self::datetime::DateTime;
pub use self::decimal::Decimal;
pub use self::integers::{Int8, Int16, Int32, Int64, Uint8, Uint16, Uint32, Uint64};
pub use self::json::Json;
pub use self::nullable::Nullable;
pub use self::text::Text;
pub use self::uuid::Uuid;

/// A trait representing a data type that can be stored in the DBMS.
///
/// This is an umbrella trait that combines several other traits to ensure that
/// any type implementing [`DataType`] can be cloned, compared, hashed, encoded,
/// and serialized/deserialized using Serde.
///
/// Also it is used by the DBMS to compare and sort values of different data types.
pub trait DataType:
    Clone
    + std::fmt::Debug
    + std::fmt::Display
    + PartialEq
    + Eq
    + Default
    + PartialOrd
    + Ord
    + std::hash::Hash
    + Encode
    + Serialize
    + Into<Value>
    + for<'de> Deserialize<'de>
{
}

/// A trait for user-defined custom data types.
///
/// Custom types are stored as type-erased [`CustomValue`](crate::dbms::custom_value::CustomValue)
/// inside `Value::Custom`. The `TYPE_TAG` constant uniquely identifies the type
/// and must be stable across versions.
///
/// # Ordering contract
///
/// For custom types used with range filters (`Gt`, `Lt`, `Ge`, `Le`) or `ORDER BY`,
/// the [`Encode`](crate::memory::Encode) output must be order-preserving: if `a < b`,
/// then `a.encode() < b.encode()` lexicographically.
/// Equality filters (`Eq`, `Ne`, `In`) only require canonical encoding.
pub trait CustomDataType: DataType {
    /// Unique string identifier for this type (e.g., `"principal"`, `"role"`).
    const TYPE_TAG: &'static str;
}

/// An enumeration of all supported data type kinds in the DBMS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataTypeKind {
    Blob,
    Boolean,
    Date,
    DateTime,
    Decimal,
    Int32,
    Int64,
    Json,
    Text,
    Uint32,
    Uint64,
    Uuid,
    Custom(&'static str),
}

#[cfg(test)]
mod test {

    use std::collections::HashSet;

    use super::*;

    #[test]
    fn test_should_create_all_data_type_kind_variants() {
        let kinds = [
            DataTypeKind::Blob,
            DataTypeKind::Boolean,
            DataTypeKind::Date,
            DataTypeKind::DateTime,
            DataTypeKind::Decimal,
            DataTypeKind::Int32,
            DataTypeKind::Int64,
            DataTypeKind::Json,
            DataTypeKind::Text,
            DataTypeKind::Uint32,
            DataTypeKind::Uint64,
            DataTypeKind::Uuid,
        ];

        assert_eq!(kinds.len(), 12);
    }

    #[test]
    #[allow(clippy::clone_on_copy)]
    fn test_should_clone_data_type_kind() {
        let kind = DataTypeKind::Text;
        let cloned = kind.clone();
        assert_eq!(kind, cloned);
    }

    #[test]
    fn test_should_copy_data_type_kind() {
        let kind = DataTypeKind::Uint32;
        let copied = kind;
        assert_eq!(kind, copied);
    }

    #[test]
    fn test_should_compare_data_type_kinds() {
        assert_eq!(DataTypeKind::Blob, DataTypeKind::Blob);
        assert_eq!(DataTypeKind::Boolean, DataTypeKind::Boolean);
        assert_ne!(DataTypeKind::Blob, DataTypeKind::Boolean);
        assert_ne!(DataTypeKind::Int32, DataTypeKind::Int64);
        assert_ne!(DataTypeKind::Uint32, DataTypeKind::Uint64);
    }

    #[test]
    fn test_should_hash_data_type_kind() {
        let mut set = HashSet::new();
        set.insert(DataTypeKind::Text);
        set.insert(DataTypeKind::Uint32);
        set.insert(DataTypeKind::Boolean);

        assert!(set.contains(&DataTypeKind::Text));
        assert!(set.contains(&DataTypeKind::Uint32));
        assert!(set.contains(&DataTypeKind::Boolean));
        assert!(!set.contains(&DataTypeKind::Blob));
    }

    #[test]
    fn test_should_debug_data_type_kind() {
        assert_eq!(format!("{:?}", DataTypeKind::Blob), "Blob");
        assert_eq!(format!("{:?}", DataTypeKind::Boolean), "Boolean");
        assert_eq!(format!("{:?}", DataTypeKind::Date), "Date");
        assert_eq!(format!("{:?}", DataTypeKind::DateTime), "DateTime");
        assert_eq!(format!("{:?}", DataTypeKind::Decimal), "Decimal");
        assert_eq!(format!("{:?}", DataTypeKind::Int32), "Int32");
        assert_eq!(format!("{:?}", DataTypeKind::Int64), "Int64");
        assert_eq!(format!("{:?}", DataTypeKind::Json), "Json");
        assert_eq!(format!("{:?}", DataTypeKind::Text), "Text");
        assert_eq!(format!("{:?}", DataTypeKind::Uint32), "Uint32");
        assert_eq!(format!("{:?}", DataTypeKind::Uint64), "Uint64");
        assert_eq!(format!("{:?}", DataTypeKind::Uuid), "Uuid");
    }

    #[test]
    fn test_should_use_data_type_kind_as_hashmap_key() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        map.insert(DataTypeKind::Text, "String type");
        map.insert(DataTypeKind::Uint32, "32-bit unsigned integer");

        assert_eq!(map.get(&DataTypeKind::Text), Some(&"String type"));
        assert_eq!(
            map.get(&DataTypeKind::Uint32),
            Some(&"32-bit unsigned integer")
        );
        assert_eq!(map.get(&DataTypeKind::Blob), None);
    }

    #[test]
    fn test_should_create_custom_data_type_kind() {
        let kind = DataTypeKind::Custom("role");
        assert_eq!(kind, DataTypeKind::Custom("role"));
        assert_ne!(kind, DataTypeKind::Custom("status"));
        assert_ne!(kind, DataTypeKind::Text);
    }

    #[test]
    fn test_should_copy_custom_data_type_kind() {
        let kind = DataTypeKind::Custom("role");
        let copied = kind;
        assert_eq!(kind, copied);
    }

    #[test]
    fn test_should_debug_custom_data_type_kind() {
        let kind = DataTypeKind::Custom("role");
        let debug = format!("{kind:?}");
        assert!(debug.contains("Custom"));
        assert!(debug.contains("role"));
    }
}

use std::collections::HashMap;

use wasm_dbms_api::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryError, PageOffset};
use wasm_dbms_api::prelude::{MemoryResult, Value};

/// Mapping between the column name and the current autoincrement value for that column.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AutoincrementRegistry(HashMap<String, Value>);

impl AutoincrementRegistry {
    /// Gets the next autoincrement value for the given column, and updates the registry with the new value.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::AutoincrementOverflow`] if the current value is already at the
    /// maximum for its integer type.
    ///
    /// # Panics
    ///
    /// Panics if `column` has not been initialized in the registry.
    pub fn next(&mut self, column: &str) -> MemoryResult<Value> {
        let current_value = self.0.entry(column.to_string()).or_insert_with(|| {
            panic!("column '{column}' does not have an autoincrement value in the registry")
        });
        let next_value = Self::next_value(current_value.clone())?;
        *current_value = next_value.clone();
        Ok(next_value)
    }

    /// Initializes the autoincrement value for the given column in the registry.
    ///
    /// Zero value should be provided, as the registry does not know the type of the column,
    /// and thus cannot determine the appropriate zero value.
    pub(crate) fn init(&mut self, column: &str, zero: Value) {
        self.0.insert(column.to_string(), zero);
    }

    /// Computes the next autoincrement value by incrementing the current value by one.
    ///
    /// Returns an error if the value is already at the maximum for its type.
    fn next_value(value: Value) -> MemoryResult<Value> {
        match value {
            Value::Int8(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Int8(v.into()))
                .ok_or_else(|| Self::overflow_error("Int8")),
            Value::Int16(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Int16(v.into()))
                .ok_or_else(|| Self::overflow_error("Int16")),
            Value::Int32(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Int32(v.into()))
                .ok_or_else(|| Self::overflow_error("Int32")),
            Value::Int64(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Int64(v.into()))
                .ok_or_else(|| Self::overflow_error("Int64")),
            Value::Uint8(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Uint8(v.into()))
                .ok_or_else(|| Self::overflow_error("Uint8")),
            Value::Uint16(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Uint16(v.into()))
                .ok_or_else(|| Self::overflow_error("Uint16")),
            Value::Uint32(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Uint32(v.into()))
                .ok_or_else(|| Self::overflow_error("Uint32")),
            Value::Uint64(val) => val
                .0
                .checked_add(1)
                .map(|v| Value::Uint64(v.into()))
                .ok_or_else(|| Self::overflow_error("Uint64")),
            value => panic!("unsupported autoincrement type: {value:?}"),
        }
    }

    /// Builds a [`MemoryError::AutoincrementOverflow`] for the given type name.
    fn overflow_error(type_name: &str) -> MemoryError {
        MemoryError::AutoincrementOverflow(type_name.to_string())
    }
}

impl Encode for AutoincrementRegistry {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        // push the number of entries in the registry
        bytes.push(self.0.len() as u8);
        for (col, v) in &self.0 {
            bytes.push(col.len() as u8);
            bytes.extend_from_slice(col.as_bytes());
            bytes.extend_from_slice(&v.encode());
        }
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let mut offset = 0;
        let num_entries = data[offset] as usize;
        offset += 1;
        let mut registry = HashMap::with_capacity(num_entries);

        for _ in 0..num_entries {
            let col_len = data[offset] as usize;
            offset += 1;
            let col = String::from_utf8(data[offset..offset + col_len].to_vec())?;
            offset += col_len;
            let v = Value::decode(std::borrow::Cow::Borrowed(&data[offset..]))?;
            offset += v.size() as usize;
            registry.insert(col, v);
        }

        Ok(Self(registry))
    }

    fn size(&self) -> MSize {
        1 + self
            .0
            .iter()
            .fold(0, |acc, (col, v)| acc + 1 + col.len() as MSize + v.size())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_autoincrement_registry_encode_decode() {
        let mut registry = AutoincrementRegistry(HashMap::new());
        registry.0.insert("id".to_string(), 42i32.into());
        registry.0.insert("age".to_string(), 30i32.into());

        let encoded = registry.encode();
        let decoded = AutoincrementRegistry::decode(encoded).expect("failed to decode");

        assert_eq!(registry, decoded);
    }

    #[test]
    fn test_should_get_next_value() {
        assert_eq!(
            AutoincrementRegistry::next_value(42i8.into()).unwrap(),
            43i8.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(30i16.into()).unwrap(),
            31i16.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(100i32.into()).unwrap(),
            101i32.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(1_000_000_000i64.into()).unwrap(),
            1_000_000_001i64.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(0u8.into()).unwrap(),
            1u8.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(100u16.into()).unwrap(),
            101u16.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(1_000u32.into()).unwrap(),
            1_001u32.into()
        );
        assert_eq!(
            AutoincrementRegistry::next_value(1_000_000_000u64.into()).unwrap(),
            1_000_000_001u64.into()
        );
    }

    #[test]
    fn test_should_error_on_overflow_unsigned() {
        assert!(AutoincrementRegistry::next_value(u8::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(u16::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(u32::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(u64::MAX.into()).is_err());
    }

    #[test]
    fn test_should_error_on_overflow_signed() {
        assert!(AutoincrementRegistry::next_value(i8::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(i16::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(i32::MAX.into()).is_err());
        assert!(AutoincrementRegistry::next_value(i64::MAX.into()).is_err());
    }

    #[test]
    fn test_next_increments_and_updates_registry() {
        let mut registry = AutoincrementRegistry::default();
        registry.init("id", Value::Uint32(0u32.into()));

        let first = registry.next("id").expect("first next failed");
        assert_eq!(first, Value::Uint32(1u32.into()));

        let second = registry.next("id").expect("second next failed");
        assert_eq!(second, Value::Uint32(2u32.into()));

        let third = registry.next("id").expect("third next failed");
        assert_eq!(third, Value::Uint32(3u32.into()));
    }

    #[test]
    #[should_panic(expected = "does not have an autoincrement value")]
    fn test_next_panics_on_uninitialized_column() {
        let mut registry = AutoincrementRegistry::default();
        let _ = registry.next("missing");
    }

    #[test]
    fn test_init_sets_zero_value() {
        let mut registry = AutoincrementRegistry::default();
        registry.init("counter", Value::Int64(0i64.into()));

        let value = registry.next("counter").expect("next failed");
        assert_eq!(value, Value::Int64(1i64.into()));
    }

    #[test]
    fn test_multiple_columns_independent() {
        let mut registry = AutoincrementRegistry::default();
        registry.init("id", Value::Uint32(0u32.into()));
        registry.init("seq", Value::Uint64(0u64.into()));

        let id1 = registry.next("id").expect("id next failed");
        let id2 = registry.next("id").expect("id next failed");
        let seq1 = registry.next("seq").expect("seq next failed");

        assert_eq!(id1, Value::Uint32(1u32.into()));
        assert_eq!(id2, Value::Uint32(2u32.into()));
        assert_eq!(seq1, Value::Uint64(1u64.into()));
    }

    #[test]
    fn test_encode_decode_after_increments() {
        let mut registry = AutoincrementRegistry::default();
        registry.init("id", Value::Uint32(0u32.into()));
        let _ = registry.next("id").unwrap();
        let _ = registry.next("id").unwrap();

        let encoded = registry.encode();
        let decoded = AutoincrementRegistry::decode(encoded).expect("failed to decode");
        assert_eq!(registry, decoded);

        // decoded registry should continue from 2
        let mut decoded = decoded;
        let value = decoded.next("id").expect("next after decode failed");
        assert_eq!(value, Value::Uint32(3u32.into()));
    }

    #[test]
    fn test_encode_decode_empty_registry() {
        let registry = AutoincrementRegistry::default();
        let encoded = registry.encode();
        let decoded = AutoincrementRegistry::decode(encoded).expect("failed to decode");
        assert_eq!(registry, decoded);
    }

    #[test]
    fn test_size_calculation() {
        let mut registry = AutoincrementRegistry::default();
        assert_eq!(registry.size(), 1); // just the count byte

        registry.init("id", Value::Uint32(0u32.into()));
        // 1 (count) + 1 (col_len) + 2 (col "id") + value size
        let expected = 1 + 1 + 2 + Value::Uint32(0u32.into()).size();
        assert_eq!(registry.size(), expected);
    }
}

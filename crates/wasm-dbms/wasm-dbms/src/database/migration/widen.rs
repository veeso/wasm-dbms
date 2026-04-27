// Rust guideline compliant 2026-04-27

//! Compatible-widening whitelist for `MigrationOp::WidenColumn`.
//!
//! Mirrors the table from the schema-migrations design:
//!
//! | From → To              | Semantics       |
//! |------------------------|-----------------|
//! | `IntN → IntM`, M > N   | sign-extend     |
//! | `UintN → UintM`, M > N | zero-extend     |
//! | `UintN → IntM`, M > N  | zero-extend     |
//! | `Float32 → Float64`    | widen           |
//!
//! Anything else returns [`MigrationError::WideningIncompatible`].

use wasm_dbms_api::prelude::{
    DataTypeSnapshot, DbmsError, DbmsResult, Int16, Int32, Int64, MigrationError, Uint16, Uint32,
    Uint64, Value,
};

/// Apply a single-column widening conversion. `Value::Null` is preserved
/// regardless of the type pair (a nullable column keeps its null after
/// widening).
pub(crate) fn widen_value(
    table: &str,
    column: &str,
    old_type: &DataTypeSnapshot,
    new_type: &DataTypeSnapshot,
    value: Value,
) -> DbmsResult<Value> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    use DataTypeSnapshot as D;
    let widened = match (old_type, new_type, value) {
        (D::Int8, D::Int16, Value::Int8(v)) => Value::Int16(Int16(v.0 as i16)),
        (D::Int8, D::Int32, Value::Int8(v)) => Value::Int32(Int32(v.0 as i32)),
        (D::Int8, D::Int64, Value::Int8(v)) => Value::Int64(Int64(v.0 as i64)),
        (D::Int16, D::Int32, Value::Int16(v)) => Value::Int32(Int32(v.0 as i32)),
        (D::Int16, D::Int64, Value::Int16(v)) => Value::Int64(Int64(v.0 as i64)),
        (D::Int32, D::Int64, Value::Int32(v)) => Value::Int64(Int64(v.0 as i64)),
        (D::Uint8, D::Uint16, Value::Uint8(v)) => Value::Uint16(Uint16(v.0 as u16)),
        (D::Uint8, D::Uint32, Value::Uint8(v)) => Value::Uint32(Uint32(v.0 as u32)),
        (D::Uint8, D::Uint64, Value::Uint8(v)) => Value::Uint64(Uint64(v.0 as u64)),
        (D::Uint16, D::Uint32, Value::Uint16(v)) => Value::Uint32(Uint32(v.0 as u32)),
        (D::Uint16, D::Uint64, Value::Uint16(v)) => Value::Uint64(Uint64(v.0 as u64)),
        (D::Uint32, D::Uint64, Value::Uint32(v)) => Value::Uint64(Uint64(v.0 as u64)),
        (D::Uint8, D::Int16, Value::Uint8(v)) => Value::Int16(Int16(v.0 as i16)),
        (D::Uint8, D::Int32, Value::Uint8(v)) => Value::Int32(Int32(v.0 as i32)),
        (D::Uint8, D::Int64, Value::Uint8(v)) => Value::Int64(Int64(v.0 as i64)),
        (D::Uint16, D::Int32, Value::Uint16(v)) => Value::Int32(Int32(v.0 as i32)),
        (D::Uint16, D::Int64, Value::Uint16(v)) => Value::Int64(Int64(v.0 as i64)),
        (D::Uint32, D::Int64, Value::Uint32(v)) => Value::Int64(Int64(v.0 as i64)),
        (old, new, _) => {
            return Err(DbmsError::Migration(MigrationError::WideningIncompatible {
                table: table.to_string(),
                column: column.to_string(),
                old_type: old.clone(),
                new_type: new.clone(),
            }));
        }
    };
    Ok(widened)
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::Int32;

    use super::*;

    #[test]
    fn test_widen_uint32_to_uint64_zero_extends() {
        let v = widen_value(
            "t",
            "c",
            &DataTypeSnapshot::Uint32,
            &DataTypeSnapshot::Uint64,
            Value::Uint32(Uint32(7)),
        )
        .unwrap();
        assert_eq!(v, Value::Uint64(Uint64(7)));
    }

    #[test]
    fn test_widen_int16_to_int64_sign_extends_negative() {
        let v = widen_value(
            "t",
            "c",
            &DataTypeSnapshot::Int16,
            &DataTypeSnapshot::Int64,
            Value::Int16(Int16(-3)),
        )
        .unwrap();
        assert_eq!(v, Value::Int64(Int64(-3)));
    }

    #[test]
    fn test_widen_uint16_to_int32_zero_extends_into_signed() {
        let v = widen_value(
            "t",
            "c",
            &DataTypeSnapshot::Uint16,
            &DataTypeSnapshot::Int32,
            Value::Uint16(Uint16(40_000)),
        )
        .unwrap();
        assert_eq!(v, Value::Int32(Int32(40_000)));
    }

    #[test]
    fn test_widen_null_preserved() {
        let v = widen_value(
            "t",
            "c",
            &DataTypeSnapshot::Int32,
            &DataTypeSnapshot::Int64,
            Value::Null,
        )
        .unwrap();
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn test_widen_int32_to_uint8_returns_incompatible() {
        let err = widen_value(
            "t",
            "c",
            &DataTypeSnapshot::Int32,
            &DataTypeSnapshot::Uint8,
            Value::Int32(Int32(7)),
        )
        .unwrap_err();
        assert!(matches!(
            err,
            DbmsError::Migration(MigrationError::WideningIncompatible { .. })
        ));
    }
}

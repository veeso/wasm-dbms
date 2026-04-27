// Rust guideline compliant 2026-04-27

//! Snapshot-driven record (de)serializer.
//!
//! Walks a [`TableSchemaSnapshot`] column list and dispatches per
//! [`DataTypeSnapshot`] variant to slice raw record bytes into typed
//! [`Value`]s — independent of the compile-time `T: TableSchema` type.
//!
//! Used by the migration apply pipeline to read records under the **stored**
//! snapshot and re-encode them under a target snapshot. Custom-typed columns
//! produce [`CustomValue`] with an empty `display`; consumers
//! (`Migrate::transform_column`, the symmetric encode) only depend on
//! `type_tag` + `encoded` and never read `display`.

use wasm_dbms_api::prelude::{
    Blob, Boolean, CustomValue, DataTypeSnapshot, Date, DateTime, Decimal, DecodeError, Encode,
    Int8, Int16, Int32, Int64, Json, MemoryError, MemoryResult, TableSchemaSnapshot, Text, Uint8,
    Uint16, Uint32, Uint64, Uuid, Value, WireSize,
};

/// Decode raw record bytes under the given stored snapshot into a
/// column-keyed value list.
///
/// # Errors
///
/// Returns [`MemoryError::DecodeError`] on truncated or shape-mismatched
/// bytes (`TooShort`, `IdentityDecodeError` for unsupported variants).
pub(crate) fn decode_record_by_snapshot(
    bytes: &[u8],
    snapshot: &TableSchemaSnapshot,
) -> MemoryResult<Vec<(String, Value)>> {
    let mut offset = 0usize;
    let mut out = Vec::with_capacity(snapshot.columns.len());
    for col in &snapshot.columns {
        if offset > bytes.len() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let (value, consumed) = decode_column(&col.data_type, col.nullable, &bytes[offset..])?;
        out.push((col.name.clone(), value));
        offset += consumed;
    }
    Ok(out)
}

/// Encode `values` as raw record bytes under the target snapshot.
///
/// `values` must match `snapshot.columns` in length and order; the caller
/// (typically the migration apply pipeline) is responsible for projecting
/// the row before invoking this.
///
/// # Errors
///
/// Returns [`MemoryError::DecodeError(IdentityDecodeError)`] on shape
/// mismatch or `Value`/`DataTypeSnapshot` discriminant mismatch.
pub(crate) fn encode_record_by_snapshot(
    values: &[(String, Value)],
    snapshot: &TableSchemaSnapshot,
) -> MemoryResult<Vec<u8>> {
    if values.len() != snapshot.columns.len() {
        return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
            format!(
                "value count ({}) does not match column count ({})",
                values.len(),
                snapshot.columns.len(),
            ),
        )));
    }
    let mut out = Vec::new();
    for (col, (name, value)) in snapshot.columns.iter().zip(values.iter()) {
        if &col.name != name {
            return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("value name `{}` does not match column `{}`", name, col.name,),
            )));
        }
        encode_column(&col.data_type, col.nullable, value, &mut out)?;
    }
    Ok(out)
}

fn decode_column(
    dt: &DataTypeSnapshot,
    nullable: bool,
    bytes: &[u8],
) -> MemoryResult<(Value, usize)> {
    if nullable {
        if bytes.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        match bytes[0] {
            0 => Ok((Value::Null, 1)),
            1 => {
                let (value, consumed) = decode_non_nullable(dt, &bytes[1..])?;
                Ok((value, 1 + consumed))
            }
            v => Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Invalid nullable flag: {v:#x}"),
            ))),
        }
    } else {
        decode_non_nullable(dt, bytes)
    }
}

fn decode_non_nullable(dt: &DataTypeSnapshot, bytes: &[u8]) -> MemoryResult<(Value, usize)> {
    match dt {
        DataTypeSnapshot::Int8 => decode_fixed::<Int8>(bytes, 1).map(|(v, n)| (Value::Int8(v), n)),
        DataTypeSnapshot::Uint8 => {
            decode_fixed::<Uint8>(bytes, 1).map(|(v, n)| (Value::Uint8(v), n))
        }
        DataTypeSnapshot::Boolean => {
            decode_fixed::<Boolean>(bytes, 1).map(|(v, n)| (Value::Boolean(v), n))
        }
        DataTypeSnapshot::Int16 => {
            decode_fixed::<Int16>(bytes, 2).map(|(v, n)| (Value::Int16(v), n))
        }
        DataTypeSnapshot::Uint16 => {
            decode_fixed::<Uint16>(bytes, 2).map(|(v, n)| (Value::Uint16(v), n))
        }
        DataTypeSnapshot::Int32 => {
            decode_fixed::<Int32>(bytes, 4).map(|(v, n)| (Value::Int32(v), n))
        }
        DataTypeSnapshot::Uint32 => {
            decode_fixed::<Uint32>(bytes, 4).map(|(v, n)| (Value::Uint32(v), n))
        }
        DataTypeSnapshot::Date => decode_fixed::<Date>(bytes, 4).map(|(v, n)| (Value::Date(v), n)),
        DataTypeSnapshot::Int64 => {
            decode_fixed::<Int64>(bytes, 8).map(|(v, n)| (Value::Int64(v), n))
        }
        DataTypeSnapshot::Uint64 => {
            decode_fixed::<Uint64>(bytes, 8).map(|(v, n)| (Value::Uint64(v), n))
        }
        DataTypeSnapshot::Datetime => {
            decode_fixed::<DateTime>(bytes, 13).map(|(v, n)| (Value::DateTime(v), n))
        }
        DataTypeSnapshot::Decimal => {
            decode_fixed::<Decimal>(bytes, 16).map(|(v, n)| (Value::Decimal(v), n))
        }
        DataTypeSnapshot::Uuid => decode_fixed::<Uuid>(bytes, 16).map(|(v, n)| (Value::Uuid(v), n)),
        DataTypeSnapshot::Text => {
            decode_length_prefixed::<Text>(bytes).map(|(v, n)| (Value::Text(v), n))
        }
        DataTypeSnapshot::Blob => {
            decode_length_prefixed::<Blob>(bytes).map(|(v, n)| (Value::Blob(v), n))
        }
        DataTypeSnapshot::Json => {
            decode_length_prefixed::<Json>(bytes).map(|(v, n)| (Value::Json(v), n))
        }
        DataTypeSnapshot::Custom(meta) => {
            let (slice, consumed) = match meta.wire_size {
                WireSize::Fixed(n) => {
                    let n = n as usize;
                    if bytes.len() < n {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }
                    (bytes[..n].to_vec(), n)
                }
                WireSize::LengthPrefixed => {
                    if bytes.len() < 2 {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }
                    let len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
                    let total = 2 + len;
                    if bytes.len() < total {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }
                    (bytes[..total].to_vec(), total)
                }
            };
            let value = Value::Custom(CustomValue {
                type_tag: meta.tag.clone(),
                encoded: slice,
                display: String::new(),
            });
            Ok((value, consumed))
        }
        // `Float32`/`Float64` exist in `DataTypeSnapshot` but no compile-time
        // `Value::FloatXX` variants, so a compiled schema cannot produce them.
        // If a stored snapshot somehow carries one, fail loud rather than
        // silently misdecoding bytes.
        DataTypeSnapshot::Float32 | DataTypeSnapshot::Float64 => {
            Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Float types are not yet wired into the snapshot codec: {dt:?}"),
            )))
        }
    }
}

fn decode_fixed<T>(bytes: &[u8], size: usize) -> MemoryResult<(T, usize)>
where
    T: Encode,
{
    if bytes.len() < size {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let value = T::decode(std::borrow::Cow::Borrowed(&bytes[..size]))?;
    Ok((value, size))
}

fn decode_length_prefixed<T>(bytes: &[u8]) -> MemoryResult<(T, usize)>
where
    T: Encode,
{
    if bytes.len() < 2 {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
    let total = 2 + len;
    if bytes.len() < total {
        return Err(MemoryError::DecodeError(DecodeError::TooShort));
    }
    let value = T::decode(std::borrow::Cow::Borrowed(&bytes[..total]))?;
    Ok((value, total))
}

fn encode_column(
    dt: &DataTypeSnapshot,
    nullable: bool,
    value: &Value,
    out: &mut Vec<u8>,
) -> MemoryResult<()> {
    if nullable {
        match value {
            Value::Null => {
                out.push(0);
                return Ok(());
            }
            _ => out.push(1),
        }
    } else if matches!(value, Value::Null) {
        return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
            "Value::Null in non-nullable column".to_string(),
        )));
    }
    encode_non_nullable(dt, value, out)
}

fn encode_non_nullable(
    dt: &DataTypeSnapshot,
    value: &Value,
    out: &mut Vec<u8>,
) -> MemoryResult<()> {
    match (dt, value) {
        (DataTypeSnapshot::Int8, Value::Int8(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Uint8, Value::Uint8(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Boolean, Value::Boolean(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Int16, Value::Int16(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Uint16, Value::Uint16(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Int32, Value::Int32(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Uint32, Value::Uint32(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Date, Value::Date(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Int64, Value::Int64(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Uint64, Value::Uint64(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Datetime, Value::DateTime(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Decimal, Value::Decimal(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Uuid, Value::Uuid(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Text, Value::Text(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Blob, Value::Blob(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Json, Value::Json(v)) => out.extend_from_slice(&v.encode()),
        (DataTypeSnapshot::Custom(_), Value::Custom(cv)) => {
            out.extend_from_slice(&cv.encoded);
        }
        (DataTypeSnapshot::Float32 | DataTypeSnapshot::Float64, _) => {
            return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Float types are not yet wired into the snapshot codec: {dt:?}"),
            )));
        }
        (dt, value) => {
            return Err(MemoryError::DecodeError(DecodeError::IdentityDecodeError(
                format!("Value variant does not match data type: {dt:?} vs {value:?}"),
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnSnapshot, CustomDataTypeSnapshot, DataTypeSnapshot, Int64, TableSchemaSnapshot, Text,
        Uint32, Value, WireSize,
    };

    use super::*;

    fn snap_with(columns: Vec<ColumnSnapshot>) -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "t".into(),
            primary_key: "id".into(),
            alignment: 32,
            columns,
            indexes: vec![],
        }
    }

    fn col(name: &str, dt: DataTypeSnapshot, nullable: bool) -> ColumnSnapshot {
        ColumnSnapshot {
            name: name.into(),
            data_type: dt,
            nullable,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        }
    }

    #[test]
    fn test_round_trip_uint32_text() {
        let snap = snap_with(vec![
            col("id", DataTypeSnapshot::Uint32, false),
            col("name", DataTypeSnapshot::Text, false),
        ]);
        let values: Vec<(String, Value)> = vec![
            ("id".into(), Value::Uint32(Uint32(7))),
            ("name".into(), Value::Text(Text("alice".into()))),
        ];
        let bytes = encode_record_by_snapshot(&values, &snap).unwrap();
        let decoded = decode_record_by_snapshot(&bytes, &snap).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn test_round_trip_nullable_null_and_value() {
        let snap = snap_with(vec![
            col("a", DataTypeSnapshot::Int64, true),
            col("b", DataTypeSnapshot::Int64, true),
        ]);
        let values = vec![
            ("a".into(), Value::Null),
            ("b".into(), Value::Int64(Int64(-3))),
        ];
        let bytes = encode_record_by_snapshot(&values, &snap).unwrap();
        let decoded = decode_record_by_snapshot(&bytes, &snap).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn test_round_trip_custom_fixed() {
        let snap = snap_with(vec![col(
            "status",
            DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: "TestStatus".into(),
                wire_size: WireSize::Fixed(1),
            })),
            false,
        )]);
        let values = vec![(
            "status".into(),
            Value::Custom(CustomValue {
                type_tag: "TestStatus".into(),
                encoded: vec![0x01],
                display: String::new(),
            }),
        )];
        let bytes = encode_record_by_snapshot(&values, &snap).unwrap();
        let decoded = decode_record_by_snapshot(&bytes, &snap).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn test_round_trip_custom_length_prefixed() {
        let snap = snap_with(vec![col(
            "blob",
            DataTypeSnapshot::Custom(Box::new(CustomDataTypeSnapshot {
                tag: "MyBlob".into(),
                wire_size: WireSize::LengthPrefixed,
            })),
            false,
        )]);
        // user encoder convention: 2-byte LE prefix + body.
        let body = b"hello";
        let mut encoded = (body.len() as u16).to_le_bytes().to_vec();
        encoded.extend_from_slice(body);
        let values = vec![(
            "blob".into(),
            Value::Custom(CustomValue {
                type_tag: "MyBlob".into(),
                encoded,
                display: String::new(),
            }),
        )];
        let bytes = encode_record_by_snapshot(&values, &snap).unwrap();
        let decoded = decode_record_by_snapshot(&bytes, &snap).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn test_decode_truncated_returns_too_short() {
        let snap = snap_with(vec![col("id", DataTypeSnapshot::Uint32, false)]);
        let err = decode_record_by_snapshot(&[0u8, 0u8], &snap).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_encode_count_mismatch_errors() {
        let snap = snap_with(vec![col("id", DataTypeSnapshot::Uint32, false)]);
        let err = encode_record_by_snapshot(&[], &snap).unwrap_err();
        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::IdentityDecodeError(_))
        ));
    }
}

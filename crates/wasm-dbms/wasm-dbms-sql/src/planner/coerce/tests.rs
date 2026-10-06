use rust_decimal::Decimal as RustDecimal;
use wasm_dbms_api::prelude::{
    Blob, CandidDataTypeKind, CustomValue, DataTypeKind, Date, DateTime, Decimal, Encode as _,
    Json, SqlError, Uuid, Value, WireSize,
};

use super::{coerce, coerce_filter_value, like_pattern, row_count};
use crate::ast::{Literal, RowCount, ValueExpr};

const PRIORITY: DataTypeKind = DataTypeKind::Custom {
    tag: "priority",
    wire_size: WireSize::Fixed(1),
};

fn integer(value: i128) -> ValueExpr {
    ValueExpr::Literal(Literal::Integer(value))
}

fn float(value: &str) -> ValueExpr {
    ValueExpr::Literal(Literal::Float(value.to_string()))
}

fn string(value: &str) -> ValueExpr {
    ValueExpr::Literal(Literal::String(value.to_string()))
}

fn boolean(value: bool) -> ValueExpr {
    ValueExpr::Literal(Literal::Boolean(value))
}

const NULL: ValueExpr = ValueExpr::Literal(Literal::Null);

/// Coerces a literal for column `col`.
fn literal(value: &ValueExpr, kind: DataTypeKind) -> Result<Value, SqlError> {
    coerce(value, &[], "col", kind)
}

/// Coerces `value` bound to the first `?` placeholder for column `col`.
fn parameter(value: Value, kind: DataTypeKind) -> Result<Value, SqlError> {
    coerce(&ValueExpr::Parameter(0), &[value], "col", kind)
}

/// `(column, expected, got)` of a [`SqlError::TypeMismatch`].
fn mismatch(result: Result<Value, SqlError>) -> (String, CandidDataTypeKind, String) {
    match result {
        Err(SqlError::TypeMismatch {
            column,
            expected,
            got,
        }) => (column, expected, got),
        other => panic!("expected a type mismatch, got {other:?}"),
    }
}

/// `reason` of a [`SqlError::InvalidLiteral`] raised for column `col`.
fn invalid(result: Result<Value, SqlError>, expected_kind: CandidDataTypeKind) -> String {
    match result {
        Err(SqlError::InvalidLiteral {
            column,
            expected,
            reason,
        }) => {
            assert_eq!(column, "col");
            assert_eq!(expected, expected_kind);
            reason
        }
        other => panic!("expected an invalid literal, got {other:?}"),
    }
}

fn decimal(text: &str) -> Value {
    Value::Decimal(Decimal(text.parse::<RustDecimal>().expect("valid decimal")))
}

fn uuid(bytes: [u8; 16]) -> Value {
    Value::Uuid(Uuid::decode(std::borrow::Cow::Owned(bytes.to_vec())).expect("valid uuid"))
}

fn priority(tag: &str) -> Value {
    Value::Custom(CustomValue {
        type_tag: tag.to_string(),
        encoded: vec![1],
        display: "priority 1".to_string(),
    })
}

#[test]
fn test_integer_literal_fits_every_integer_kind_at_its_bounds() {
    let cases: [(DataTypeKind, i128, i128); 8] = [
        (DataTypeKind::Int8, i8::MIN.into(), i8::MAX.into()),
        (DataTypeKind::Int16, i16::MIN.into(), i16::MAX.into()),
        (DataTypeKind::Int32, i32::MIN.into(), i32::MAX.into()),
        (DataTypeKind::Int64, i64::MIN.into(), i64::MAX.into()),
        (DataTypeKind::Uint8, 0, u8::MAX.into()),
        (DataTypeKind::Uint16, 0, u16::MAX.into()),
        (DataTypeKind::Uint32, 0, u32::MAX.into()),
        (DataTypeKind::Uint64, 0, u64::MAX.into()),
    ];
    for (kind, min, max) in cases {
        for bound in [min, max] {
            let value = literal(&integer(bound), kind).expect("bound must fit");
            assert_eq!(value.type_name(), format!("{kind:?}"));
        }
        for outside in [min - 1, max + 1] {
            let reason = invalid(literal(&integer(outside), kind), kind.into());
            assert_eq!(
                reason,
                format!("integer {outside} is out of range for {kind:?}")
            );
        }
    }
}

#[test]
fn test_integer_literal_keeps_its_value() {
    assert_eq!(
        literal(&integer(-5), DataTypeKind::Int8).unwrap(),
        Value::from(-5i8)
    );
    assert_eq!(
        literal(&integer(42), DataTypeKind::Uint32).unwrap(),
        Value::from(42u32)
    );
    assert_eq!(
        literal(&integer(u64::MAX.into()), DataTypeKind::Uint64).unwrap(),
        Value::from(u64::MAX)
    );
    assert_eq!(
        literal(&integer(i64::MIN.into()), DataTypeKind::Int64).unwrap(),
        Value::from(i64::MIN)
    );
}

#[test]
fn test_integer_literal_becomes_decimal_for_decimal_columns() {
    assert_eq!(
        literal(&integer(-12), DataTypeKind::Decimal).unwrap(),
        decimal("-12")
    );
    assert_eq!(
        literal(&integer(u64::MAX.into()), DataTypeKind::Decimal).unwrap(),
        decimal("18446744073709551615")
    );
}

#[test]
fn test_integer_literal_is_rejected_by_non_numeric_columns() {
    for kind in [
        DataTypeKind::Text,
        DataTypeKind::Boolean,
        DataTypeKind::Date,
        DataTypeKind::DateTime,
        DataTypeKind::Uuid,
        DataTypeKind::Blob,
        DataTypeKind::Json,
        PRIORITY,
    ] {
        assert_eq!(
            mismatch(literal(&integer(1), kind)),
            ("col".to_string(), kind.into(), "Integer".to_string())
        );
    }
}

#[test]
fn test_float_literal_becomes_decimal() {
    assert_eq!(
        literal(&float("1.50"), DataTypeKind::Decimal).unwrap(),
        decimal("1.50")
    );
    assert_eq!(
        literal(&float("-0.25"), DataTypeKind::Decimal).unwrap(),
        decimal("-0.25")
    );
}

#[test]
fn test_float_literal_is_rejected_by_other_columns() {
    for kind in [
        DataTypeKind::Int32,
        DataTypeKind::Uint64,
        DataTypeKind::Text,
        DataTypeKind::Boolean,
    ] {
        assert_eq!(
            mismatch(literal(&float("1.5"), kind)),
            ("col".to_string(), kind.into(), "Float".to_string())
        );
    }
}

#[test]
fn test_float_literal_with_too_many_digits_is_invalid() {
    let digits = format!("{int}.5", int = "9".repeat(40));
    let reason = invalid(
        literal(&float(&digits), DataTypeKind::Decimal),
        CandidDataTypeKind::Decimal,
    );
    assert_eq!(reason, format!("`{digits}` is not a valid decimal"));
}

#[test]
fn test_string_literal_for_text_column() {
    assert_eq!(
        literal(&string("it's"), DataTypeKind::Text).unwrap(),
        Value::from("it's")
    );
    assert_eq!(
        literal(&string(""), DataTypeKind::Text).unwrap(),
        Value::from("")
    );
}

#[test]
fn test_string_literal_is_rejected_by_number_and_boolean_columns() {
    for kind in [
        DataTypeKind::Int8,
        DataTypeKind::Uint32,
        DataTypeKind::Decimal,
        DataTypeKind::Boolean,
        PRIORITY,
    ] {
        assert_eq!(
            mismatch(literal(&string("1"), kind)),
            ("col".to_string(), kind.into(), "String".to_string())
        );
    }
}

#[test]
fn test_string_literal_becomes_date() {
    assert_eq!(
        literal(&string("2026-04-24"), DataTypeKind::Date).unwrap(),
        Value::Date(Date {
            year: 2026,
            month: 4,
            day: 24
        })
    );
    // leap day
    assert_eq!(
        literal(&string("2024-02-29"), DataTypeKind::Date).unwrap(),
        Value::Date(Date {
            year: 2024,
            month: 2,
            day: 29
        })
    );
}

#[test]
fn test_invalid_date_strings() {
    for text in [
        "2026-02-30",
        "2025-02-29",
        "2026-13-01",
        "2026-00-10",
        "2026-04-00",
        "2026-4-24",
        "26-04-24",
        "2026/04/24",
        "2026-04-24T00:00:00Z",
        "2026-04-24 ",
        "tomorrow",
        "",
    ] {
        let reason = invalid(
            literal(&string(text), DataTypeKind::Date),
            CandidDataTypeKind::Date,
        );
        assert_eq!(
            reason,
            format!("`{text}` is not a valid date (expected YYYY-MM-DD)")
        );
    }
}

#[test]
fn test_string_literal_becomes_datetime() {
    let base = DateTime {
        year: 2026,
        month: 4,
        day: 24,
        hour: 13,
        minute: 5,
        second: 9,
        microsecond: 0,
        timezone_offset_minutes: 0,
    };
    let cases = [
        ("2026-04-24T13:05:09Z", base),
        ("2026-04-24T13:05:09", base),
        ("2026-04-24 13:05:09", base),
        ("2026-04-24T13:05:09+00:00", base),
        (
            "2026-04-24T13:05:09.5Z",
            DateTime {
                microsecond: 500_000,
                ..base
            },
        ),
        (
            "2026-04-24T13:05:09.000001Z",
            DateTime {
                microsecond: 1,
                ..base
            },
        ),
        (
            "2026-04-24T13:05:09+02:30",
            DateTime {
                timezone_offset_minutes: 150,
                ..base
            },
        ),
        (
            "2026-04-24 13:05:09.25-08:00",
            DateTime {
                microsecond: 250_000,
                timezone_offset_minutes: -480,
                ..base
            },
        ),
    ];
    for (text, expected) in cases {
        assert_eq!(
            literal(&string(text), DataTypeKind::DateTime).unwrap(),
            Value::DateTime(expected),
            "{text}"
        );
    }
}

#[test]
fn test_datetime_display_form_round_trips() {
    let value = DateTime {
        year: 1999,
        month: 12,
        day: 31,
        hour: 23,
        minute: 59,
        second: 59,
        microsecond: 999_999,
        timezone_offset_minutes: -90,
    };
    assert_eq!(
        literal(&string(&value.to_string()), DataTypeKind::DateTime).unwrap(),
        Value::DateTime(value)
    );
}

#[test]
fn test_invalid_datetime_strings() {
    for text in [
        "2026-04-24",
        "2026-02-30T10:00:00Z",
        "2026-04-24T24:00:00Z",
        "2026-04-24T10:60:00Z",
        "2026-04-24T10:00:60Z",
        "2026-04-24T10:00Z",
        "2026-04-24T10:00:00.Z",
        "2026-04-24T10:00:00.1234567Z",
        "2026-04-24T10:00:00+24:00",
        "2026-04-24T10:00:00+02",
        "2026-04-24T10:00:00 UTC",
        "2026-04-24  10:00:00",
        "10:00:00",
        "",
    ] {
        let reason = invalid(
            literal(&string(text), DataTypeKind::DateTime),
            CandidDataTypeKind::DateTime,
        );
        assert_eq!(
            reason,
            format!(
                "`{text}` is not a valid date-time (expected YYYY-MM-DDTHH:MM:SS[.ffffff][Z|+HH:MM|-HH:MM])"
            ),
        );
    }
}

#[test]
fn test_string_literal_becomes_uuid() {
    let expected = uuid([
        0x55, 0x0e, 0x84, 0x00, 0xe2, 0x9b, 0x41, 0xd4, 0xa7, 0x16, 0x44, 0x66, 0x55, 0x44, 0x00,
        0x00,
    ]);
    for text in [
        "550e8400-e29b-41d4-a716-446655440000",
        "550E8400-E29B-41D4-A716-446655440000",
    ] {
        assert_eq!(
            literal(&string(text), DataTypeKind::Uuid).unwrap(),
            expected
        );
    }
}

#[test]
fn test_invalid_uuid_strings() {
    for text in [
        "550e8400e29b41d4a716446655440000",
        "550e8400-e29b-41d4-a716-44665544000",
        "550e8400-e29b-41d4-a716-4466554400000",
        "550e8400-e29b-41d4-a716-44665544000g",
        "550e8400_e29b_41d4_a716_446655440000",
        "{550e8400-e29b-41d4-a716-446655440000}",
        "",
    ] {
        let reason = invalid(
            literal(&string(text), DataTypeKind::Uuid),
            CandidDataTypeKind::Uuid,
        );
        assert_eq!(reason, format!("`{text}` is not a valid UUID"));
    }
}

#[test]
fn test_string_literal_becomes_blob() {
    let cases: [(&str, &[u8]); 6] = [
        ("deadbeef", &[0xde, 0xad, 0xbe, 0xef]),
        ("DEADBEEF", &[0xde, 0xad, 0xbe, 0xef]),
        ("0x00ff", &[0x00, 0xff]),
        ("0X0a", &[0x0a]),
        ("", &[]),
        // a bare prefix is an empty blob
        ("0x", &[]),
    ];
    for (text, bytes) in cases {
        assert_eq!(
            literal(&string(text), DataTypeKind::Blob).unwrap(),
            Value::Blob(Blob(bytes.to_vec())),
            "{text}"
        );
    }
}

#[test]
fn test_invalid_blob_strings() {
    for text in ["abc", "zz", "0xabc", "de ad", "é0", "0x0x00"] {
        let reason = invalid(
            literal(&string(text), DataTypeKind::Blob),
            CandidDataTypeKind::Blob,
        );
        assert_eq!(
            reason,
            format!("`{text}` is not a hexadecimal string with an even number of digits")
        );
    }
}

#[test]
fn test_string_literal_becomes_json() {
    let text = r#"{"tags": ["a", "b"], "n": 1}"#;
    assert_eq!(
        literal(&string(text), DataTypeKind::Json).unwrap(),
        Value::Json(text.parse::<Json>().expect("valid json"))
    );
}

#[test]
fn test_invalid_json_string() {
    let reason = invalid(
        literal(&string("{not json"), DataTypeKind::Json),
        CandidDataTypeKind::Json,
    );
    assert!(
        reason.starts_with("`{not json` is not valid JSON: "),
        "{reason}"
    );
}

#[test]
fn test_boolean_literal() {
    assert_eq!(
        literal(&boolean(true), DataTypeKind::Boolean).unwrap(),
        Value::from(true)
    );
    assert_eq!(
        literal(&boolean(false), DataTypeKind::Boolean).unwrap(),
        Value::from(false)
    );
    for kind in [
        DataTypeKind::Int8,
        DataTypeKind::Text,
        DataTypeKind::Decimal,
    ] {
        assert_eq!(
            mismatch(literal(&boolean(true), kind)),
            ("col".to_string(), kind.into(), "Boolean".to_string())
        );
    }
}

#[test]
fn test_null_literal_is_null_for_every_kind() {
    for kind in [
        DataTypeKind::Int8,
        DataTypeKind::Text,
        DataTypeKind::Date,
        DataTypeKind::Json,
        PRIORITY,
    ] {
        assert_eq!(literal(&NULL, kind).unwrap(), Value::Null);
        assert_eq!(parameter(Value::Null, kind).unwrap(), Value::Null);
    }
}

#[test]
fn test_parameter_of_the_column_type_passes_through() {
    let date = Value::Date(Date {
        year: 2026,
        month: 1,
        day: 2,
    });
    let datetime = Value::DateTime(DateTime {
        year: 2026,
        month: 1,
        day: 2,
        hour: 3,
        minute: 4,
        second: 5,
        microsecond: 6,
        timezone_offset_minutes: 7,
    });
    let cases = [
        (Value::from(true), DataTypeKind::Boolean),
        (Value::from("text"), DataTypeKind::Text),
        (Value::from(-3i16), DataTypeKind::Int16),
        (Value::from(9u64), DataTypeKind::Uint64),
        (decimal("3.14"), DataTypeKind::Decimal),
        (date, DataTypeKind::Date),
        (datetime, DataTypeKind::DateTime),
        (uuid([7; 16]), DataTypeKind::Uuid),
        (Value::Blob(Blob(vec![1, 2, 3])), DataTypeKind::Blob),
        (
            Value::Json(r#"{"a":1}"#.parse::<Json>().expect("valid json")),
            DataTypeKind::Json,
        ),
        (priority("priority"), PRIORITY),
    ];
    for (value, kind) in cases {
        assert_eq!(parameter(value.clone(), kind).unwrap(), value);
    }
}

#[test]
fn test_integer_parameter_is_converted_between_integer_kinds() {
    assert_eq!(
        parameter(Value::from(5i64), DataTypeKind::Uint8).unwrap(),
        Value::from(5u8)
    );
    assert_eq!(
        parameter(Value::from(200u8), DataTypeKind::Int64).unwrap(),
        Value::from(200i64)
    );
    assert_eq!(
        parameter(Value::from(7u32), DataTypeKind::Decimal).unwrap(),
        decimal("7")
    );
    let reason = invalid(
        parameter(Value::from(-1i64), DataTypeKind::Uint32),
        CandidDataTypeKind::Uint32,
    );
    assert_eq!(reason, "integer -1 is out of range for Uint32");
    let reason = invalid(
        parameter(Value::from(300u16), DataTypeKind::Int8),
        CandidDataTypeKind::Int8,
    );
    assert_eq!(reason, "integer 300 is out of range for Int8");
}

#[test]
fn test_text_parameter_is_converted_like_a_string_literal() {
    assert_eq!(
        parameter(Value::from("2026-04-24"), DataTypeKind::Date).unwrap(),
        Value::Date(Date {
            year: 2026,
            month: 4,
            day: 24
        })
    );
    assert_eq!(
        parameter(Value::from("ff"), DataTypeKind::Blob).unwrap(),
        Value::Blob(Blob(vec![0xff]))
    );
    let reason = invalid(
        parameter(Value::from("nope"), DataTypeKind::Uuid),
        CandidDataTypeKind::Uuid,
    );
    assert_eq!(reason, "`nope` is not a valid UUID");
}

#[test]
fn test_parameter_of_another_type_is_a_mismatch_named_after_its_type() {
    let cases = [
        (Value::from(true), DataTypeKind::Text, "Boolean"),
        (decimal("1.5"), DataTypeKind::Int32, "Decimal"),
        (Value::from("1"), DataTypeKind::Uint32, "Text"),
        (Value::from(1u32), DataTypeKind::Text, "Uint32"),
        (
            Value::Date(Date {
                year: 2026,
                month: 1,
                day: 1,
            }),
            DataTypeKind::DateTime,
            "Date",
        ),
        (Value::Blob(Blob(vec![1])), DataTypeKind::Uuid, "Blob"),
        (priority("status"), PRIORITY, "Custom(status)"),
        (priority("priority"), DataTypeKind::Text, "Custom(priority)"),
    ];
    for (value, kind, got) in cases {
        assert_eq!(
            mismatch(parameter(value, kind)),
            ("col".to_string(), kind.into(), got.to_string())
        );
    }
}

#[test]
fn test_missing_parameter_is_a_count_mismatch() {
    let result = coerce(
        &ValueExpr::Parameter(2),
        &[Value::from(1u32)],
        "col",
        DataTypeKind::Uint32,
    );
    assert!(matches!(
        result,
        Err(SqlError::ParameterCountMismatch {
            expected: 3,
            got: 1
        })
    ));
}

#[test]
fn test_filter_value_rejects_null_parameter() {
    assert_eq!(
        coerce_filter_value(&integer(1), &[], "col", DataTypeKind::Uint8).unwrap(),
        Value::from(1u8)
    );
    assert_eq!(
        mismatch(coerce_filter_value(
            &ValueExpr::Parameter(0),
            &[Value::Null],
            "col",
            DataTypeKind::Uint8
        )),
        (
            "col".to_string(),
            CandidDataTypeKind::Uint8,
            "Null".to_string()
        )
    );
}

#[test]
fn test_like_pattern_accepts_strings_only() {
    assert_eq!(like_pattern(&string("A%"), &[], "name").unwrap(), "A%");
    assert_eq!(
        like_pattern(&ValueExpr::Parameter(0), &[Value::from("%z")], "name").unwrap(),
        "%z"
    );
    for value in [Value::from(1u32), Value::Null, Value::from(true)] {
        let got = value.type_name().to_string();
        match like_pattern(&ValueExpr::Parameter(0), &[value], "name") {
            Err(SqlError::TypeMismatch {
                column,
                expected,
                got: actual,
            }) => {
                assert_eq!(column, "name");
                assert_eq!(expected, CandidDataTypeKind::Text);
                assert_eq!(actual, got);
            }
            other => panic!("expected a type mismatch, got {other:?}"),
        }
    }
}

#[test]
fn test_row_count_from_literal_and_parameter() {
    assert_eq!(row_count(&RowCount::Value(10), &[], "LIMIT").unwrap(), 10);
    assert_eq!(row_count(&RowCount::Value(0), &[], "OFFSET").unwrap(), 0);
    assert_eq!(
        row_count(&RowCount::Value(u64::from(u32::MAX)), &[], "LIMIT").unwrap(),
        usize::try_from(u32::MAX).unwrap()
    );
    for value in [Value::from(3u8), Value::from(3i64), Value::from(3u64)] {
        assert_eq!(
            row_count(&RowCount::Parameter(0), &[value], "LIMIT").unwrap(),
            3
        );
    }
    assert_eq!(
        row_count(&RowCount::Parameter(0), &[Value::from(u32::MAX)], "OFFSET").unwrap(),
        usize::try_from(u32::MAX).unwrap()
    );
}

#[test]
fn test_row_count_rejects_negative_and_non_integer_parameters() {
    match row_count(&RowCount::Parameter(0), &[Value::from(-1i32)], "LIMIT") {
        Err(SqlError::InvalidLiteral {
            column,
            expected,
            reason,
        }) => {
            assert_eq!(column, "LIMIT");
            assert_eq!(expected, CandidDataTypeKind::Uint64);
            assert_eq!(reason, "row count -1 is out of range");
        }
        other => panic!("expected an invalid literal, got {other:?}"),
    }
    for value in [Value::from("5"), Value::Null, decimal("5")] {
        let got = value.type_name().to_string();
        match row_count(&RowCount::Parameter(0), &[value], "OFFSET") {
            Err(SqlError::TypeMismatch {
                column,
                expected,
                got: actual,
            }) => {
                assert_eq!(column, "OFFSET");
                assert_eq!(expected, CandidDataTypeKind::Uint64);
                assert_eq!(actual, got);
            }
            other => panic!("expected a type mismatch, got {other:?}"),
        }
    }
}

#[test]
fn test_row_count_rejects_values_above_wasm32_maximum() {
    let too_large = u64::from(u32::MAX) + 1;
    for clause in ["LIMIT", "OFFSET"] {
        for count in [RowCount::Value(too_large), RowCount::Parameter(0)] {
            let params = [Value::from(too_large)];
            match row_count(&count, &params, clause) {
                Err(SqlError::InvalidLiteral {
                    column,
                    expected,
                    reason,
                }) => {
                    assert_eq!(column, clause);
                    assert_eq!(expected, CandidDataTypeKind::Uint64);
                    assert_eq!(reason, format!("row count {too_large} is out of range"));
                }
                other => panic!("expected an invalid literal, got {other:?}"),
            }
        }
    }
}

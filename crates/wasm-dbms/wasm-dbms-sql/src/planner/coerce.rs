//! Conversion of statement values into [`Value`]s of a column's type.
//!
//! A [`Value`] only compares equal to a value of the same variant, so every
//! literal and parameter is converted to the exact type of the column it is
//! written against before it reaches the DBMS.

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::str::FromStr as _;

use rust_decimal::Decimal as RustDecimal;
use wasm_dbms_api::prelude::{
    Blob, CandidDataTypeKind, DataTypeKind, Date, DateTime, Decimal, Encode as _, Json, SqlError,
    Text, Uuid, Value,
};

use crate::ast::{Literal, RowCount, ValueExpr};

/// A statement value reduced to what matters for conversion.
enum Input {
    Null,
    Integer(i128),
    /// A decimal literal, as written.
    Float(String),
    Text(String),
    Boolean(bool),
    /// A parameter that can only be used as it is.
    Typed(Value),
}

/// Resolves `value` to an [`Input`] and the name used for it in type errors.
///
/// # Errors
///
/// Returns [`SqlError::ParameterCountMismatch`] when `value` is a placeholder
/// with no matching entry in `params`.
fn input(value: &ValueExpr, params: &[Value]) -> Result<(Input, &'static str), SqlError> {
    let literal = match value {
        ValueExpr::Literal(literal) => literal,
        ValueExpr::Parameter(index) => {
            let param = params.get(*index).ok_or(SqlError::ParameterCountMismatch {
                expected: index + 1,
                got: params.len(),
            })?;
            return Ok((parameter_input(param), param.type_name()));
        }
    };
    Ok(match literal {
        Literal::Null => (Input::Null, "Null"),
        Literal::Integer(value) => (Input::Integer(*value), "Integer"),
        Literal::Float(value) => (Input::Float(value.clone()), "Float"),
        Literal::String(value) => (Input::Text(value.clone()), "String"),
        Literal::Boolean(value) => (Input::Boolean(*value), "Boolean"),
    })
}

/// Classifies a bound parameter: integers, text, and booleans convert like the
/// matching literal; every other value must already have the column's type.
fn parameter_input(param: &Value) -> Input {
    match param {
        Value::Null => Input::Null,
        Value::Int8(value) => Input::Integer(value.0.into()),
        Value::Int16(value) => Input::Integer(value.0.into()),
        Value::Int32(value) => Input::Integer(value.0.into()),
        Value::Int64(value) => Input::Integer(value.0.into()),
        Value::Uint8(value) => Input::Integer(value.0.into()),
        Value::Uint16(value) => Input::Integer(value.0.into()),
        Value::Uint32(value) => Input::Integer(value.0.into()),
        Value::Uint64(value) => Input::Integer(value.0.into()),
        Value::Text(value) => Input::Text(value.0.clone()),
        Value::Boolean(value) => Input::Boolean(value.0),
        other => Input::Typed(other.clone()),
    }
}

/// Converts `value` into a [`Value`] that `column`, of type `kind`, can store.
///
/// `NULL` converts to [`Value::Null`] for every type; whether the column
/// accepts it is checked by the DBMS.
///
/// # Errors
///
/// - [`SqlError::TypeMismatch`] when the value's kind cannot be used for `kind`.
/// - [`SqlError::InvalidLiteral`] when the kind fits but the content does not,
///   such as an out-of-range integer or a malformed date.
/// - [`SqlError::ParameterCountMismatch`] when a placeholder has no parameter.
pub(super) fn coerce(
    value: &ValueExpr,
    params: &[Value],
    column: &str,
    kind: DataTypeKind,
) -> Result<Value, SqlError> {
    let (input, got) = input(value, params)?;
    let mismatch = || SqlError::TypeMismatch {
        column: column.to_string(),
        expected: kind.into(),
        got: got.to_string(),
    };
    let invalid = |reason: String| SqlError::InvalidLiteral {
        column: column.to_string(),
        expected: kind.into(),
        reason,
    };
    match (input, kind) {
        (Input::Null, _) => Ok(Value::Null),
        (Input::Integer(value), DataTypeKind::Decimal) => {
            RustDecimal::try_from_i128_with_scale(value, 0)
                .map(|decimal| Value::Decimal(Decimal(decimal)))
                .map_err(|_| invalid(format!("integer {value} is out of range for Decimal")))
        }
        (Input::Integer(value), _) => integer(value, kind)
            .ok_or_else(mismatch)?
            .ok_or_else(|| invalid(format!("integer {value} is out of range for {kind:?}"))),
        (Input::Float(text), DataTypeKind::Decimal) => RustDecimal::from_str(&text)
            .map(|decimal| Value::Decimal(Decimal(decimal)))
            .map_err(|_| invalid(format!("`{text}` is not a valid decimal"))),
        (Input::Boolean(value), DataTypeKind::Boolean) => Ok(Value::from(value)),
        (Input::Text(text), DataTypeKind::Text) => Ok(Value::Text(Text(text))),
        (Input::Text(text), DataTypeKind::Date) => {
            parse_date(&text).map(Value::Date).ok_or_else(|| {
                invalid(format!(
                    "`{text}` is not a valid date (expected YYYY-MM-DD)"
                ))
            })
        }
        (Input::Text(text), DataTypeKind::DateTime) => {
            parse_datetime(&text).map(Value::DateTime).ok_or_else(|| {
                invalid(format!(
                    "`{text}` is not a valid date-time (expected \
                     YYYY-MM-DDTHH:MM:SS[.ffffff][Z|+HH:MM|-HH:MM])"
                ))
            })
        }
        (Input::Text(text), DataTypeKind::Uuid) => parse_uuid(&text)
            .map(Value::Uuid)
            .ok_or_else(|| invalid(format!("`{text}` is not a valid UUID"))),
        (Input::Text(text), DataTypeKind::Blob) => parse_hex(&text)
            .map(|bytes| Value::Blob(Blob(bytes)))
            .ok_or_else(|| {
                invalid(format!(
                    "`{text}` is not a hexadecimal string with an even number of digits"
                ))
            }),
        (Input::Text(text), DataTypeKind::Json) => Json::from_str(&text)
            .map(Value::Json)
            .map_err(|error| invalid(format!("`{text}` is not valid JSON: {error}"))),
        (Input::Typed(value), _) if has_kind(&value, kind) => Ok(value),
        _ => Err(mismatch()),
    }
}

/// Converts a value used in a comparison or an `IN` list.
///
/// # Errors
///
/// Same as [`coerce`], plus [`SqlError::TypeMismatch`] for a `NULL`
/// parameter: a comparison with `NULL` is never true, so `IS NULL` must be
/// used instead.
pub(super) fn coerce_filter_value(
    value: &ValueExpr,
    params: &[Value],
    column: &str,
    kind: DataTypeKind,
) -> Result<Value, SqlError> {
    match coerce(value, params, column, kind)? {
        Value::Null => Err(SqlError::TypeMismatch {
            column: column.to_string(),
            expected: kind.into(),
            got: "Null".to_string(),
        }),
        value => Ok(value),
    }
}

/// Resolves the pattern of a `LIKE` test on `column`.
///
/// # Errors
///
/// Returns [`SqlError::TypeMismatch`] when the pattern is not a string.
pub(super) fn like_pattern(
    value: &ValueExpr,
    params: &[Value],
    column: &str,
) -> Result<String, SqlError> {
    match input(value, params)? {
        (Input::Text(pattern), _) => Ok(pattern),
        (_, got) => Err(SqlError::TypeMismatch {
            column: column.to_string(),
            expected: CandidDataTypeKind::Text,
            got: got.to_string(),
        }),
    }
}

/// Resolves the argument of `LIMIT` or `OFFSET`; `clause` names it in errors.
///
/// # Errors
///
/// - [`SqlError::TypeMismatch`] when a parameter is not an integer.
/// - [`SqlError::InvalidLiteral`] when the count is negative or exceeds the
///   maximum supported on WASM.
pub(super) fn row_count(
    count: &RowCount,
    params: &[Value],
    clause: &str,
) -> Result<usize, SqlError> {
    let value = match count {
        RowCount::Value(value) => i128::from(*value),
        RowCount::Parameter(index) => match input(&ValueExpr::Parameter(*index), params)? {
            (Input::Integer(value), _) => value,
            (_, got) => {
                return Err(SqlError::TypeMismatch {
                    column: clause.to_string(),
                    expected: CandidDataTypeKind::Uint64,
                    got: got.to_string(),
                });
            }
        },
    };
    let count = u32::try_from(value).map_err(|_| SqlError::InvalidLiteral {
        column: clause.to_string(),
        expected: CandidDataTypeKind::Uint64,
        reason: format!("row count {value} is out of range"),
    })?;
    usize::try_from(count).map_err(|_| SqlError::InvalidLiteral {
        column: clause.to_string(),
        expected: CandidDataTypeKind::Uint64,
        reason: format!("row count {value} is out of range"),
    })
}

/// Converts an integer to the integer type `kind`.
///
/// Returns `None` when `kind` is not an integer type, and `Some(None)` when
/// the value does not fit in it.
fn integer(value: i128, kind: DataTypeKind) -> Option<Option<Value>> {
    Some(match kind {
        DataTypeKind::Int8 => i8::try_from(value).ok().map(Value::from),
        DataTypeKind::Int16 => i16::try_from(value).ok().map(Value::from),
        DataTypeKind::Int32 => i32::try_from(value).ok().map(Value::from),
        DataTypeKind::Int64 => i64::try_from(value).ok().map(Value::from),
        DataTypeKind::Uint8 => u8::try_from(value).ok().map(Value::from),
        DataTypeKind::Uint16 => u16::try_from(value).ok().map(Value::from),
        DataTypeKind::Uint32 => u32::try_from(value).ok().map(Value::from),
        DataTypeKind::Uint64 => u64::try_from(value).ok().map(Value::from),
        _ => return None,
    })
}

/// Returns whether `value` is a non-null value of the column type `kind`.
fn has_kind(value: &Value, kind: DataTypeKind) -> bool {
    match (value, kind) {
        (Value::Blob(_), DataTypeKind::Blob)
        | (Value::Boolean(_), DataTypeKind::Boolean)
        | (Value::Date(_), DataTypeKind::Date)
        | (Value::DateTime(_), DataTypeKind::DateTime)
        | (Value::Decimal(_), DataTypeKind::Decimal)
        | (Value::Int8(_), DataTypeKind::Int8)
        | (Value::Int16(_), DataTypeKind::Int16)
        | (Value::Int32(_), DataTypeKind::Int32)
        | (Value::Int64(_), DataTypeKind::Int64)
        | (Value::Json(_), DataTypeKind::Json)
        | (Value::Text(_), DataTypeKind::Text)
        | (Value::Uint8(_), DataTypeKind::Uint8)
        | (Value::Uint16(_), DataTypeKind::Uint16)
        | (Value::Uint32(_), DataTypeKind::Uint32)
        | (Value::Uint64(_), DataTypeKind::Uint64)
        | (Value::Uuid(_), DataTypeKind::Uuid) => true,
        (Value::Custom(custom), DataTypeKind::Custom { tag, .. }) => custom.type_tag == tag,
        _ => false,
    }
}

/// Parses `text` as an unsigned number of `min_len..=max_len` ASCII digits.
fn parse_digits<T>(text: &str, min_len: usize, max_len: usize) -> Option<T>
where
    T: std::str::FromStr,
{
    let well_formed =
        (min_len..=max_len).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    well_formed.then(|| text.parse().ok()).flatten()
}

/// Returns the number of days in `month` of `year`, or `None` for an invalid month.
fn days_in_month(year: u16, month: u8) -> Option<u8> {
    let leap_year =
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if leap_year => Some(29),
        2 => Some(28),
        _ => None,
    }
}

/// Parses a `YYYY-MM-DD` string into a calendar-valid [`Date`].
fn parse_date(text: &str) -> Option<Date> {
    let mut parts = text.split('-');
    let year = parse_digits::<u16>(parts.next()?, 4, 5)?;
    let month = parse_digits::<u8>(parts.next()?, 2, 2)?;
    let day = parse_digits::<u8>(parts.next()?, 2, 2)?;
    if parts.next().is_some() {
        return None;
    }
    days_in_month(year, month)
        .is_some_and(|max_day| (1..=max_day).contains(&day))
        .then_some(Date { year, month, day })
}

/// Parses a date-time string into a calendar-valid [`DateTime`].
///
/// The accepted form is `YYYY-MM-DD`, a `T` or a single space, `HH:MM:SS`, an
/// optional fraction of one to six digits, and an optional `Z` or `±HH:MM`
/// offset. Without an offset the value is taken as UTC.
fn parse_datetime(text: &str) -> Option<DateTime> {
    let (date, time) = text.split_once(['T', ' '])?;
    let Date { year, month, day } = parse_date(date)?;

    let (clock, timezone_offset_minutes) = match (time.strip_suffix('Z'), time.rfind(['+', '-'])) {
        (Some(clock), _) => (clock, 0),
        (None, Some(offset_start)) => {
            let (clock, offset) = time.split_at(offset_start);
            (clock, parse_offset(offset)?)
        }
        (None, None) => (time, 0),
    };

    let (hms, fraction) = match clock.split_once('.') {
        Some((hms, fraction)) => (hms, Some(fraction)),
        None => (clock, None),
    };
    let mut hms = hms.split(':');
    let hour = parse_digits::<u8>(hms.next()?, 2, 2).filter(|hour| *hour < 24)?;
    let minute = parse_digits::<u8>(hms.next()?, 2, 2).filter(|minute| *minute < 60)?;
    let second = parse_digits::<u8>(hms.next()?, 2, 2).filter(|second| *second < 60)?;
    if hms.next().is_some() {
        return None;
    }
    let microsecond = match fraction {
        Some(fraction) => {
            let digits = parse_digits::<u32>(fraction, 1, 6)?;
            // right-pad to six digits: ".5" is 500000 microseconds
            digits * 10_u32.pow(6 - fraction.len() as u32)
        }
        None => 0,
    };

    Some(DateTime {
        year,
        month,
        day,
        hour,
        minute,
        second,
        microsecond,
        timezone_offset_minutes,
    })
}

/// Parses a `±HH:MM` timezone offset smaller than one day into minutes.
fn parse_offset(offset: &str) -> Option<i16> {
    let (sign, rest) = match offset.split_at_checked(1)? {
        ("+", rest) => (1, rest),
        ("-", rest) => (-1, rest),
        _ => return None,
    };
    let (hours, minutes) = rest.split_once(':')?;
    let hours = parse_digits::<i16>(hours, 2, 2).filter(|hours| *hours < 24)?;
    let minutes = parse_digits::<i16>(minutes, 2, 2).filter(|minutes| *minutes < 60)?;
    Some(sign * (hours * 60 + minutes))
}

/// Parses a hyphenated UUID such as `550e8400-e29b-41d4-a716-446655440000`.
///
/// Hex digits may be upper or lower case.
fn parse_uuid(text: &str) -> Option<Uuid> {
    let well_formed = text.len() == 36
        && text.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !well_formed {
        return None;
    }
    let bytes = parse_hex(&text.replace('-', ""))?;
    Uuid::decode(Cow::Owned(bytes)).ok()
}

/// Parses a hexadecimal string, with an optional `0x` prefix, into bytes.
fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    if !digits.len().is_multiple_of(2) || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..digits.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&digits[index..index + 2], 16).ok())
        .collect()
}

use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that clamps integer values within a specified range.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{ClampSanitizer, Value, Sanitize as _};
///
/// let value = Value::Int32(150.into());
/// let sanitizer = ClampSanitizer { min: 0, max: 100 };
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Int32(100.into()));
/// ```
pub struct ClampSanitizer {
    pub min: i64,
    pub max: i64,
}

impl Sanitize for ClampSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Int32(num) => {
                let clamped = (num.0 as i64).clamp(self.min, self.max);
                let clamped: Result<i32, _> = clamped.try_into();
                match clamped {
                    Ok(clamped_i32) => Ok(Value::Int32(clamped_i32.into())),
                    Err(_) => Err(crate::prelude::DbmsError::Sanitize(
                        "Clamped value out of Int32 range".into(),
                    )),
                }
            }
            Value::Int64(num) => {
                let clamped = num.0.clamp(self.min, self.max);
                Ok(Value::Int64(clamped.into()))
            }
            other => Ok(other),
        }
    }
}

/// Sanitizer that clamps unsigned integer values within a specified range.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{ClampUnsignedSanitizer, Value, Sanitize as _};
///
/// let value = Value::Uint32(150.into());
/// let sanitizer = ClampUnsignedSanitizer { min: 0, max: 100 };
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Uint32(100.into()));
/// ```
pub struct ClampUnsignedSanitizer {
    pub min: u64,
    pub max: u64,
}

impl Sanitize for ClampUnsignedSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Uint32(num) => {
                let clamped = (num.0 as u64).clamp(self.min, self.max);
                let clamped: Result<u32, _> = clamped.try_into();
                match clamped {
                    Ok(clamped_u32) => Ok(Value::Uint32(clamped_u32.into())),
                    Err(_) => Err(crate::prelude::DbmsError::Sanitize(
                        "Clamped value out of Uint32 range".into(),
                    )),
                }
            }
            Value::Uint64(num) => {
                let clamped = num.0.clamp(self.min, self.max);
                Ok(Value::Uint64(clamped.into()))
            }
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_sanitizer_i32() {
        let sanitizer = ClampSanitizer { min: 0, max: 100 };
        let value_in_range = Value::Int32(50.into());
        let value_below_range = Value::Int32((-10i32).into());
        let value_above_range = Value::Int32(150.into());
        let non_integer_value = Value::Text("Not an integer".into());

        let sanitized_in_range = sanitizer.sanitize(value_in_range).unwrap();
        let sanitized_below_range = sanitizer.sanitize(value_below_range).unwrap();
        let sanitized_above_range = sanitizer.sanitize(value_above_range).unwrap();
        let sanitized_non_integer = sanitizer.sanitize(non_integer_value).unwrap();

        assert_eq!(sanitized_in_range, Value::Int32(50.into()));
        assert_eq!(sanitized_below_range, Value::Int32(0.into()));
        assert_eq!(sanitized_above_range, Value::Int32(100.into()));
        assert_eq!(sanitized_non_integer, Value::Text("Not an integer".into()));
    }

    #[test]
    fn test_clamp_sanitizer_i64() {
        let sanitizer = ClampSanitizer { min: 0, max: 100 };
        let value_in_range = Value::Int64(50.into());
        let value_below_range = Value::Int64((-10i64).into());
        let value_above_range = Value::Int64(150.into());
        let non_integer_value = Value::Text("Not an integer".into());

        let sanitized_in_range = sanitizer.sanitize(value_in_range).unwrap();
        let sanitized_below_range = sanitizer.sanitize(value_below_range).unwrap();
        let sanitized_above_range = sanitizer.sanitize(value_above_range).unwrap();
        let sanitized_non_integer = sanitizer.sanitize(non_integer_value).unwrap();

        assert_eq!(sanitized_in_range, Value::Int64(50.into()));
        assert_eq!(sanitized_below_range, Value::Int64(0.into()));
        assert_eq!(sanitized_above_range, Value::Int64(100.into()));
        assert_eq!(sanitized_non_integer, Value::Text("Not an integer".into()));
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_u32() {
        let sanitizer = ClampUnsignedSanitizer { min: 0, max: 100 };
        let value_in_range = Value::Uint32(50.into());
        let value_below_range = Value::Uint32(0.into()); // Unsigned can't be negative
        let value_above_range = Value::Uint32(150.into());
        let non_integer_value = Value::Text("Not an integer".into());

        let sanitized_in_range = sanitizer.sanitize(value_in_range).unwrap();
        let sanitized_below_range = sanitizer.sanitize(value_below_range).unwrap();
        let sanitized_above_range = sanitizer.sanitize(value_above_range).unwrap();
        let sanitized_non_integer = sanitizer.sanitize(non_integer_value).unwrap();

        assert_eq!(sanitized_in_range, Value::Uint32(50.into()));
        assert_eq!(sanitized_below_range, Value::Uint32(0.into()));
        assert_eq!(sanitized_above_range, Value::Uint32(100.into()));
        assert_eq!(sanitized_non_integer, Value::Text("Not an integer".into()));
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_u64() {
        let sanitizer = ClampUnsignedSanitizer { min: 0, max: 100 };
        let value_in_range = Value::Uint64(50.into());
        let value_below_range = Value::Uint64(0.into()); // Unsigned can't be negative
        let value_above_range = Value::Uint64(150.into());
        let non_integer_value = Value::Text("Not an integer".into());

        let sanitized_in_range = sanitizer.sanitize(value_in_range).unwrap();
        let sanitized_below_range = sanitizer.sanitize(value_below_range).unwrap();
        let sanitized_above_range = sanitizer.sanitize(value_above_range).unwrap();
        let sanitized_non_integer = sanitizer.sanitize(non_integer_value).unwrap();

        assert_eq!(sanitized_in_range, Value::Uint64(50.into()));
        assert_eq!(sanitized_below_range, Value::Uint64(0.into()));
        assert_eq!(sanitized_above_range, Value::Uint64(100.into()));
        assert_eq!(sanitized_non_integer, Value::Text("Not an integer".into()));
    }
}

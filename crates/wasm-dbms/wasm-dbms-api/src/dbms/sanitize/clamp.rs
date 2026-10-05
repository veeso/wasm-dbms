use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that clamps signed integer values within a specified range.
///
/// Supports [`Value::Int8`], [`Value::Int16`], [`Value::Int32`] and
/// [`Value::Int64`]; other values are returned unchanged. Bounds that exceed
/// the range of the value's integer type are saturated to that range.
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
            Value::Int8(num) => {
                let clamped = self.clamp_within(num.0.into(), i8::MIN.into(), i8::MAX.into());
                Ok(Value::Int8((clamped as i8).into()))
            }
            Value::Int16(num) => {
                let clamped = self.clamp_within(num.0.into(), i16::MIN.into(), i16::MAX.into());
                Ok(Value::Int16((clamped as i16).into()))
            }
            Value::Int32(num) => {
                let clamped = self.clamp_within(num.0.into(), i32::MIN.into(), i32::MAX.into());
                Ok(Value::Int32((clamped as i32).into()))
            }
            Value::Int64(num) => {
                let clamped = num.0.clamp(self.min, self.max);
                Ok(Value::Int64(clamped.into()))
            }
            other => Ok(other),
        }
    }
}

impl ClampSanitizer {
    /// Clamps `num` to the configured bounds, saturating the bounds to the
    /// `[type_min, type_max]` range of the target integer type.
    fn clamp_within(&self, num: i64, type_min: i64, type_max: i64) -> i64 {
        num.clamp(self.min, self.max).clamp(type_min, type_max)
    }
}

/// Sanitizer that clamps unsigned integer values within a specified range.
///
/// Supports [`Value::Uint8`], [`Value::Uint16`], [`Value::Uint32`] and
/// [`Value::Uint64`]; other values are returned unchanged. Bounds that exceed
/// the range of the value's integer type are saturated to that range.
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
            Value::Uint8(num) => {
                let clamped = self.clamp_within(num.0.into(), u8::MAX.into());
                Ok(Value::Uint8((clamped as u8).into()))
            }
            Value::Uint16(num) => {
                let clamped = self.clamp_within(num.0.into(), u16::MAX.into());
                Ok(Value::Uint16((clamped as u16).into()))
            }
            Value::Uint32(num) => {
                let clamped = self.clamp_within(num.0.into(), u32::MAX.into());
                Ok(Value::Uint32((clamped as u32).into()))
            }
            Value::Uint64(num) => {
                let clamped = num.0.clamp(self.min, self.max);
                Ok(Value::Uint64(clamped.into()))
            }
            other => Ok(other),
        }
    }
}

impl ClampUnsignedSanitizer {
    /// Clamps `num` to the configured bounds, saturating the bounds to the
    /// `[0, type_max]` range of the target integer type.
    fn clamp_within(&self, num: u64, type_max: u64) -> u64 {
        num.clamp(self.min, self.max).min(type_max)
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

    #[test]
    fn test_clamp_sanitizer_i8() {
        let sanitizer = ClampSanitizer { min: -10, max: 10 };

        assert_eq!(
            sanitizer.sanitize(Value::Int8(5.into())).unwrap(),
            Value::Int8(5.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Int8((-100i8).into())).unwrap(),
            Value::Int8((-10i8).into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Int8(100.into())).unwrap(),
            Value::Int8(10.into())
        );
    }

    #[test]
    fn test_clamp_sanitizer_i16() {
        let sanitizer = ClampSanitizer { min: -10, max: 10 };

        assert_eq!(
            sanitizer.sanitize(Value::Int16(5.into())).unwrap(),
            Value::Int16(5.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Int16((-1000i16).into())).unwrap(),
            Value::Int16((-10i16).into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Int16(1000.into())).unwrap(),
            Value::Int16(10.into())
        );
    }

    #[test]
    fn test_clamp_sanitizer_saturates_bounds_outside_type_range() {
        let wide = ClampSanitizer {
            min: -1000,
            max: 1000,
        };
        assert_eq!(
            wide.sanitize(Value::Int8(i8::MIN.into())).unwrap(),
            Value::Int8(i8::MIN.into())
        );
        assert_eq!(
            wide.sanitize(Value::Int8(i8::MAX.into())).unwrap(),
            Value::Int8(i8::MAX.into())
        );

        let above = ClampSanitizer {
            min: 1_000,
            max: 2_000,
        };
        assert_eq!(
            above.sanitize(Value::Int8(0.into())).unwrap(),
            Value::Int8(i8::MAX.into())
        );

        let below = ClampSanitizer {
            min: -100_000,
            max: -50_000,
        };
        assert_eq!(
            below.sanitize(Value::Int16(0.into())).unwrap(),
            Value::Int16(i16::MIN.into())
        );

        let above_i32 = ClampSanitizer {
            min: i64::from(i32::MAX) + 1,
            max: i64::MAX,
        };
        assert_eq!(
            above_i32.sanitize(Value::Int32(0.into())).unwrap(),
            Value::Int32(i32::MAX.into())
        );
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_u8() {
        let sanitizer = ClampUnsignedSanitizer { min: 10, max: 100 };

        assert_eq!(
            sanitizer.sanitize(Value::Uint8(50.into())).unwrap(),
            Value::Uint8(50.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Uint8(3.into())).unwrap(),
            Value::Uint8(10.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Uint8(200.into())).unwrap(),
            Value::Uint8(100.into())
        );
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_u16() {
        let sanitizer = ClampUnsignedSanitizer { min: 10, max: 100 };

        assert_eq!(
            sanitizer.sanitize(Value::Uint16(50.into())).unwrap(),
            Value::Uint16(50.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Uint16(3.into())).unwrap(),
            Value::Uint16(10.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Uint16(1000.into())).unwrap(),
            Value::Uint16(100.into())
        );
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_raises_values_below_min() {
        let sanitizer = ClampUnsignedSanitizer { min: 10, max: 100 };

        assert_eq!(
            sanitizer.sanitize(Value::Uint32(3.into())).unwrap(),
            Value::Uint32(10.into())
        );
        assert_eq!(
            sanitizer.sanitize(Value::Uint64(3.into())).unwrap(),
            Value::Uint64(10.into())
        );
    }

    #[test]
    fn test_clamp_unsigned_sanitizer_saturates_bounds_outside_type_range() {
        let wide = ClampUnsignedSanitizer { min: 0, max: 1000 };
        assert_eq!(
            wide.sanitize(Value::Uint8(u8::MAX.into())).unwrap(),
            Value::Uint8(u8::MAX.into())
        );

        let above = ClampUnsignedSanitizer {
            min: 1_000,
            max: 2_000,
        };
        assert_eq!(
            above.sanitize(Value::Uint8(0.into())).unwrap(),
            Value::Uint8(u8::MAX.into())
        );

        let above_u16 = ClampUnsignedSanitizer {
            min: 100_000,
            max: 200_000,
        };
        assert_eq!(
            above_u16.sanitize(Value::Uint16(0.into())).unwrap(),
            Value::Uint16(u16::MAX.into())
        );

        let above_u32 = ClampUnsignedSanitizer {
            min: u64::from(u32::MAX) + 1,
            max: u64::MAX,
        };
        assert_eq!(
            above_u32.sanitize(Value::Uint32(0.into())).unwrap(),
            Value::Uint32(u32::MAX.into())
        );
    }
}

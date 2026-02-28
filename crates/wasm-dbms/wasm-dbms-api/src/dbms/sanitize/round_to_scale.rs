use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that rounds [`rust_decimal::Decimal`] values to a specified scale.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{RoundToScaleSanitizer, Value, Sanitize as _};
/// use rust_decimal::Decimal;
///
/// let value = Value::Decimal(Decimal::new(123456, 4).into()); // 12.3456
/// let sanitizer = RoundToScaleSanitizer(2);
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Decimal(Decimal::new(1235, 2).into())); // 12.35
/// ```
pub struct RoundToScaleSanitizer(pub u32);

impl Sanitize for RoundToScaleSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Decimal(num) => {
                let rounded = num.0.round_dp(self.0);
                Ok(Value::Decimal(rounded.into()))
            }
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal::Decimal;

    use super::*;
    use crate::prelude::Value;

    #[test]
    fn test_round_to_scale_sanitizer() {
        let sanitizer = RoundToScaleSanitizer(2);
        let value = Value::Decimal(Decimal::new(123456, 4).into()); // 12.3456
        let sanitized_value = sanitizer.sanitize(value).unwrap();
        assert_eq!(
            sanitized_value,
            Value::Decimal(Decimal::new(1235, 2).into())
        ); // 12.35

        // Test with non-decimal value
        let non_decimal_value = Value::Int32(42.into());
        let sanitized_value = sanitizer.sanitize(non_decimal_value.clone()).unwrap();
        assert_eq!(sanitized_value, non_decimal_value);
    }
}

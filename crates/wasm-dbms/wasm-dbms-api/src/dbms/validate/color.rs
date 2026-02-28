use crate::prelude::{DbmsError, Validate, Value};

/// A validator for RGB color strings.
///
/// An RGB color string must be in the format `#RRGGBB`, where `RR`, `GG`, and `BB` are
/// two-digit hexadecimal numbers representing the red, green, and blue components of the color.
///
/// # Examples
///
/// ```rust
/// use wasm_dbms_api::prelude::{RgbColorValidator, Value, Validate};
///
/// let validator = RgbColorValidator;
/// let valid_color = Value::Text(wasm_dbms_api::prelude::Text("#1A2B3C".into()));
/// assert!(validator.validate(&valid_color).is_ok());
/// ```
pub struct RgbColorValidator;

impl Validate for RgbColorValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation(
                "RGB color validation requires a text value".to_string(),
            ));
        };

        let s = &text.0;
        if s.len() != 7 || !s.starts_with('#') {
            return Err(DbmsError::Validation(
                "Invalid RGB color format".to_string(),
            ));
        }
        for c in s.chars().skip(1) {
            if !c.is_ascii_hexdigit() {
                return Err(DbmsError::Validation(
                    "Invalid RGB color format".to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::{Text, Value};

    #[test]
    fn test_rgb_color_validator() {
        let validator = RgbColorValidator;

        // Valid RGB color
        let value = Value::Text(Text("#1A2B3C".to_string()));
        assert!(validator.validate(&value).is_ok());

        // Invalid RGB color (wrong length)
        let value = Value::Text(Text("#1A2B3".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid RGB color (missing #)
        let value = Value::Text(Text("1A2B3C".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid RGB color (non-hex character)
        let value = Value::Text(Text("#1A2B3G".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid type
        let value = Value::Int32(123i32.into());
        assert!(validator.validate(&value).is_err());
    }
}

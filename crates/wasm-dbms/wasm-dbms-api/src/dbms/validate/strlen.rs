use crate::prelude::{DbmsError, Validate, Value};

/// A validator that checks if the length of a string does not exceed a maximum length.
///
/// The length is the number of Unicode characters (scalar values), not bytes.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{MaxStrlenValidator, Validate, Value, Text};
/// let validator = MaxStrlenValidator(10);
/// let value = Value::Text(Text("Hello".to_string()));
/// assert!(validator.validate(&value).is_ok());
/// let long_value = Value::Text(Text("Hello, World!".to_string()));
/// assert!(validator.validate(&long_value).is_err());
/// ```
pub struct MaxStrlenValidator(pub usize);

impl Validate for MaxStrlenValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation("Value is not a `Text`".to_string()));
        };

        let len = text.0.chars().count();

        if len <= self.0 {
            Ok(())
        } else {
            Err(DbmsError::Validation(format!(
                "String length {} exceeds maximum allowed length of {}",
                len, self.0
            )))
        }
    }
}

/// A validator that checks if the length of a string is at least a minimum length.
///
/// The length is the number of Unicode characters (scalar values), not bytes.
///
/// # Example
/// ```rust
/// use wasm_dbms_api::prelude::{MinStrlenValidator, Validate, Value, Text};
/// let validator = MinStrlenValidator(5);
/// let value = Value::Text(Text("Hello".to_string()));
/// assert!(validator.validate(&value).is_ok());
/// let short_value = Value::Text(Text("Hi".to_string()));
/// assert!(validator.validate(&short_value).is_err());
/// ```
pub struct MinStrlenValidator(pub usize);

impl Validate for MinStrlenValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation("Value is not a `Text`".to_string()));
        };

        let len = text.0.chars().count();

        if len >= self.0 {
            Ok(())
        } else {
            Err(DbmsError::Validation(format!(
                "String length {} is less than minimum required length of {}",
                len, self.0
            )))
        }
    }
}

/// A validator that checks if the length of a string is within a specified range.
///
/// The length is the number of Unicode characters (scalar values), not bytes.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{RangeStrlenValidator, Validate, Value, Text};
/// let validator = RangeStrlenValidator(3, 10);
/// let value = Value::Text(Text("Hello".to_string()));
/// assert!(validator.validate(&value).is_ok());
/// let short_value = Value::Text(Text("Hi".to_string()));
/// assert!(validator.validate(&short_value).is_err());
/// let long_value = Value::Text(Text("Hello, World!".to_string()));
/// assert!(validator.validate(&long_value).is_err());
/// ```
pub struct RangeStrlenValidator(pub usize, pub usize);

impl Validate for RangeStrlenValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation("Value is not a `Text`".to_string()));
        };

        let len = text.0.chars().count();

        if len >= self.0 && len <= self.1 {
            Ok(())
        } else {
            Err(DbmsError::Validation(format!(
                "String length {} is not within the allowed range of {} to {}",
                len, self.0, self.1
            )))
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::Text;

    #[test]
    fn test_max_strlen_validator() {
        let validator = MaxStrlenValidator(5);

        let valid_value = Value::Text(Text("Hello".to_string()));
        let invalid_value = Value::Text(Text("Hello, World!".to_string()));

        assert!(validator.validate(&valid_value).is_ok());
        assert!(validator.validate(&invalid_value).is_err());
    }

    #[test]
    fn test_max_strlen_validator_non_text() {
        let validator = MaxStrlenValidator(5);
        let non_text_value = Value::Uint32(crate::prelude::Uint32(42));
        assert!(validator.validate(&non_text_value).is_err());
    }

    #[test]
    fn test_min_strlen_validator() {
        let validator = MinStrlenValidator(5);
        let valid_value = Value::Text(Text("Hello".to_string()));
        let invalid_value = Value::Text(Text("Hi".to_string()));
        assert!(validator.validate(&valid_value).is_ok());
        assert!(validator.validate(&invalid_value).is_err());
    }

    #[test]
    fn test_min_strlen_validator_non_text() {
        let validator = MinStrlenValidator(5);
        let non_text_value = Value::Uint32(crate::prelude::Uint32(42));
        assert!(validator.validate(&non_text_value).is_err());
    }

    #[test]
    fn test_range_strlen_validator() {
        let validator = RangeStrlenValidator(3, 10);
        let valid_value = Value::Text(Text("Hello".to_string()));
        let too_short_value = Value::Text(Text("Hi".to_string()));
        let too_long_value = Value::Text(Text("Hello, World!".to_string()));
        assert!(validator.validate(&valid_value).is_ok());
        assert!(validator.validate(&too_short_value).is_err());
        assert!(validator.validate(&too_long_value).is_err());
    }

    #[test]
    fn test_range_strlen_validator_non_text() {
        let validator = RangeStrlenValidator(3, 10);
        let non_text_value = Value::Uint32(crate::prelude::Uint32(42));
        assert!(validator.validate(&non_text_value).is_err());
    }

    #[test]
    fn test_strlen_validators_count_unicode_characters() {
        let emoji = Value::Text(Text("🙂".to_string()));
        let accented = Value::Text(Text("héllo".to_string()));

        assert!(MaxStrlenValidator(1).validate(&emoji).is_ok());
        assert!(MaxStrlenValidator(5).validate(&accented).is_ok());
        assert!(MinStrlenValidator(2).validate(&emoji).is_err());
        assert!(MinStrlenValidator(6).validate(&accented).is_err());
        assert!(RangeStrlenValidator(1, 1).validate(&emoji).is_ok());
        assert!(RangeStrlenValidator(2, 4).validate(&emoji).is_err());
        assert!(RangeStrlenValidator(5, 5).validate(&accented).is_ok());
    }

    #[test]
    fn test_strlen_validator_error_reports_character_count() {
        let two_emoji = Value::Text(Text("🙂🙂".to_string()));

        let err = MaxStrlenValidator(1).validate(&two_emoji).unwrap_err();

        assert!(err.to_string().contains("String length 2 exceeds"), "{err}");
    }
}

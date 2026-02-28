use crate::prelude::{Validate, Value};

/// A validator for `snake_case` strings.
///
/// Rules:
/// - Only lowercase letters, digits, and underscores are allowed.
/// - The string must start with a lowercase letter or an underscore.
///
/// # Examples of valid snake_case
///
/// - `valid_snake_case`
/// - `_leading_underscore`
/// - `snake_case_123`
///
/// # Examples of invalid snake_case
///
/// - `Invalid_Snake_Case` (contains uppercase letters)
/// - `invalid-snake-case!` (contains special characters)
/// - `1invalid_snake_case` (starts with a digit)
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{SnakeCaseValidator, Value, Validate};
///
/// let validator = SnakeCaseValidator;
/// let value = Value::Text(wasm_dbms_api::prelude::Text("valid_snake_case".into()));
/// assert!(validator.validate(&value).is_ok());
/// ```
pub struct SnakeCaseValidator;

impl Validate for SnakeCaseValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "RGB color validation requires a text value".to_string(),
            ));
        };

        let s = &text.0;

        // first must be lowercase letter or underscore
        let first_char = s.chars().next().ok_or_else(|| {
            crate::prelude::DbmsError::Validation(
                "Empty string is not valid snake_case".to_string(),
            )
        })?;
        if !first_char.is_lowercase() && first_char != '_' {
            return Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{s}' is not in snake_case format",
            )));
        }

        if s.chars()
            .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
        {
            Ok(())
        } else {
            Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{s}' is not in snake_case format",
            )))
        }
    }
}

/// A validator for `kebab-case` strings.
///
/// Rules:
/// - Only lowercase letters, digits, and hyphens are allowed.
/// - The string must start with a lowercase letter.
///
/// # Examples of valid kebab-case
///
/// - `valid-kebab-case`
/// - `kebab-case-123`
///
/// # Examples of invalid kebab-case
///
/// - `Invalid-Kebab-Case` (contains uppercase letters)
/// - `invalid_kebab_case!` (contains special characters)
/// - `1invalid-kebab-case` (starts with a digit)
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{KebabCaseValidator, Value, Validate};
/// let validator = KebabCaseValidator;
/// let value = Value::Text(wasm_dbms_api::prelude::Text("valid-kebab-case".into()));
/// assert!(validator.validate(&value).is_ok());
/// ```
pub struct KebabCaseValidator;

impl Validate for KebabCaseValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "RGB color validation requires a text value".to_string(),
            ));
        };

        let s = &text.0;

        // first must be lowercase letter
        let first_char = s.chars().next().ok_or_else(|| {
            crate::prelude::DbmsError::Validation(
                "Empty string is not valid kebab-case".to_string(),
            )
        })?;
        if !first_char.is_lowercase() {
            return Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{s}' is not in kebab-case format",
            )));
        }

        if s.chars()
            .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '-')
        {
            Ok(())
        } else {
            Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{s}' is not in kebab-case format",
            )))
        }
    }
}

/// A validator for `CamelCase` strings.
///
/// Rules:
/// - The string must start with an uppercase letter.
/// - Only alphanumeric characters are allowed (no spaces, underscores, or special characters).
///
/// # Examples of valid CamelCase
/// - `ValidCamelCase`
/// - `AnotherExample123`
///
/// # Examples of invalid CamelCase
///
/// - `invalidCamelCase` (starts with a lowercase letter
/// - `Invalid-CamelCase!` (contains special characters)
/// - `Invalid_CamelCase` (contains underscores)
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{CamelCaseValidator, Value, Validate};
/// let validator = CamelCaseValidator;
/// let value = Value::Text(wasm_dbms_api::prelude::Text("ValidCamelCase".into()));
/// assert!(validator.validate(&value).is_ok());
/// ```
pub struct CamelCaseValidator;

impl Validate for CamelCaseValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "CamelCase validation requires a text value".to_string(),
            ));
        };

        let s = &text.0;

        let mut chars = s.chars();
        let first_char = chars.next().ok_or_else(|| {
            crate::prelude::DbmsError::Validation("Empty string is not valid CamelCase".to_string())
        })?;
        if !first_char.is_uppercase() {
            return Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{s}' is not in CamelCase format"
            )));
        }

        if s.chars().all(|c| c.is_alphanumeric()) {
            Ok(())
        } else {
            Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{}' is not in CamelCase format",
                s
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::{Text, Value};

    #[test]
    fn test_snake_case_validator() {
        let validator = SnakeCaseValidator;

        // Valid snake_case
        let value = Value::Text(Text("valid_snake_case".to_string()));
        assert!(validator.validate(&value).is_ok());

        // Invalid snake_case (uppercase letter)
        let value = Value::Text(Text("Invalid_Snake_Case".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid snake_case (special character)
        let value = Value::Text(Text("invalid-snake-case!".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid snake_case (starts with digit)
        let value = Value::Text(Text("1invalid_snake_case".to_string()));
        assert!(validator.validate(&value).is_err());

        // Valid snake_case (starts with underscore)
        let value = Value::Text(Text("_valid_snake_case".to_string()));
        assert!(validator.validate(&value).is_ok());

        // empty string
        let value = Value::Text(Text("".to_string()));
        assert!(validator.validate(&value).is_err());

        let value = Value::Int32(123i32.into());
        assert!(validator.validate(&value).is_err());
    }

    #[test]
    fn test_kebab_case_validator() {
        let validator = KebabCaseValidator;

        // Valid kebab-case
        let value = Value::Text(Text("valid-kebab-case".to_string()));
        assert!(validator.validate(&value).is_ok());

        // Invalid kebab-case (uppercase letter)
        let value = Value::Text(Text("Invalid-Kebab-Case".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid kebab-case (special character)
        let value = Value::Text(Text("invalid-kebab-case!".to_string()));
        assert!(validator.validate(&value).is_err());

        // empty string
        let value = Value::Text(Text("".to_string()));
        assert!(validator.validate(&value).is_err());

        // Invalid kebab-case (starts with digit)
        let value = Value::Text(Text("1invalid-kebab-case".to_string()));
        assert!(validator.validate(&value).is_err());

        let value = Value::Int32(123i32.into());
        assert!(validator.validate(&value).is_err());
    }

    #[test]
    fn test_camel_case_validator() {
        let validator = CamelCaseValidator;
        // Valid CamelCase
        let value = Value::Text(Text("ValidCamelCase".to_string()));
        assert!(validator.validate(&value).is_ok());
        // Invalid CamelCase (starts with lowercase)
        let value = Value::Text(Text("invalidCamelCase".to_string()));
        assert!(validator.validate(&value).is_err());
        // Invalid CamelCase (special character)
        let value = Value::Text(Text("Invalid-CamelCase!".to_string()));
        assert!(validator.validate(&value).is_err());
        // Invalid CamelCase (contains underscore)
        let value = Value::Text(Text("Invalid_CamelCase".to_string()));
        assert!(validator.validate(&value).is_err());

        // empty string
        let value = Value::Text(Text("".to_string()));
        assert!(validator.validate(&value).is_err());

        let value = Value::Int32(123i32.into());
        assert!(validator.validate(&value).is_err());
    }
}

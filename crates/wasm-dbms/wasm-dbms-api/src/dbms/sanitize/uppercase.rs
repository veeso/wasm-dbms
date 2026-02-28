use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that converts strings to uppercase.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{UpperCaseSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("Hello, World!".into());
/// let sanitizer = UpperCaseSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("HELLO, WORLD!".into()));
/// ```
pub struct UpperCaseSanitizer;

impl Sanitize for UpperCaseSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => Ok(Value::Text(text.as_str().to_uppercase().into())),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_trim_sanitizer() {
        let sanitizer = UpperCaseSanitizer;
        let string = Value::Text("Hello, World!".into());
        let number_value = Value::Int32(42.into());

        let sanitized_uppercase = sanitizer.sanitize(string).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();

        assert_eq!(sanitized_uppercase, Value::Text("HELLO, WORLD!".into()));
        assert_eq!(sanitized_number, Value::Int32(42.into()));
    }
}

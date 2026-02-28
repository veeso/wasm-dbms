use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that trims leading and trailing whitespace from strings.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{TrimSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("  Hello, World!  ".into());
/// let sanitizer = TrimSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("Hello, World!".into()));
/// ```
pub struct TrimSanitizer;

impl Sanitize for TrimSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => Ok(Value::Text(text.as_str().trim().into())),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_trim_sanitizer() {
        let sanitizer = TrimSanitizer;
        let string_with_whitespace = Value::Text("  Hello, World!  ".into());
        let string_without_whitespace = Value::Text("Hello".into());
        let number_value = Value::Int32(42.into());

        let sanitized_with_whitespace = sanitizer.sanitize(string_with_whitespace).unwrap();
        let sanitized_without_whitespace = sanitizer.sanitize(string_without_whitespace).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();

        assert_eq!(
            sanitized_with_whitespace,
            Value::Text("Hello, World!".into())
        );
        assert_eq!(sanitized_without_whitespace, Value::Text("Hello".into()));
        assert_eq!(sanitized_number, Value::Int32(42.into()));
    }
}

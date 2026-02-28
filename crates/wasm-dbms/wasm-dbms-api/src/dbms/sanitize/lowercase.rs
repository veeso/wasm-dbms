use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that converts strings to lowercase.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{LowerCaseSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("Hello, World!".into());
/// let sanitizer = LowerCaseSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("hello, world!".into()));
/// ```
pub struct LowerCaseSanitizer;

impl Sanitize for LowerCaseSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => Ok(Value::Text(text.as_str().to_lowercase().into())),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_trim_sanitizer() {
        let sanitizer = LowerCaseSanitizer;
        let string = Value::Text("Hello, World!".into());
        let number_value = Value::Int32(42.into());

        let sanitized_lowercase = sanitizer.sanitize(string).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();

        assert_eq!(sanitized_lowercase, Value::Text("hello, world!".into()));
        assert_eq!(sanitized_number, Value::Int32(42.into()));
    }
}

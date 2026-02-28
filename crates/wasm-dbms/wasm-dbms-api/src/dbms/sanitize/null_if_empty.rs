use crate::prelude::{DbmsResult, Sanitize, Value};

/// The [`NullIfEmptySanitizer`] struct is used to sanitize input by converting empty strings to null values.
///
/// This [`Sanitize`] never returns an error.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{NullIfEmptySanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("".into());
/// let sanitizer = NullIfEmptySanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Null);
/// ```
pub struct NullIfEmptySanitizer;

impl Sanitize for NullIfEmptySanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) if text.as_str().is_empty() => Ok(Value::Null),
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_null_if_empty_sanitizer() {
        let sanitizer = NullIfEmptySanitizer;
        let empty_string = Value::Text("".into());
        let non_empty_string = Value::Text("Hello".into());
        let number_value = Value::Int32(32.into());

        let sanitized_empty = sanitizer.sanitize(empty_string).unwrap();
        let sanitized_non_empty = sanitizer.sanitize(non_empty_string).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();
        assert_eq!(sanitized_empty, Value::Null);
        assert_eq!(sanitized_non_empty, Value::Text("Hello".into()));
        assert_eq!(sanitized_number, Value::Int32(32.into()));
    }
}

use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer URL-encodes strings by converting them to percent-encoded format.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{UrlEncodingSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("你好 rust".into());
/// let sanitizer = UrlEncodingSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("%E4%BD%A0%E5%A5%BD%20rust".into()));
/// ```
pub struct UrlEncodingSanitizer;

impl Sanitize for UrlEncodingSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => {
                let encoded = percent_encoding::utf8_percent_encode(
                    text.as_str(),
                    percent_encoding::NON_ALPHANUMERIC,
                )
                .to_string();
                Ok(Value::Text(encoded.into()))
            }
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encoding_sanitizer() {
        let sanitizer = UrlEncodingSanitizer;
        let string_value = Value::Text("你好 rust".into());
        let number_value = Value::Int32(42.into());

        let sanitized_string = sanitizer.sanitize(string_value).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();

        assert_eq!(
            sanitized_string,
            Value::Text("%E4%BD%A0%E5%A5%BD%20rust".into())
        );
        assert_eq!(sanitized_number, Value::Int32(42.into()));
    }
}

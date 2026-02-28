use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer sluggifies strings by converting them to lowercase, replacing spaces with hyphens,
/// and removing non-alphanumeric characters.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{SlugSanitizer, Value, Sanitize as _};
///
/// let value = Value::Text("  Hello,       World!  ".into());
/// let sanitizer = SlugSanitizer;
/// let sanitized_value = sanitizer.sanitize(value).unwrap();
/// assert_eq!(sanitized_value, Value::Text("hello-world".into()));
/// ```
pub struct SlugSanitizer;

impl Sanitize for SlugSanitizer {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        match value {
            Value::Text(text) => {
                let slug = text
                    .as_str()
                    .to_lowercase()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("-")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '-')
                    .collect::<String>();
                Ok(Value::Text(slug.into()))
            }
            other => Ok(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slug_sanitizer() {
        let sanitizer = SlugSanitizer;
        let string_value = Value::Text("  Hello,          World!  ".into());
        let number_value = Value::Int32(42.into());

        let sanitized_string = sanitizer.sanitize(string_value).unwrap();
        let sanitized_number = sanitizer.sanitize(number_value).unwrap();

        assert_eq!(sanitized_string, Value::Text("hello-world".into()));
        assert_eq!(sanitized_number, Value::Int32(42.into()));
    }
}

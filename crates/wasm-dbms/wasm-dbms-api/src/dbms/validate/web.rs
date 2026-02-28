use crate::prelude::{DbmsError, Validate, Value};

/// A validator that checks if a string is a valid MIME type.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{MimeTypeValidator, Validate, Value};
///
/// let validator = MimeTypeValidator;
/// let valid_mime = Value::Text(wasm_dbms_api::prelude::Text("text/plain".into()));
/// assert!(validator.validate(&valid_mime).is_ok());
/// let invalid_mime = Value::Text(wasm_dbms_api::prelude::Text("invalid-mime".into()));
/// assert!(validator.validate(&invalid_mime).is_err());
/// ```
pub struct MimeTypeValidator;

impl Validate for MimeTypeValidator {
    fn validate(&self, value: &crate::prelude::Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation("Value is not a Text".to_string()));
        };

        let s = &text.0;

        // must have exactly '/' character
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return Err(DbmsError::Validation(format!(
                "MIME type '{s}' must contain exactly one '/'"
            )));
        }

        let is_valid_part = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
        };

        if !is_valid_part(parts[0]) {
            return Err(DbmsError::Validation(format!(
                "MIME type '{s}' has invalid type part"
            )));
        }
        if !is_valid_part(parts[1]) {
            return Err(DbmsError::Validation(format!(
                "MIME type '{s}' has invalid subtype part"
            )));
        }

        Ok(())
    }
}

/// A validator that checks if a string is a valid URL.
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{UrlValidator, Validate, Value};
/// let validator = UrlValidator;
/// let valid_url = Value::Text(wasm_dbms_api::prelude::Text("http://example.com".into()));
/// assert!(validator.validate(&valid_url).is_ok());
/// ```
pub struct UrlValidator;

impl Validate for UrlValidator {
    fn validate(&self, value: &crate::prelude::Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(DbmsError::Validation("Value is not a Text".to_string()));
        };

        let s = &text.0;

        if url::Url::parse(s).is_err() {
            return Err(DbmsError::Validation(format!(
                "Value '{s}' is not a valid URL"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::Text;

    #[test]
    fn test_should_not_validate_mime_if_not_text() {
        let value = Value::Uint32(crate::prelude::Uint32(42));
        let result = MimeTypeValidator.validate(&value);
        assert!(result.is_err());
    }

    #[test]
    fn test_mime_type_validator() {
        let valid_mime_types = vec![
            "text/plain",
            "image/jpeg",
            "application/json",
            "application/vnd.api+json",
            "audio/mpeg",
        ];
        for mime in valid_mime_types {
            let value = Value::Text(Text(mime.to_string()));
            assert!(
                MimeTypeValidator.validate(&value).is_ok(),
                "MIME type '{mime}' should be valid"
            );
        }
    }

    #[test]
    fn test_invalid_mime_type_validator() {
        let invalid_mime_types = vec![
            "textplain",
            "image//jpeg",
            "/json",
            "application/vnd.api+json/extra",
            "audio/mpeg/",
            "audio/mpe g",
        ];
        for mime in invalid_mime_types {
            let value = Value::Text(Text(mime.to_string()));
            assert!(
                MimeTypeValidator.validate(&value).is_err(),
                "MIME type '{mime}' should be invalid"
            );
        }
    }

    #[test]
    fn test_url_validator() {
        let valid_urls = vec![
            "http://example.com",
            "https://example.com/path?query=param#fragment",
            "ftp://ftp.example.com/resource",
            "mailto:christian@example.com",
        ];
        for url in valid_urls {
            let value = Value::Text(Text(url.to_string()));
            assert!(
                UrlValidator.validate(&value).is_ok(),
                "URL '{url}' should be valid"
            );
        }
    }

    #[test]
    fn test_invalid_url_validator() {
        let invalid_urls = vec![
            //"htp:/example.com",
            "://missing.scheme.com",
            "http//missing.colon.com",
            "justastring",
            "http://in valid.com",
        ];
        for url in invalid_urls {
            let value = Value::Text(Text(url.to_string()));
            assert!(
                UrlValidator.validate(&value).is_err(),
                "URL '{url}' should be invalid"
            );
        }
    }

    #[test]
    fn test_should_not_validate_url_if_not_text() {
        let value = Value::Uint32(crate::prelude::Uint32(42));
        let result = UrlValidator.validate(&value);
        assert!(result.is_err());
    }
}

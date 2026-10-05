use crate::prelude::{DbmsError, Validate, Value};

/// A validator that checks if a string is a valid MIME type.
///
/// The value must have the form `type/subtype`, where both the type and the subtype
/// follow the RFC 6838 section 4.2 `restricted-name` grammar. Parameters such as
/// `; charset=utf-8` are not accepted.
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

        if !is_restricted_name(parts[0]) {
            return Err(DbmsError::Validation(format!(
                "MIME type '{s}' has invalid type part"
            )));
        }
        if !is_restricted_name(parts[1]) {
            return Err(DbmsError::Validation(format!(
                "MIME type '{s}' has invalid subtype part"
            )));
        }

        Ok(())
    }
}

/// Maximum length of a MIME type or subtype name, per RFC 6838 section 4.2.
const MIME_RESTRICTED_NAME_MAX_LEN: usize = 127;

/// Returns whether `name` matches the RFC 6838 section 4.2 `restricted-name` grammar.
///
/// The first character must be an ASCII letter or digit; the following ones may also be
/// any of `!#$&-^_.+`. The whole name must be between 1 and 127 characters long.
fn is_restricted_name(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= MIME_RESTRICTED_NAME_MAX_LEN
        && chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || "!#$&-^_.+".contains(c))
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
    fn test_mime_type_validator_accepts_rfc6838_restricted_name_chars() {
        let max_name = "a".repeat(127);
        let max_len_mime = format!("{max_name}/{max_name}");
        let valid_mime_types = [
            "application/vnd.example_test",
            "application/e!example",
            "application/x#y$z&w^v",
            "1type/2sub",
            "application/vnd.example+json",
            "Text/Plain",
            max_len_mime.as_str(),
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
    fn test_mime_type_validator_rejects_invalid_restricted_names() {
        let long_name = "a".repeat(128);
        let long_type = format!("{long_name}/plain");
        let long_subtype = format!("text/{long_name}");
        let invalid_mime_types = [
            "-/plain",
            "+/plain",
            "./plain",
            "_/plain",
            "text/-plain",
            "text/.plain",
            "text/+json",
            "text/!plain",
            "text/",
            "text /plain",
            "text/pl ain",
            "text/plain; charset=utf-8",
            "text/pl=ain",
            "text/pl*ain",
            "text/plàin",
            long_type.as_str(),
            long_subtype.as_str(),
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

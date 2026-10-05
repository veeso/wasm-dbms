use lazy_regex::{Lazy, Regex, lazy_regex};

use crate::prelude::{Validate, Value};

/// Email address pattern.
///
/// - The local part starts with a letter or digit and is a sequence of dot-separated atoms,
///   so it never contains consecutive dots or ends with a dot.
/// - Each domain label starts and ends with a letter or digit; hyphens are only allowed inside.
/// - The top-level domain has at least two letters.
static EMAIL_REGEX: Lazy<Regex> = lazy_regex!(
    r"^[A-Za-z0-9][A-Za-z0-9_%+-]*(\.[A-Za-z0-9_%+-]+)*@([A-Za-z0-9]([A-Za-z0-9-]*[A-Za-z0-9])?\.)+[A-Za-z]{2,}$"
);

/// A validator for email addresses.
///
/// This validator checks if a given text value conforms to a standard email format.
///
/// # Examples
///
/// ```rust
/// use wasm_dbms_api::prelude::{EmailValidator, Validate, Value};
///
/// let validator = EmailValidator;
/// let valid_email = Value::Text("christian.visintin@gmail.com".into());
/// let invalid_email = Value::Text("invalid-email".into());
///
/// assert!(validator.validate(&valid_email).is_ok());
/// assert!(validator.validate(&invalid_email).is_err());
/// ```
pub struct EmailValidator;

impl Validate for EmailValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "EmailValidator can only be applied to Text values".to_string(),
            ));
        };

        if EMAIL_REGEX.is_match(text.as_str()) {
            Ok(())
        } else {
            Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{text}' is not a valid email address",
            )))
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_email_validator() {
        let validator = EmailValidator;
        let valid_emails = vec![
            "christian.visintin1997@yahoo.com",
            "nome.cognome@gmail.com",
            "user123@outlook.com",
            "user.name+tag@gmail.com",
            "info@azienda.it",
            "support@sub.domain.com",
            "hello-world@my-site.org",
            "a@b.co",
            "test_email@domain.travel",
            "user99@domain.co.uk",
            "a.b.c@x-y.z-w.io",
            "user_name.last-name@a1.b2.example",
            "u@1-2.example.com",
        ];
        let invalid_emails = vec![
            "",
            "plainaddress",
            "@gmail.com",
            "user@",
            "user@gmail",
            "user@gmail.",
            "user@.com",
            "user@@gmail.com",
            "user@gmail..com",
            "user..name@gmail.com",
            ".user@gmail.com",
            "user.@gmail.com",
            "user@-gmail.com",
            "user@gmail-.com",
            "user@sub.-domain.com",
            "user@sub-.domain.com",
            "user@domain.-com",
            "user@111.222.333.444",
            "user@[127.0.0.1]",
            "\"user\"@gmail.com",
            "user name@gmail.com",
        ];

        for email in valid_emails {
            let value = Value::Text(email.into());
            assert!(
                validator.validate(&value).is_ok(),
                "Expected '{}' to be valid",
                email
            );
        }
        for email in invalid_emails {
            let value = Value::Text(email.into());
            assert!(
                validator.validate(&value).is_err(),
                "Expected '{}' to be invalid",
                email
            );
        }
    }
}

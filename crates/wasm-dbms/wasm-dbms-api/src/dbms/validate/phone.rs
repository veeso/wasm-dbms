use lazy_regex::{Lazy, Regex, lazy_regex};

use crate::prelude::{Validate, Value};

static PHONE_REGEX: Lazy<Regex> = lazy_regex!(r"^\+?[0-9\s().-]{7,20}$");

/// A validator for phone numbers.
///
/// # Examples of valid phone numbers:
///
/// +1-202-555-0173
/// (202) 555-0173
/// +44 20 7946 0958
/// 202.555.0173
/// 2025550173
///
/// ## Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{PhoneNumberValidator, Validate, Value};
///
/// let validator = PhoneNumberValidator;
/// let valid_phone = Value::Text("+1-202-555-0173".into());
/// let invalid_phone = Value::Text("123-ABC-7890".into());
///
/// assert!(validator.validate(&valid_phone).is_ok());
/// assert!(validator.validate(&invalid_phone).is_err());
/// ```
pub struct PhoneNumberValidator;

impl Validate for PhoneNumberValidator {
    fn validate(&self, value: &Value) -> crate::prelude::DbmsResult<()> {
        let Value::Text(text) = value else {
            return Err(crate::prelude::DbmsError::Validation(
                "PhoneNumberValidator can only be applied to Text values".to_string(),
            ));
        };

        if PHONE_REGEX.is_match(text.as_str()) {
            Ok(())
        } else {
            Err(crate::prelude::DbmsError::Validation(format!(
                "Value '{text}' is not a valid phone number",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phone_number_validator() {
        let validator = PhoneNumberValidator;
        let valid_phones = vec![
            "+1-202-555-0173",
            "(202) 555-0173",
            "+44 20 7946 0958",
            "202.555.0173",
            "2025550173",
            "+91 (22) 1234-5678",
            "+81-3-1234-5678",
            "123-456-7890",
            "+49 30 123456",
            "+33 366 167 7509",
            "+33 3661677509",
        ];

        let invalid_phones = vec![
            "123-ABC-7890",
            "++1-202-555-0173",
            //"202--555--0173",
            "202 555 0173 ext. 5",
            "phone:2025550173",
            "202/555/0173",
            "202_555_0173",
            "++44 20 7946 0958",
        ];
        for phone in valid_phones {
            let value = Value::Text(phone.into());
            assert!(
                validator.validate(&value).is_ok(),
                "Expected '{}' to be valid",
                phone
            );
        }

        for phone in invalid_phones {
            let value = Value::Text(phone.into());
            assert!(
                validator.validate(&value).is_err(),
                "Expected '{}' to be invalid",
                phone
            );
        }

        // non-Text value
        let non_text_value = Value::Int32(1234567890i32.into());
        assert!(
            validator.validate(&non_text_value).is_err(),
            "Expected non-Text value to be invalid"
        );
    }
}

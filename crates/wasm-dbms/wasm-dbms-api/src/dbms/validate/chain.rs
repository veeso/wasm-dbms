use std::fmt;

use crate::prelude::{DbmsResult, Validate, Value};

/// Validator that runs several validators in order.
///
/// The value is valid when every validator accepts it; the error of the first failing
/// validator is returned. `#[derive(Table)]` uses it when a column declares more than one
/// `#[validate(...)]` attribute, keeping the declaration order (top to bottom).
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{
///     MaxStrlenValidator, MinStrlenValidator, Validate as _, ValidatorChain, Value,
/// };
///
/// let validator = ValidatorChain::new(vec![
///     Box::new(MinStrlenValidator(2)),
///     Box::new(MaxStrlenValidator(5)),
/// ]);
/// assert!(validator.validate(&Value::Text("abc".into())).is_ok());
/// assert!(validator.validate(&Value::Text("a".into())).is_err());
/// assert!(validator.validate(&Value::Text("abcdef".into())).is_err());
/// ```
pub struct ValidatorChain(Vec<Box<dyn Validate>>);

impl ValidatorChain {
    /// Creates a chain running `validators` in the given order.
    pub fn new(validators: Vec<Box<dyn Validate>>) -> Self {
        Self(validators)
    }
}

impl fmt::Debug for ValidatorChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatorChain")
            .field("len", &self.0.len())
            .finish()
    }
}

impl Validate for ValidatorChain {
    fn validate(&self, value: &Value) -> DbmsResult<()> {
        self.0
            .iter()
            .try_for_each(|validator| validator.validate(value))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::{DbmsError, MaxStrlenValidator, MinStrlenValidator};

    struct FailingValidator(&'static str);

    impl Validate for FailingValidator {
        fn validate(&self, _value: &Value) -> DbmsResult<()> {
            Err(DbmsError::Validation(self.0.to_string()))
        }
    }

    #[test]
    fn test_should_accept_value_valid_for_every_validator() {
        let validator = ValidatorChain::new(vec![
            Box::new(MinStrlenValidator(2)),
            Box::new(MaxStrlenValidator(5)),
        ]);
        assert!(validator.validate(&Value::Text("abc".into())).is_ok());
        assert!(validator.validate(&Value::Text("a".into())).is_err());
        assert!(validator.validate(&Value::Text("abcdef".into())).is_err());
    }

    #[test]
    fn test_should_accept_any_value_when_empty() {
        let validator = ValidatorChain::new(vec![]);
        assert!(validator.validate(&Value::Text("anything".into())).is_ok());
    }

    #[test]
    fn test_should_return_first_error() {
        let validator = ValidatorChain::new(vec![
            Box::new(FailingValidator("first")),
            Box::new(FailingValidator("second")),
        ]);
        let err = validator.validate(&Value::Text("x".into())).unwrap_err();
        assert!(matches!(err, DbmsError::Validation(message) if message == "first"));
    }
}

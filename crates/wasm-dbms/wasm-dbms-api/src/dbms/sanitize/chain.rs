use std::fmt;

use crate::prelude::{DbmsResult, Sanitize, Value};

/// Sanitizer that applies several sanitizers in order.
///
/// Each sanitizer receives the output of the previous one; the first error stops the chain.
/// `#[derive(Table)]` uses it when a column declares more than one `#[sanitizer(...)]`
/// attribute, keeping the declaration order (top to bottom).
///
/// # Example
///
/// ```rust
/// use wasm_dbms_api::prelude::{
///     LowerCaseSanitizer, Sanitize as _, SanitizerChain, TrimSanitizer, Value,
/// };
///
/// let sanitizer = SanitizerChain::new(vec![Box::new(TrimSanitizer), Box::new(LowerCaseSanitizer)]);
/// let sanitized = sanitizer.sanitize(Value::Text("  ALICE  ".into())).unwrap();
/// assert_eq!(sanitized, Value::Text("alice".into()));
/// ```
pub struct SanitizerChain(Vec<Box<dyn Sanitize>>);

impl SanitizerChain {
    /// Creates a chain applying `sanitizers` in the given order.
    pub fn new(sanitizers: Vec<Box<dyn Sanitize>>) -> Self {
        Self(sanitizers)
    }
}

impl fmt::Debug for SanitizerChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SanitizerChain")
            .field("len", &self.0.len())
            .finish()
    }
}

impl Sanitize for SanitizerChain {
    fn sanitize(&self, value: Value) -> DbmsResult<Value> {
        self.0
            .iter()
            .try_fold(value, |value, sanitizer| sanitizer.sanitize(value))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::prelude::{DbmsError, LowerCaseSanitizer, TrimSanitizer};

    struct FailingSanitizer;

    impl Sanitize for FailingSanitizer {
        fn sanitize(&self, _value: Value) -> DbmsResult<Value> {
            Err(DbmsError::Sanitize("failure".to_string()))
        }
    }

    #[test]
    fn test_should_apply_sanitizers_in_order() {
        let sanitizer =
            SanitizerChain::new(vec![Box::new(TrimSanitizer), Box::new(LowerCaseSanitizer)]);
        let sanitized = sanitizer
            .sanitize(Value::Text("  Hello World  ".into()))
            .unwrap();
        assert_eq!(sanitized, Value::Text("hello world".into()));
    }

    #[test]
    fn test_should_return_value_unchanged_when_empty() {
        let sanitizer = SanitizerChain::new(vec![]);
        let value = Value::Text(" as is ".into());
        assert_eq!(sanitizer.sanitize(value.clone()).unwrap(), value);
    }

    #[test]
    fn test_should_stop_at_first_error() {
        let sanitizer =
            SanitizerChain::new(vec![Box::new(FailingSanitizer), Box::new(TrimSanitizer)]);
        assert!(matches!(
            sanitizer.sanitize(Value::Text(" x ".into())),
            Err(DbmsError::Sanitize(_))
        ));
    }
}

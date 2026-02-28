//! This module contains all the built-in sanitizers which can be applied to columns.
//!
//! Each validation function takes a [`crate::prelude::Value`] as input and returns a `DbmsResult<crate::prelude::Value>` with the sanitized
//! value or an error if the value could not be sanitized.
//!
//! This module contains the [`Sanitize`] trait which should be implemented by all sanitizers.

mod clamp;
mod collapse_whitespace;
mod lowercase;
mod null_if_empty;
mod round_to_scale;
mod slug_sanitizer;
mod timezone;
mod trim;
mod uppercase;
mod url_encoding;

pub use self::clamp::{ClampSanitizer, ClampUnsignedSanitizer};
pub use self::collapse_whitespace::CollapseWhitespaceSanitizer;
pub use self::lowercase::LowerCaseSanitizer;
pub use self::null_if_empty::NullIfEmptySanitizer;
pub use self::round_to_scale::RoundToScaleSanitizer;
pub use self::slug_sanitizer::SlugSanitizer;
pub use self::timezone::{TimezoneSanitizer, UtcSanitizer};
pub use self::trim::TrimSanitizer;
pub use self::uppercase::UpperCaseSanitizer;
pub use self::url_encoding::UrlEncodingSanitizer;
use crate::prelude::{DbmsResult, Value};

/// Trait for sanitizing [`Value`]s.
pub trait Sanitize {
    /// Sanitizes the given [`Value`].
    ///
    /// In case of error it should return a [`crate::prelude::DbmsError::Sanitize`] error.
    ///
    /// Sanitizers should not return error if the value is not of the expected type, they should just return the value as is.
    fn sanitize(&self, value: Value) -> DbmsResult<Value>;
}

//! This module contains all the built-in validations which can be applied to columns.
//!
//! Each validation function takes a [`&crate::prelude::Value`] as input and returns a `DbmsResult<()>` indicating
//! whether the value passes the validation or not.

mod case;
mod color;
mod email;
mod locale;
mod phone;
mod strlen;
mod web;

pub use self::case::{CamelCaseValidator, KebabCaseValidator, SnakeCaseValidator};
pub use self::color::RgbColorValidator;
pub use self::email::EmailValidator;
pub use self::locale::{CountryIso639Validator, CountryIso3166Validator};
pub use self::phone::PhoneNumberValidator;
pub use self::strlen::{MaxStrlenValidator, MinStrlenValidator, RangeStrlenValidator};
pub use self::web::{MimeTypeValidator, UrlValidator};
use crate::error::DbmsResult;

/// Trait for validating [`crate::prelude::Value`]s.
pub trait Validate {
    /// Validates the given [`crate::prelude::Value`].
    ///
    /// In case of error it should return a [`crate::prelude::DbmsError::Validation`] error.
    fn validate(&self, value: &crate::prelude::Value) -> DbmsResult<()>;
}

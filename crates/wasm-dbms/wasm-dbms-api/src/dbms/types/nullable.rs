use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::dbms::value::Value;
use crate::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, PageOffset};

/// Nullable data type for the DBMS.
///
/// A nullable means that the type can either hold a value of type T or be null.
/// It is a wrapper around another [`DataType`] T.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[serde(bound(deserialize = "T: DataType"))]
pub enum Nullable<T>
where
    T: DataType,
{
    #[default]
    Null,
    Value(T),
}

impl<T> fmt::Display for Nullable<T>
where
    T: DataType,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Nullable::Null => write!(f, "NULL"),
            Nullable::Value(v) => write!(f, "{}", v),
        }
    }
}

impl<T> From<Nullable<T>> for Value
where
    T: DataType,
{
    fn from(nullable: Nullable<T>) -> Self {
        match nullable {
            Nullable::Null => Value::Null,
            Nullable::Value(v) => v.into(),
        }
    }
}

impl<T> Nullable<T>
where
    T: DataType,
{
    /// Checks if the nullable is null.
    pub fn is_null(&self) -> bool {
        matches!(self, Nullable::Null)
    }

    /// Checks if the nullable has a value.
    pub fn is_value(&self) -> bool {
        matches!(self, Nullable::Value(_))
    }

    /// Unwraps the nullable to get the inner value.
    ///
    /// # Panics
    ///
    /// Panics if the nullable is null.
    pub fn unwrap(&self) -> &T {
        match self {
            Nullable::Value(v) => v,
            Nullable::Null => panic!("Called unwrap on a Null Nullable"),
        }
    }

    /// Returns the inner value as an [`Option<T>`].
    pub fn as_opt(&self) -> Option<&T> {
        match self {
            Nullable::Value(v) => Some(v),
            Nullable::Null => None,
        }
    }

    /// Returns a mutable reference to the inner value as an [`Option<T>`].
    pub fn as_mut_opt(&mut self) -> Option<&mut T> {
        match self {
            Nullable::Value(v) => Some(v),
            Nullable::Null => None,
        }
    }

    /// Converts the nullable into an Option<T>.
    pub fn into_opt(self) -> Option<T> {
        self.into()
    }
}

impl<T> DataType for Nullable<T> where T: DataType {}

impl<T> From<Option<T>> for Nullable<T>
where
    T: DataType,
{
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => Nullable::Value(v),
            None => Nullable::Null,
        }
    }
}

impl<T> From<Nullable<T>> for Option<T>
where
    T: DataType,
{
    fn from(value: Nullable<T>) -> Self {
        match value {
            Nullable::Value(v) => Some(v),
            Nullable::Null => None,
        }
    }
}

impl<T> Encode for Nullable<T>
where
    T: DataType,
{
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        match self {
            Nullable::Null => std::borrow::Cow::Owned(vec![0]),
            Nullable::Value(v) => {
                let mut encoded = vec![1];
                encoded.extend_from_slice(&v.encode());
                std::borrow::Cow::Owned(encoded)
            }
        }
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.is_empty() {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let is_set = data[0];
        match is_set {
            0 => Ok(Nullable::Null),
            _ => {
                let value = T::decode(std::borrow::Cow::Owned(data[1..].to_vec()))?;
                Ok(Nullable::Value(value))
            }
        }
    }

    fn size(&self) -> crate::memory::MSize {
        match self {
            Nullable::Null => 1,
            Nullable::Value(v) => 1 + v.size(),
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::dbms::types::Int32;

    #[test]
    fn test_should_encode_and_decode_nullable_null() {
        let nullable: Nullable<Int32> = Nullable::Null;
        let encoded = nullable.encode();
        let decoded = Nullable::<Int32>::decode(encoded).unwrap();
        assert_eq!(nullable, decoded);
    }

    #[test]
    fn test_should_encode_and_decode_nullable_value() {
        let nullable: Nullable<Int32> = Nullable::Value(Int32::from(42));
        let encoded = nullable.encode();
        let decoded = Nullable::<Int32>::decode(encoded).unwrap();
        assert_eq!(nullable, decoded);
    }

    #[test]
    fn test_should_convert_between_option_and_nullable() {
        let some_value: Option<Int32> = Some(Int32::from(42));
        let nullable: Nullable<Int32> = some_value.into();
        assert_eq!(nullable, Nullable::Value(Int32::from(42)));
        let back_to_option: Option<Int32> = nullable.into();
        assert_eq!(some_value, back_to_option);

        let none_value: Option<Int32> = None;
        let nullable_none: Nullable<Int32> = none_value.into();
        assert_eq!(nullable_none, Nullable::Null);
        let back_to_option_none: Option<Int32> = nullable_none.into();
        assert_eq!(none_value, back_to_option_none);
    }

    #[test]
    fn test_should_check_is_null_and_is_value() {
        let nullable_null: Nullable<Int32> = Nullable::Null;
        assert!(nullable_null.is_null());
        assert!(!nullable_null.is_value());

        let nullable_value: Nullable<Int32> = Nullable::Value(Int32::from(42));
        assert!(!nullable_value.is_null());
        assert!(nullable_value.is_value());
    }

    #[test]
    fn test_should_unwrap_nullable_value() {
        let nullable_value: Nullable<Int32> = Nullable::Value(Int32::from(42));
        let value = nullable_value.unwrap();
        assert_eq!(*value, Int32::from(42));
    }

    #[test]
    #[should_panic(expected = "Called unwrap on a Null Nullable")]
    fn test_should_panic_on_unwrap_nullable_null() {
        let nullable_null: Nullable<Int32> = Nullable::Null;
        let _value = nullable_null.unwrap();
    }

    #[test]
    fn test_should_get_as_opt() {
        let nullable_value: Nullable<Int32> = Nullable::Value(Int32::from(42));
        let opt_value = nullable_value.as_opt();
        assert_eq!(opt_value, Some(&Int32::from(42)));
    }

    #[test]
    fn test_should_get_as_mut_opt() {
        let mut nullable_value: Nullable<Int32> = Nullable::Value(Int32::from(42));
        if let Some(value) = nullable_value.as_mut_opt() {
            *value = Int32::from(100);
        }
        assert_eq!(nullable_value, Nullable::Value(Int32::from(100)));
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = Nullable::Value(Int32::from(42));
        let buf = candid::encode_one(src).expect("Candid encoding failed");
        let decoded: Nullable<Int32> = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);

        let src_null: Nullable<Int32> = Nullable::Null;
        let buf_null = candid::encode_one(src_null).expect("Candid encoding failed");
        let decoded_null: Nullable<Int32> =
            candid::decode_one(&buf_null).expect("Candid decoding failed");
        assert_eq!(src_null, decoded_null);
    }

    #[test]
    fn test_should_default_nullable_to_null() {
        let default_nullable: Nullable<Int32> = Default::default();
        assert_eq!(default_nullable, Nullable::Null);
    }
}

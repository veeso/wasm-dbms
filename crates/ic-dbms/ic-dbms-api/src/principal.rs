use std::fmt;

use candid::CandidType;
use serde::{Deserialize, Serialize};
use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, DataType, DecodeError, Encode, MSize, MemoryError, MemoryResult,
    PageOffset,
};
use wasm_dbms_macros::CustomDataType;

/// Principal data type for the IC DBMS.
///
/// This is an IC-specific custom data type that wraps [`candid::Principal`].
/// Use with `#[custom_type]` annotation in table definitions.
#[derive(
    Clone, Debug, CustomDataType, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[type_tag = "principal"]
pub struct Principal(pub candid::Principal);

impl Default for Principal {
    fn default() -> Self {
        Self(candid::Principal::anonymous())
    }
}

impl CandidType for Principal {
    fn _ty() -> candid::types::Type {
        candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Principal))
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        candid::Principal::idl_serialize(&self.0, serializer)
    }
}

impl Encode for Principal {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let principal_bytes = self.0.as_slice();
        let mut bytes = Vec::with_capacity(1 + principal_bytes.len());
        let len = principal_bytes.len() as u8;
        bytes.push(len);
        bytes.extend_from_slice(principal_bytes);
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.is_empty() {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let buf_len = data[0] as usize;

        if data.len() < 1 + buf_len {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let principal = candid::Principal::try_from_slice(&data[1..1 + buf_len]).map_err(|e| {
            MemoryError::DecodeError(DecodeError::IdentityDecodeError(e.to_string()))
        })?;

        Ok(Self(principal))
    }

    fn size(&self) -> MSize {
        1 + self.0.as_slice().len() as MSize
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl DataType for Principal {}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{CustomDataType as _, Value};

    use super::*;

    #[test]
    fn test_principal_encode_decode() {
        let original = Principal(
            candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid principal"),
        );
        let encoded = original.encode();
        let decoded = Principal::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_principal_encode_decode_anonymous() {
        let original = Principal(candid::Principal::anonymous());
        let encoded = original.encode();
        let decoded = Principal::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_should_candid_encode_decode() {
        let src = Principal(
            candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid principal"),
        );
        let buf = candid::encode_one(&src).expect("Candid encoding failed");
        let decoded: Principal = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }

    #[test]
    fn test_decode_empty_data_returns_too_short() {
        let result = Principal::decode(std::borrow::Cow::Borrowed(&[]));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_decode_truncated_data_returns_too_short() {
        // Length prefix says 10 bytes, but only 2 bytes follow
        let data = vec![10, 0x01, 0x02];
        let result = Principal::decode(std::borrow::Cow::Owned(data));
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_size_returns_correct_value() {
        let principal = Principal(
            candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid principal"),
        );
        let size = principal.size();
        // 1 byte for length prefix + principal bytes length
        assert_eq!(size, 1 + principal.0.as_slice().len() as MSize);
    }

    #[test]
    fn test_size_anonymous_principal() {
        let principal = Principal(candid::Principal::anonymous());
        let size = principal.size();
        assert_eq!(size, 1 + principal.0.as_slice().len() as MSize);
    }

    #[test]
    fn test_display() {
        let inner =
            candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid principal");
        let principal = Principal(inner);
        let display = format!("{principal}");
        assert_eq!(display, "ryjl3-tyaaa-aaaaa-aaaba-cai");
    }

    #[test]
    fn test_default_is_anonymous() {
        let principal = Principal::default();
        assert_eq!(principal.0, candid::Principal::anonymous());
    }

    #[test]
    fn test_from_principal_to_value() {
        let principal = Principal(
            candid::Principal::from_text("ryjl3-tyaaa-aaaaa-aaaba-cai").expect("invalid principal"),
        );
        let value: Value = principal.into();
        assert!(matches!(value, Value::Custom(_)));
    }

    #[test]
    fn test_custom_data_type_tag() {
        assert_eq!(Principal::TYPE_TAG, "principal");
    }
}

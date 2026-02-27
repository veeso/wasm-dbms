use std::fmt;

use candid::CandidType;
use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, PageOffset};

/// Principal data type for the DBMS.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
        // put 2 bytes for length
        let len = principal_bytes.len() as u8;
        bytes.push(len);
        bytes.extend_from_slice(principal_bytes);
        std::borrow::Cow::Owned(bytes)
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

        // read length
        let buf_len = data[0] as usize;

        if data.len() < 1 + buf_len {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let principal = candid::Principal::try_from_slice(&data[1..1 + buf_len])?;

        Ok(Self(principal))
    }

    fn size(&self) -> crate::memory::MSize {
        1 + self.0.as_slice().len() as crate::memory::MSize
    }
}

impl fmt::Display for Principal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl DataType for Principal {}

impl crate::dbms::types::CustomDataType for Principal {
    const TYPE_TAG: &'static str = "principal";
}

impl From<Principal> for crate::dbms::value::Value {
    fn from(val: Principal) -> crate::dbms::value::Value {
        crate::dbms::value::Value::Custom(crate::dbms::custom_value::CustomValue::new(&val))
    }
}

#[cfg(test)]
mod tests {

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
}

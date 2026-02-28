use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DataSize, Encode, PageOffset};

/// Boolean data type for the DBMS.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct Boolean(pub bool);

impl fmt::Display for Boolean {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Encode for Boolean {
    const SIZE: DataSize = DataSize::Fixed(1);

    const ALIGNMENT: PageOffset = 1;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Owned(vec![self.0 as u8])
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

        Ok(Self(data[0] != 0))
    }

    fn size(&self) -> crate::memory::MSize {
        Self::SIZE.get_fixed_size().expect("should be fixed")
    }
}

#[cfg(feature = "candid")]
impl candid::CandidType for Boolean {
    fn _ty() -> candid::types::Type {
        candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Bool))
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        serializer.serialize_bool(self.0)
    }
}

impl DataType for Boolean {}

impl From<bool> for Boolean {
    fn from(value: bool) -> Self {
        Boolean(value)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_boolean_encode_decode() {
        let value = Boolean(true);
        let encoded = value.encode();
        let decoded = Boolean::decode(encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = Boolean(true);
        let buf = candid::encode_one(src).expect("Candid encoding failed");
        let decoded: Boolean = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }
}

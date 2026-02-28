use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DataSize, Encode, MSize, MemoryError, PageOffset};

const UUID_SIZE: usize = 16;

/// UUID data type for the DBMS.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid(pub uuid::Uuid);

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "candid")]
impl candid::CandidType for Uuid {
    fn _ty() -> candid::types::Type {
        candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Vec(
            candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Nat8)),
        )))
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        let bytes = self.0.as_bytes();
        serializer.serialize_blob(bytes)
    }
}

impl Serialize for Uuid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let bytes = self.0.as_bytes();
        serializer.serialize_bytes(bytes)
    }
}

impl<'de> Deserialize<'de> for Uuid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // deserialize bytes
        let bytes: &[u8] = serde::Deserialize::deserialize(deserializer)?;
        if bytes.len() != UUID_SIZE {
            return Err(serde::de::Error::custom("Invalid UUID length"));
        }
        Ok(Uuid(uuid::Uuid::from_bytes(bytes.try_into().unwrap())))
    }
}

impl Encode for Uuid {
    const SIZE: DataSize = DataSize::Fixed(UUID_SIZE as MSize);

    const ALIGNMENT: PageOffset = UUID_SIZE as MSize;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        std::borrow::Cow::Borrowed(self.0.as_bytes())
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() != UUID_SIZE {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        uuid::Uuid::from_slice(&data)
            .map(Uuid)
            .map_err(MemoryError::from)
    }

    fn size(&self) -> MSize {
        Self::SIZE.get_fixed_size().expect("Should be fixed size")
    }
}

impl DataType for Uuid {}

#[cfg(test)]
mod tests {

    use uuid::{NoContext, Timestamp};

    use super::*;

    #[test]
    fn test_uuid_encode_decode() {
        let original_uuid = Uuid(uuid::Uuid::new_v7(Timestamp::from_unix(
            NoContext, 1497624119, 1234,
        )));
        let encoded = original_uuid.encode();
        let decoded = Uuid::decode(encoded).unwrap();
        assert_eq!(original_uuid, decoded)
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_uuid_candid_serialization() {
        let original_uuid = Uuid(uuid::Uuid::new_v7(Timestamp::from_unix(
            NoContext, 1497624119, 1234,
        )));
        let bytes = candid::encode_one(&original_uuid).unwrap();
        let decoded_uuid: Uuid = candid::decode_one(&bytes).unwrap();
        assert_eq!(original_uuid, decoded_uuid);
    }
}

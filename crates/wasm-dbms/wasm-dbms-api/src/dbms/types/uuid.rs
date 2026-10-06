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
        deserializer.deserialize_bytes(UuidBytesVisitor)
    }
}

/// Visitor accepting the 16 UUID bytes either as a byte buffer or as a sequence of `u8`,
/// so that every format can read back what [`Uuid`]'s [`Serialize`] implementation emits.
struct UuidBytesVisitor;

impl<'de> serde::de::Visitor<'de> for UuidBytesVisitor {
    type Value = Uuid;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "{UUID_SIZE} UUID bytes")
    }

    fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let bytes: [u8; UUID_SIZE] = bytes
            .try_into()
            .map_err(|_| E::invalid_length(bytes.len(), &self))?;
        Ok(Uuid(uuid::Uuid::from_bytes(bytes)))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut bytes = [0u8; UUID_SIZE];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = seq
                .next_element()?
                .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
        }
        if seq.next_element::<u8>()?.is_some() {
            return Err(serde::de::Error::invalid_length(UUID_SIZE + 1, &self));
        }
        Ok(Uuid(uuid::Uuid::from_bytes(bytes)))
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
        // Only the leading bytes belong to this value: a record decoder passes the rest of the
        // record, so the input may continue with the following columns.
        if data.len() < UUID_SIZE {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        uuid::Uuid::from_slice(&data[..UUID_SIZE])
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

    #[test]
    fn test_uuid_decode_ignores_trailing_bytes() {
        let original_uuid = Uuid(uuid::Uuid::new_v7(Timestamp::from_unix(
            NoContext, 1497624119, 1234,
        )));
        let mut bytes = original_uuid.encode().into_owned();
        bytes.extend_from_slice(&[0xff; 5]);
        let decoded = Uuid::decode(std::borrow::Cow::Owned(bytes)).unwrap();
        assert_eq!(original_uuid, decoded);
    }

    #[test]
    fn test_uuid_decode_rejects_short_input() {
        let bytes = vec![0u8; UUID_SIZE - 1];
        let result = Uuid::decode(std::borrow::Cow::Owned(bytes));
        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort
            ))
        ));
    }

    #[test]
    fn test_uuid_serde_json_round_trip() {
        let original_uuid = Uuid(
            uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").expect("valid uuid"),
        );
        let json = serde_json::to_string(&original_uuid).expect("serialize uuid");
        let decoded: Uuid = serde_json::from_str(&json).expect("deserialize uuid");
        assert_eq!(original_uuid, decoded);
    }

    #[test]
    fn test_uuid_serde_json_rejects_wrong_length() {
        let too_short = serde_json::to_string(&[0u8; 15]).expect("serialize bytes");
        let too_long = serde_json::to_string(&[0u8; 17]).expect("serialize bytes");
        assert!(serde_json::from_str::<Uuid>(&too_short).is_err());
        assert!(serde_json::from_str::<Uuid>(&too_long).is_err());
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

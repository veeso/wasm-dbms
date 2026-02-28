use std::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, PageOffset};

/// Blob data type for the DBMS.
#[derive(Clone, Debug, PartialEq, Default, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct Blob(pub Vec<u8>);

impl fmt::Display for Blob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Blob(len={})", self.0.len())
    }
}

impl Encode for Blob {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(2 + self.0.len());
        // put 2 bytes for length
        let len = self.0.len() as u16;
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(self.0.as_slice());
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < 2 {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        // read length
        let buf_len = {
            let mut len_bytes = [0u8; 2];
            len_bytes.copy_from_slice(&data[0..2]);
            u16::from_le_bytes(len_bytes) as usize
        };

        if data.len() < 2 + buf_len {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let bytes = data[2..2 + buf_len].to_vec();

        Ok(Self(bytes))
    }

    fn size(&self) -> crate::memory::MSize {
        2 + self.0.len() as crate::memory::MSize
    }
}

impl From<Vec<u8>> for Blob {
    fn from(s: Vec<u8>) -> Self {
        Blob(s)
    }
}

impl From<&[u8]> for Blob {
    fn from(s: &[u8]) -> Self {
        Blob(s.to_vec())
    }
}

impl DataType for Blob {}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_blob_encode_decode() {
        let original = Blob(vec![1, 2, 3, 4, 5]);
        let encoded = original.encode();
        let decoded = Blob::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = Blob(vec![10, 20, 30, 40, 50]);
        let buf = candid::encode_one(&src).expect("Candid encoding failed");
        let decoded: Blob = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }
}

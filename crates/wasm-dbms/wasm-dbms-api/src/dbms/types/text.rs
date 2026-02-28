use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DEFAULT_ALIGNMENT, DataSize, Encode, PageOffset};

/// Text data type for the DBMS.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct Text(pub String);

impl Text {
    /// Returns the string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Encode for Text {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(2 + self.0.len());
        // put 2 bytes for length
        let len = self.0.len() as u16;
        bytes.extend_from_slice(&len.to_le_bytes());
        bytes.extend_from_slice(self.0.as_bytes());
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
        let str_len = {
            let mut len_bytes = [0u8; 2];
            len_bytes.copy_from_slice(&data[0..2]);
            u16::from_le_bytes(len_bytes) as usize
        };

        if data.len() < 2 + str_len {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let string_bytes = &data[2..2 + str_len];
        let string = String::from_utf8(string_bytes.to_vec())?;

        Ok(Self(string))
    }

    fn size(&self) -> crate::memory::MSize {
        2 + self.0.len() as crate::memory::MSize
    }
}

impl FromStr for Text {
    type Err = std::string::FromUtf8Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Text(s.to_string()))
    }
}

impl From<String> for Text {
    fn from(s: String) -> Self {
        Text(s)
    }
}

impl From<&str> for Text {
    fn from(s: &str) -> Self {
        Text(s.to_string())
    }
}

impl DataType for Text {}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_text_encode_decode() {
        let original = Text("Hello, World!".to_string());
        let encoded = original.encode();
        let decoded = Text::decode(encoded).unwrap();
        assert_eq!(original, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = Text("Hello, World!".to_string());
        let buf = candid::encode_one(&src).expect("Candid encoding failed");
        let decoded: Text = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }
}

use rust_decimal::Decimal as RustDecimal;
use serde::{Deserialize, Serialize};

use crate::memory::{DataSize, DecodeError, Encode, MSize, PageOffset};

const RUST_DECIMAL_ENCODE_SIZE: MSize = 16;

/// Decimal data type for the DBMS.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Decimal(pub RustDecimal);

impl From<RustDecimal> for Decimal {
    fn from(value: RustDecimal) -> Self {
        Decimal(value)
    }
}

impl From<Decimal> for RustDecimal {
    fn from(value: Decimal) -> Self {
        value.0
    }
}

impl std::fmt::Display for Decimal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(feature = "candid")]
impl candid::CandidType for Decimal {
    fn _ty() -> candid::types::Type {
        candid::types::Type(std::rc::Rc::new(candid::types::TypeInner::Text))
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        serializer.serialize_text(&self.0.to_string())
    }
}

impl Serialize for Decimal {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Decimal {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let decimal = RustDecimal::from_str_exact(&s).map_err(serde::de::Error::custom)?;
        Ok(Decimal(decimal))
    }
}

impl Encode for Decimal {
    const SIZE: DataSize = DataSize::Fixed(RUST_DECIMAL_ENCODE_SIZE);

    const ALIGNMENT: PageOffset = RUST_DECIMAL_ENCODE_SIZE;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let buf = self.0.serialize();
        std::borrow::Cow::Owned(buf.to_vec())
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() != RUST_DECIMAL_ENCODE_SIZE as usize {
            return Err(crate::memory::MemoryError::DecodeError(
                DecodeError::TooShort,
            ));
        }

        let buff: [u8; RUST_DECIMAL_ENCODE_SIZE as usize] = data
            [..RUST_DECIMAL_ENCODE_SIZE as usize]
            .as_ref()
            .try_into()
            .map_err(|_| crate::memory::MemoryError::DecodeError(DecodeError::TooShort))?;

        Ok(Self::from(RustDecimal::deserialize(buff)))
    }

    fn size(&self) -> crate::memory::MSize {
        Self::SIZE.get_fixed_size().expect("should be fixed size")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_decimal_encode_decode() {
        let original_decimal = Decimal(RustDecimal::new(12345, 2)); // Represents 123.45
        let encoded = original_decimal.encode();
        let decoded = Decimal::decode(encoded).expect("Decoding failed");
        assert_eq!(original_decimal, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let original_decimal = Decimal(RustDecimal::new(67890, 3)); // Represents 67.890
        let buf = candid::encode_one(original_decimal).expect("Candid encoding failed");
        let decoded: Decimal = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(original_decimal, decoded);
    }
}

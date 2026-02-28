use core::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DataSize, Encode, PageOffset};

/// Date data type for the DBMS.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl DataType for Date {}

impl Encode for Date {
    const SIZE: DataSize = DataSize::Fixed(4);

    const ALIGNMENT: PageOffset = 4;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(4);
        bytes.extend_from_slice(&self.year.to_le_bytes());
        bytes.push(self.month);
        bytes.push(self.day);
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < 4 {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let year = u16::from_le_bytes([data[0], data[1]]);
        let month = data[2];
        let day = data[3];

        Ok(Self { year, month, day })
    }

    fn size(&self) -> crate::memory::MSize {
        Self::SIZE.get_fixed_size().expect("should be fixed")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_date_encode_decode() {
        let value = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let encoded = value.encode();
        let decoded = Date::decode(encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_date_display() {
        let date = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        assert_eq!(date.to_string(), "2024-06-15");
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let buf = candid::encode_one(src).expect("Candid encoding failed");
        let decoded: Date = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }

    #[test]
    fn test_should_compare_dates() {
        let date1 = Date {
            year: 2024,
            month: 6,
            day: 15,
        };
        let date2 = Date {
            year: 2024,
            month: 6,
            day: 16,
        };
        let date3 = Date {
            year: 2024,
            month: 7,
            day: 1,
        };
        let date4 = Date {
            year: 2025,
            month: 1,
            day: 1,
        };

        assert!(date1 < date2);
        assert!(date2 < date3);
        assert!(date3 < date4);
        assert!(date4 > date1);
    }
}

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::dbms::types::DataType;
use crate::memory::{DataSize, Encode, MSize, PageOffset};

const TYPE_SIZE: usize = 2 + 1 + 1 + 1 + 1 + 1 + 4 + 2; // year + month + day + hour + minute + second + microsecond + timezone_offset_minutes

/// Date time data type for the DBMS.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub microsecond: u32,
    pub timezone_offset_minutes: i16,
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}{:+03}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.microsecond,
            self.timezone_offset_minutes / 60,
            self.timezone_offset_minutes % 60
        )
    }
}

impl DataType for DateTime {}

impl Encode for DateTime {
    const SIZE: DataSize = DataSize::Fixed(TYPE_SIZE as MSize);

    const ALIGNMENT: PageOffset = TYPE_SIZE as MSize;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(TYPE_SIZE);
        bytes.extend_from_slice(&self.year.to_le_bytes());
        bytes.push(self.month);
        bytes.push(self.day);
        bytes.push(self.hour);
        bytes.push(self.minute);
        bytes.push(self.second);
        bytes.extend_from_slice(&self.microsecond.to_le_bytes());
        bytes.extend_from_slice(&self.timezone_offset_minutes.to_le_bytes());
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> crate::memory::MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < TYPE_SIZE {
            return Err(crate::memory::MemoryError::DecodeError(
                crate::memory::DecodeError::TooShort,
            ));
        }

        let year = u16::from_le_bytes([data[0], data[1]]);
        let month = data[2];
        let day = data[3];
        let hour = data[4];
        let minute = data[5];
        let second = data[6];
        let microsecond = u32::from_le_bytes([data[7], data[8], data[9], data[10]]);
        let timezone_offset_minutes = i16::from_le_bytes([data[11], data[12]]);

        Ok(Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
            microsecond,
            timezone_offset_minutes,
        })
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
        let value = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone_offset_minutes: -120,
        };
        let encoded = value.encode();
        let decoded = DateTime::decode(encoded).unwrap();
        assert_eq!(value, decoded);
    }

    #[test]
    fn test_date_display() {
        let date = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone_offset_minutes: -120,
        };
        assert_eq!(date.to_string(), "2024-06-15T12:30:45.123456-02:00");
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_encode_decode() {
        let src = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone_offset_minutes: -120,
        };
        let buf = candid::encode_one(src).expect("Candid encoding failed");
        let decoded: DateTime = candid::decode_one(&buf).expect("Candid decoding failed");
        assert_eq!(src, decoded);
    }

    #[test]
    fn test_should_compare_datetimes() {
        let dt1 = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123456,
            timezone_offset_minutes: -120,
        };
        let dt2 = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            microsecond: 123457,
            timezone_offset_minutes: -120,
        };
        let dt3 = DateTime {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 31,
            second: 0,
            microsecond: 0,
            timezone_offset_minutes: -120,
        };
        assert!(dt1 < dt2);
        assert!(dt2 < dt3);
        assert!(dt1 < dt3);
    }
}

//! Module describing the type for the location of a record in the table registry.

use std::borrow::Cow;
use std::cmp::Ordering;

use wasm_dbms_api::memory::{DataSize, DecodeError, Encode, MSize, MemoryError, MemoryResult};
use wasm_dbms_api::prelude::{Page, PageOffset};

/// Serialized record pointer size.
const RECORD_POINTER_SIZE: usize = 6;

/// The address of a record in the table registry, consisting of a page number and an offset within that page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordAddress {
    pub page: Page,
    pub offset: PageOffset,
}

impl RecordAddress {
    /// Creates a new [`RecordAddress`] with the given page and offset.
    pub fn new(page: Page, offset: PageOffset) -> Self {
        Self { page, offset }
    }
}

impl PartialOrd for RecordAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RecordAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        self.page
            .cmp(&other.page)
            .then(self.offset.cmp(&other.offset))
    }
}

impl Encode for RecordAddress {
    const SIZE: DataSize = DataSize::Fixed(RECORD_POINTER_SIZE as MSize);
    const ALIGNMENT: PageOffset = RECORD_POINTER_SIZE as PageOffset;

    fn encode(&'_ self) -> Cow<'_, [u8]> {
        let mut buf = [0u8; RECORD_POINTER_SIZE];
        buf[0..4].copy_from_slice(&self.page.to_le_bytes());
        buf[4..6].copy_from_slice(&self.offset.to_le_bytes());
        Cow::Owned(buf.to_vec())
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < RECORD_POINTER_SIZE {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        Ok(Self {
            page: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
            offset: u16::from_le_bytes([data[4], data[5]]),
        })
    }

    fn size(&self) -> MSize {
        RECORD_POINTER_SIZE as MSize
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_create_record_pointer() {
        let ptr = RecordAddress::new(42, 128);
        assert_eq!(ptr.page, 42);
        assert_eq!(ptr.offset, 128);
    }

    #[test]
    fn test_record_pointer_encode_decode() {
        let ptr = RecordAddress {
            page: 42,
            offset: 128,
        };
        let encoded = ptr.encode();
        let decoded = RecordAddress::decode(encoded).expect("record pointer decode failed");
        assert_eq!(decoded, ptr);
    }

    #[test]
    fn test_record_pointer_ordering() {
        let a = RecordAddress { page: 1, offset: 2 };
        let b = RecordAddress { page: 1, offset: 3 };
        let c = RecordAddress { page: 2, offset: 0 };
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn test_record_pointer_byte_layout() {
        let ptr = RecordAddress {
            page: 0x0102_0304,
            offset: 0x0506,
        };
        let encoded = ptr.encode();
        assert_eq!(&*encoded, &[0x04, 0x03, 0x02, 0x01, 0x06, 0x05]);
    }
}

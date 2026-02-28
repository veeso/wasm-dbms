// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{
    DataSize, DecodeError, Encode, MSize, MemoryError, MemoryResult, PageOffset,
};

/// Each record is prefixed with its length encoded in 2 bytes.
pub const RAW_RECORD_HEADER_SIZE: MSize = 2;

/// A raw record stored in memory, consisting of its length and data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecord<E>
where
    E: Encode,
{
    length: MSize,
    pub data: E,
}

impl<E> RawRecord<E>
where
    E: Encode,
{
    /// Creates a new raw record from the given data.
    pub fn new(data: E) -> Self {
        let length = data.size();
        Self { length, data }
    }
}

impl<E> Encode for RawRecord<E>
where
    E: Encode,
{
    const SIZE: DataSize = if let DataSize::Fixed(size) = E::SIZE {
        DataSize::Fixed(RAW_RECORD_HEADER_SIZE + size)
    } else {
        DataSize::Dynamic
    };

    const ALIGNMENT: PageOffset = if let DataSize::Fixed(size) = E::SIZE {
        size + RAW_RECORD_HEADER_SIZE
    } else {
        E::ALIGNMENT
    };

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut encoded = Vec::with_capacity(self.size() as usize);
        encoded.extend_from_slice(&self.length.to_le_bytes());
        encoded.extend_from_slice(&self.data.encode());
        std::borrow::Cow::Owned(encoded)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < 2 {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let length = u16::from_le_bytes([data[0], data[1]]) as MSize;
        if data.len() < (RAW_RECORD_HEADER_SIZE as usize) + length as usize {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let data_slice = &data[(RAW_RECORD_HEADER_SIZE as usize)
            ..(RAW_RECORD_HEADER_SIZE as usize) + length as usize];
        let data_cow = std::borrow::Cow::Borrowed(data_slice);
        let data_decoded = E::decode(data_cow)?;
        Ok(Self {
            length,
            data: data_decoded,
        })
    }

    fn size(&self) -> MSize {
        RAW_RECORD_HEADER_SIZE + self.length // 1 (start) + 2 bytes for length + data size
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_raw_record_encode_decode() {
        let record = TestRecord { a: 42, b: 65535 };
        let raw_record = RawRecord::new(record);
        let encoded = raw_record.encode();
        let decoded = RawRecord::<TestRecord>::decode(encoded).unwrap();
        assert_eq!(raw_record.length, decoded.length);
        assert_eq!(raw_record.data, decoded.data);
    }

    #[derive(Debug, PartialEq, Clone, Copy)]
    struct TestRecord {
        a: u8,
        b: u16,
    }

    impl Encode for TestRecord {
        const SIZE: DataSize = DataSize::Fixed(3);

        const ALIGNMENT: PageOffset = 3;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![self.a, (self.b & 0xFF) as u8, (self.b >> 8) as u8])
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            if data.len() != 3 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let a = data[0];
            let b = u16::from_le_bytes([data[1], data[2]]);
            Ok(Self { a, b })
        }

        fn size(&self) -> MSize {
            3
        }
    }
}

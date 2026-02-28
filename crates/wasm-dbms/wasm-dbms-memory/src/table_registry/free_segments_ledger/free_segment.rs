// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{DataSize, Encode, MSize, MemoryResult, Page, PageOffset};

/// Represents a free segment's metadata.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FreeSegment {
    /// The page where the free segment was located.
    pub page: Page,
    /// The offset within the page where the free segment was located.
    pub offset: PageOffset,
    /// The size of the free segment.
    pub size: MSize,
}

impl Encode for FreeSegment {
    const SIZE: DataSize = DataSize::Fixed(8); // page (4) + offset (2) + size (2)

    const ALIGNMENT: PageOffset = 8;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut buffer = Vec::with_capacity(self.size() as usize);

        buffer.extend_from_slice(&self.page.to_le_bytes());
        buffer.extend_from_slice(&self.offset.to_le_bytes());
        buffer.extend_from_slice(&self.size.to_le_bytes());
        std::borrow::Cow::Owned(buffer)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let page = Page::from_le_bytes(data[0..4].try_into()?);
        let offset = PageOffset::from_le_bytes(data[4..6].try_into()?);
        let size = MSize::from_le_bytes(data[6..8].try_into()?);

        Ok(FreeSegment { page, offset, size })
    }

    fn size(&self) -> MSize {
        Self::SIZE.get_fixed_size().expect("Should be fixed")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_should_encode_and_decode_free_segment() {
        let original_record = FreeSegment {
            page: 42,
            offset: 1000,
            size: 256,
        };

        assert_eq!(original_record.size(), 8);
        let encoded = original_record.encode();
        let decoded = FreeSegment::decode(encoded).expect("Decoding failed");

        assert_eq!(original_record, decoded);
    }
}

// Rust guideline compliant 2026-02-28

use std::borrow::Cow;

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryResult, Page, PageOffset,
};

const TABLE_LEN_SIZE: usize = 2;

/// A wrapper around a vector of pages containing the [`FreeSegmentsTable`]s.
#[derive(Debug, Default, Clone)]
pub struct PagesTable(Vec<Page>);

impl PagesTable {
    /// Adds a new page to the pages table.
    pub fn push(&mut self, page: Page) {
        self.0.push(page);
    }

    pub fn pages(&self) -> &[Page] {
        &self.0
    }
}

impl Encode for PagesTable {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&self) -> Cow<'_, [u8]> {
        let mut vec = Vec::with_capacity(self.size() as usize);
        vec.extend_from_slice(&(self.0.len() as u16).to_le_bytes());
        for page in &self.0 {
            vec.extend_from_slice(&page.to_le_bytes());
        }

        Cow::Owned(vec)
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let len_bytes = &data[..TABLE_LEN_SIZE];
        let table_len = u16::from_le_bytes(len_bytes.try_into()?) as usize;

        let mut pages = Vec::with_capacity(table_len);
        let mut offset = TABLE_LEN_SIZE;
        for _ in 0..table_len {
            let page_bytes = &data[offset..offset + size_of::<Page>()];
            let page = Page::from_le_bytes(page_bytes.try_into()?);
            pages.push(page);
            offset += size_of::<Page>();
        }

        Ok(PagesTable(pages))
    }

    fn size(&self) -> MSize {
        (TABLE_LEN_SIZE + (self.0.len() * size_of::<Page>())) as MSize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pages_table_encode() {
        let pages = vec![1, 2, 3, 4, 5];
        let pages_table = PagesTable(pages.clone());
        let encoded = pages_table.encode();
        let expected_size = TABLE_LEN_SIZE + pages.len() * size_of::<Page>();
        assert_eq!(encoded.len(), expected_size);

        // decode
        let decoded = PagesTable::decode(encoded).unwrap();
        assert_eq!(decoded.0, pages);
    }

    #[test]
    fn test_pages_table_empty_encode() {
        let pages_table = PagesTable::default();
        let encoded = pages_table.encode();
        let expected_size = TABLE_LEN_SIZE;
        assert_eq!(encoded.len(), expected_size);
    }

    #[test]
    fn test_should_iter_pages_table() {
        let pages = vec![10, 20, 30];
        let pages_table = PagesTable(pages.clone());
        let mut iter = pages_table.pages().iter();

        for page in pages {
            assert_eq!(iter.next(), Some(&page));
        }
        assert_eq!(iter.next(), None);
    }
}

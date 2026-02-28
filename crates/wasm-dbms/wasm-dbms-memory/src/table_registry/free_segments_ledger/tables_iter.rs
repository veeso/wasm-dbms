// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{MemoryResult, Page};

use crate::{MemoryManager, MemoryProvider};

use super::FreeSegmentsTable;

/// An iterator which yields all the [`FreeSegmentsTable`]s.
pub struct TablesIter<'a, P>
where
    P: MemoryProvider,
{
    /// Tracks the current index.
    index: usize,
    /// Reference to the memory manager.
    mm: &'a MemoryManager<P>,
    /// The pages to iterate over.
    pages: &'a [Page],
}

impl<'a, P> TablesIter<'a, P>
where
    P: MemoryProvider,
{
    /// Creates a new [`TablesIter`].
    pub fn new(pages: &'a [Page], mm: &'a MemoryManager<P>) -> Self {
        Self {
            index: 0,
            mm,
            pages,
        }
    }
}

impl<P> Iterator for TablesIter<'_, P>
where
    P: MemoryProvider,
{
    type Item = MemoryResult<FreeSegmentsTable>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.pages.len() {
            return None;
        }
        let page = self.pages[self.index];
        self.index += 1;

        // read next page
        Some(FreeSegmentsTable::load(page, self.mm))
    }
}

#[cfg(test)]
mod tests {
    use crate::HeapMemoryProvider;

    use super::*;

    #[test]
    fn test_tables_iter_empty() {
        let mm = MemoryManager::init(HeapMemoryProvider::default());
        let pages = vec![];
        let mut iter = TablesIter::new(&pages, &mm);
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_should_iter_tables() {
        const COUNT: usize = 5;
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut pages = Vec::new();
        for _ in 0..COUNT {
            let page = mm.allocate_page().expect("Failed to allocate page");
            let mut table = FreeSegmentsTable::load(page, &mm).expect("Failed to load page");
            // insert a segment
            table
                .insert_free_segment(100 + page as Page, 0, 50, &mut mm)
                .expect("Failed to insert segment");
            pages.push(page);
        }

        let mut iter = TablesIter::new(&pages, &mm);
        for expected_page in &pages {
            let table_result = iter.next();
            assert!(table_result.is_some());
            let table = table_result.unwrap().expect("Failed to load table");
            // should have a segment
            let segment = table.find(|_| true).expect("Failed to find segment");
            assert_eq!(segment.page, expected_page + 100);
        }
        assert!(iter.next().is_none());
    }
}

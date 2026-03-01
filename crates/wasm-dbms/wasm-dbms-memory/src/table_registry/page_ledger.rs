// Rust guideline compliant 2026-02-28

mod page_table;

use wasm_dbms_api::prelude::{Encode, MemoryResult, Page, PageOffset};

pub use self::page_table::PageRecord;
use self::page_table::PageTable;
use super::raw_record::RawRecord;
use crate::{MemoryAccess, align_up};

/// Takes care of storing the pages for each table
#[derive(Debug)]
pub struct PageLedger {
    /// The page where the ledger is stored in memory.
    ledger_page: Page,
    /// The pages table.
    pages: PageTable,
}

impl PageLedger {
    /// Load the page ledger from memory at the given [`Page`].
    pub fn load(page: Page, mm: &impl MemoryAccess) -> MemoryResult<Self> {
        Ok(Self {
            pages: mm.read_at(page, 0)?,
            ledger_page: page,
        })
    }

    /// Get the page number and the offset to store the next record.
    ///
    /// It usually returns the first page with enough free space.
    /// If the provided record is larger than any page's free space,
    /// it allocates a new page and returns it.
    pub fn get_page_and_offset_for_record<R>(
        &mut self,
        record: &R,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<(Page, PageOffset)>
    where
        R: Encode,
    {
        let required_size = record.size() as u64;
        let page_size = mm.page_size();
        // check if record can fit in a page
        if required_size > page_size {
            return Err(wasm_dbms_api::prelude::MemoryError::DataTooLarge {
                page_size,
                requested: required_size,
            });
        }

        // iter ledger pages to find a page with enough free space
        let next_page = self.pages.pages.iter().find(|page_record| {
            let taken = page_size.saturating_sub(page_record.free);
            taken + required_size <= page_size
        });
        // if page found, return it
        if let Some(page_record) = next_page {
            // NOTE: since `page_record.free` is already aligned, we don't need to recalculate alignment here
            let offset = page_size.saturating_sub(page_record.free) as PageOffset;
            return Ok((page_record.page, offset));
        }

        // otherwise allocate a new one
        let new_page = mm.allocate_page()?;
        // add to ledger
        self.pages.pages.push(PageRecord {
            page: new_page,
            free: page_size, // NOTE: we commit later, so full free space
        });

        Ok((new_page, 0))
    }

    /// Commits the allocation of a record in the given page.
    ///
    /// This will commit the eventual allocated page
    /// and decrease the free space available in the page and write the updated ledger to memory.
    pub fn commit<R>(
        &mut self,
        page: Page,
        record: &R,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        R: Encode,
    {
        if let Some(page_record) = self.pages.pages.iter_mut().find(|pr| pr.page == page) {
            let record_size = record.size() as u64;
            if page_record.free < record_size {
                return Err(wasm_dbms_api::prelude::MemoryError::DataTooLarge {
                    page_size: page_record.free,
                    requested: record_size,
                });
            }
            // add padding to record size
            let padding = align_up::<RawRecord<R>>(record_size as usize);
            // add record size + required padding
            let record_size = record_size + ((padding as u64).saturating_sub(record.size() as u64));
            page_record.free = page_record.free.saturating_sub(record_size);
            self.write(mm)?;
            return Ok(());
        }

        Err(wasm_dbms_api::prelude::MemoryError::OutOfBounds)
    }

    /// Returns the list of pages in the ledger.
    pub fn pages(&self) -> &[PageRecord] {
        &self.pages.pages
    }

    /// Write the page ledger to memory.
    fn write(&self, mm: &mut impl MemoryAccess) -> MemoryResult<()> {
        mm.write_at(self.ledger_page, 0, &self.pages)
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{DataSize, MSize, MemoryResult};

    use super::super::raw_record::RAW_RECORD_HEADER_SIZE;
    use super::page_table::PageRecord;
    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager, MemoryProvider};

    #[test]
    fn test_should_store_pages_and_load_back() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().unwrap();
        let page_ledger = PageLedger {
            pages: PageTable {
                pages: vec![
                    PageRecord {
                        page: 10,
                        free: 100,
                    },
                    PageRecord {
                        page: 11,
                        free: 200,
                    },
                    PageRecord {
                        page: 12,
                        free: 300,
                    },
                ],
            },
            ledger_page: page,
        };
        page_ledger
            .write(&mut mm)
            .expect("failed to write page ledger");
        let loaded_ledger = PageLedger::load(page, &mm).expect("failed to load page ledger");
        assert_eq!(page_ledger.pages.pages, loaded_ledger.pages.pages);
    }

    #[test]
    fn test_should_get_page_for_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        // allocate page
        let ledger_page = mm.allocate_page().expect("failed to allocate ledger page");
        let mut page_ledger =
            PageLedger::load(ledger_page, &mm).expect("failed to load page ledger");
        assert!(page_ledger.pages.pages.is_empty());

        // create test record
        let record = TestRecord { data: [1; 100] };
        // get page for record
        let (page, offset) = page_ledger
            .get_page_and_offset_for_record(&record, &mut mm)
            .expect("failed to get page for record");
        assert_eq!(page_ledger.pages.pages.len(), 1);
        assert_eq!((page_ledger.pages.pages[0].page, 0), (page, offset));
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE
        );

        // commit record allocation
        page_ledger
            .commit(page, &record, &mut mm)
            .expect("failed to commit record allocation");
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE - 100 - RAW_RECORD_HEADER_SIZE as u64
        );

        // reload
        let reloaded_ledger =
            PageLedger::load(ledger_page, &mm).expect("failed to load page ledger");
        assert_eq!(page_ledger.pages.pages, reloaded_ledger.pages.pages);
    }

    #[test]
    fn test_should_get_page_with_offset() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        // allocate page
        let ledger_page = mm.allocate_page().expect("failed to allocate ledger page");
        let mut page_ledger =
            PageLedger::load(ledger_page, &mm).expect("failed to load page ledger");
        assert!(page_ledger.pages.pages.is_empty());

        // create test record
        let record = TestRecord { data: [1; 100] };
        // get page for record
        let (page, offset) = page_ledger
            .get_page_and_offset_for_record(&record, &mut mm)
            .expect("failed to get page for record");
        assert_eq!(page_ledger.pages.pages.len(), 1);
        assert_eq!((page_ledger.pages.pages[0].page, 0), (page, offset));
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE
        );

        // commit record allocation
        page_ledger
            .commit(page, &record, &mut mm)
            .expect("failed to commit record allocation");
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE - 100 - RAW_RECORD_HEADER_SIZE as u64
        );

        // get page for another record
        let (page, offset) = page_ledger
            .get_page_and_offset_for_record(&record, &mut mm)
            .expect("failed to get page for record");
        assert_eq!(page_ledger.pages.pages.len(), 1);
        assert_eq!(
            (
                page_ledger.pages.pages[0].page,
                100 + RAW_RECORD_HEADER_SIZE
            ),
            (page, offset)
        );
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE - 100 - RAW_RECORD_HEADER_SIZE as u64
        );
    }

    #[test]
    fn test_should_account_for_padding_on_commit() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        // allocate page
        let ledger_page = mm.allocate_page().expect("failed to allocate ledger page");
        let mut page_ledger =
            PageLedger::load(ledger_page, &mm).expect("failed to load page ledger");
        assert!(page_ledger.pages.pages.is_empty());

        // create test record with 32 bytes alignment
        let record = RecordWith32BytesPadding { data: [1; 100] };
        // get page for record
        let (page, offset) = page_ledger
            .get_page_and_offset_for_record(&record, &mut mm)
            .expect("failed to get page for record");
        assert_eq!(page_ledger.pages.pages.len(), 1);
        assert_eq!((page_ledger.pages.pages[0].page, 0), (page, offset));
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE
        );

        // commit record allocation
        page_ledger
            .commit(page, &record, &mut mm)
            .expect("failed to commit record allocation");
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE - 128
        );

        // commit another to align
        page_ledger
            .commit(page, &record, &mut mm)
            .expect("failed to commit record allocation");
        assert_eq!(
            page_ledger.pages.pages[0].free,
            HeapMemoryProvider::PAGE_SIZE - 128 - 128
        );
    }

    #[derive(Debug, Clone)]
    struct TestRecord {
        data: [u8; 100],
    }

    impl Encode for TestRecord {
        const SIZE: DataSize = DataSize::Fixed(100);

        const ALIGNMENT: PageOffset = 100;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Borrowed(&self.data)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let mut record = TestRecord { data: [0; 100] };
            record.data.copy_from_slice(&data[0..100]);
            Ok(record)
        }

        fn size(&self) -> MSize {
            100
        }
    }

    #[derive(Debug, Clone)]
    struct RecordWith32BytesPadding {
        data: [u8; 100],
    }

    impl Encode for RecordWith32BytesPadding {
        const SIZE: DataSize = DataSize::Dynamic;

        const ALIGNMENT: PageOffset = 32;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Borrowed(&self.data)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let mut record = Self { data: [0; 100] };
            record.data.copy_from_slice(&data[0..100]);
            Ok(record)
        }

        fn size(&self) -> MSize {
            100
        }
    }
}

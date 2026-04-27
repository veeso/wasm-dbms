// Rust guideline compliant 2026-02-28

mod free_segment;
mod free_segments_table;
mod pages_table;
mod tables_iter;

use wasm_dbms_api::prelude::{Encode, MSize, MemoryError, MemoryResult, Page, PageOffset};

pub use self::free_segment::FreeSegment;
use self::free_segments_table::FreeSegmentsTable;
use self::pages_table::PagesTable;
use self::tables_iter::TablesIter;
use crate::{MemoryAccess, align_up};

/// A ticket representing a reusable free segment.
///
/// This is used to track the origin of the free segment along with its metadata.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct FreeSegmentTicket {
    pub segment: FreeSegment,
    #[cfg(test)]
    pub table: Page,
    #[cfg(not(test))]
    table: Page,
}

/// The free segments ledger keeps track of free segments in the [`FreeSegmentsTable`] registry.
///
/// Free segments can occur either when a record is deleted or
/// when a record is moved to a different location due to resizing after an update.
///
/// Each record tracks:
///
/// - The page number where the record was located
/// - The offset within that page
/// - The size of the free segment
///
/// The responsibilities of this ledger include:
///
/// - Storing metadata about free segments whenever a record is deleted or moved
/// - Find a suitable location for new records by reusing space from free segments
///
/// The ledger is stored in a page which contains all the pages of the [`FreeSegmentsTable`]s.
///
/// The ledger can then load each [`FreeSegmentsTable`] as needed from their respective pages.
pub struct FreeSegmentsLedger {
    /// The page where the free segments ledger is stored in memory.
    ///
    /// This page actually just holds the page numbers to the pages containing the [`FreeSegmentsTable`]s.
    free_segments_page: Page,
    /// Pages containing the [`FreeSegmentTable`]s.
    tables: PagesTable,
}

impl FreeSegmentsLedger {
    /// Loads the deleted records ledger from memory.
    pub fn load(free_segments_page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        // read from memory
        let tables = mm.read_at(free_segments_page, 0)?;

        Ok(Self {
            free_segments_page,
            tables,
        })
    }

    /// Inserts a new [`FreeSegment`] into the ledger with the specified [`Page`], offset, and size.
    ///
    /// The size is calculated based on the size of the record plus the length prefix.
    ///
    /// The table is then written back to memory.
    pub fn insert_free_segment<E>(
        &mut self,
        page: Page,
        offset: PageOffset,
        record: &E,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        E: Encode,
    {
        let physical_size = align_up::<E>(record.size() as usize) as MSize;

        // get the first table with space to allocate the new segment
        for table in self.tables(mm) {
            let mut table = table?;
            if !table.is_full() {
                return table.insert_free_segment(page, offset, physical_size, mm);
            }
        }

        // otherwise, create a new page
        let new_page = self.create_new_page(mm)?;
        let mut table = FreeSegmentsTable::load(new_page, mm)?;

        table.insert_free_segment(page, offset, physical_size, mm)
    }

    /// Finds a reusable free segment that can accommodate the size of the given record.
    ///
    /// - If a suitable free segment is found, it is returned as [`Some<FreeSegmentTicket>`].
    /// - If no suitable free segment is found, [`None`] is returned.
    /// - If an error occurs during the search, it is returned as a [`MemoryError`].
    pub fn find_reusable_segment<E>(
        &self,
        record: &E,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Option<FreeSegmentTicket>>
    where
        E: Encode,
    {
        let required_size = record.size();

        // iterate over tables to find a suitable segment
        for table in self.tables(mm) {
            let table = table?;
            if let Some(segment) = table.find(|r| r.size >= required_size) {
                return Ok(Some(FreeSegmentTicket {
                    segment,
                    table: table.page(),
                }));
            }
        }

        Ok(None)
    }

    /// Commits a reused free segment by removing it from the ledger and updating it based on the used size.
    pub fn commit_reused_space<E>(
        &mut self,
        record: &E,
        segment: FreeSegmentTicket,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        E: Encode,
    {
        let physical_size = align_up::<E>(record.size() as usize) as MSize;

        // get the table
        let mut table = None;
        for i_table in self.tables(mm) {
            let i_table = i_table?;
            if i_table.page() == segment.table {
                table = Some(i_table);
                break;
            }
        }
        let Some(mut table) = table else {
            // return error, memory may be corrupted
            return Err(MemoryError::OutOfBounds);
        };

        table.remove(segment.segment, physical_size, mm)
    }

    /// Inserts a free segment with a runtime-known physical size.
    ///
    /// Sibling to [`Self::insert_free_segment`], used by the migration apply
    /// pipeline when releasing a record whose alignment came from a stored
    /// snapshot rather than a compile-time `Encode` impl.
    pub fn insert_free_segment_raw(
        &mut self,
        page: Page,
        offset: PageOffset,
        physical_size: MSize,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        for table in self.tables(mm) {
            let mut table = table?;
            if !table.is_full() {
                return table.insert_free_segment(page, offset, physical_size, mm);
            }
        }

        let new_page = self.create_new_page(mm)?;
        let mut table = FreeSegmentsTable::load(new_page, mm)?;
        table.insert_free_segment(page, offset, physical_size, mm)
    }

    /// Find a reusable free segment for a record of `required_size` bytes.
    /// Sibling to [`Self::find_reusable_segment`].
    pub fn find_reusable_segment_raw(
        &self,
        required_size: MSize,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Option<FreeSegmentTicket>> {
        for table in self.tables(mm) {
            let table = table?;
            if let Some(segment) = table.find(|r| r.size >= required_size) {
                return Ok(Some(FreeSegmentTicket {
                    segment,
                    table: table.page(),
                }));
            }
        }
        Ok(None)
    }

    /// Commit a reused segment with a runtime-known physical size.
    /// Sibling to [`Self::commit_reused_space`].
    pub fn commit_reused_space_raw(
        &mut self,
        physical_size: MSize,
        segment: FreeSegmentTicket,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        let mut table = None;
        for i_table in self.tables(mm) {
            let i_table = i_table?;
            if i_table.page() == segment.table {
                table = Some(i_table);
                break;
            }
        }
        let Some(mut table) = table else {
            return Err(MemoryError::OutOfBounds);
        };
        table.remove(segment.segment, physical_size, mm)
    }

    /// Writes the current state of the free segments table back to memory.
    fn commit(&self, mm: &mut impl MemoryAccess) -> MemoryResult<()> {
        mm.write_at(self.free_segments_page, 0, &self.tables)
    }

    /// Creates a new page for storing additional [`FreeSegmentsTable`]s when needed.
    fn create_new_page(&mut self, mm: &mut impl MemoryAccess) -> MemoryResult<Page> {
        let new_page = mm.allocate_page()?;
        self.tables.push(new_page);
        self.commit(mm).map(|_| new_page)
    }

    /// Returns an iterator over the [`FreeSegmentsTable`]s in the ledger.
    fn tables<'a, MA>(&'a self, mm: &'a mut MA) -> TablesIter<'a, MA>
    where
        MA: MemoryAccess,
    {
        TablesIter::new(self.tables.pages(), mm)
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{DEFAULT_ALIGNMENT, DataSize, DecodeError, MSize};

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    #[test]
    fn test_should_load_free_segments_ledger() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        // allocate new page
        let page = mm.allocate_page().expect("Failed to allocate page");

        let ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");
        assert_eq!(ledger.free_segments_page, page);
        assert!(ledger.tables.pages().is_empty());
    }

    #[test]
    fn test_should_insert_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        // allocate new page
        let page = mm.allocate_page().expect("Failed to allocate page");

        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let record = TestRecord { data: [0; 100] };

        ledger
            .insert_free_segment(4, 0, &record, &mut mm)
            .expect("Failed to insert deleted record");
    }

    #[test]
    fn test_should_find_suitable_reusable_space() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");

        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let record = TestRecord { data: [0; 100] };

        ledger
            .insert_free_segment(4, 0, &record, &mut mm)
            .expect("Failed to insert deleted record");

        let record = TestRecord { data: [0; 100] };
        let reusable_space = ledger
            .find_reusable_segment(&record, &mut mm)
            .expect("should find reusable space")
            .map(|ticket| ticket.segment);
        assert_eq!(
            reusable_space,
            Some(FreeSegment {
                page: 4,
                offset: 0,
                size: record.size(),
            })
        );
    }

    #[test]
    fn test_should_not_find_suitable_reusable_space() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");

        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let record = TestRecord { data: [0; 100] };

        ledger
            .insert_free_segment(4, 0, &record, &mut mm)
            .expect("Failed to insert deleted record");

        let record = BigTestRecord { data: [0; 200] };
        let reusable_space = ledger
            .find_reusable_segment(&record, &mut mm)
            .expect("should not find reusable space")
            .map(|ticket| ticket.segment);
        assert_eq!(reusable_space, None);
    }

    #[test]
    fn test_should_commit_reused_space_without_creating_a_new_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");

        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let record = TestRecord { data: [0; 100] };

        ledger
            .insert_free_segment(4, 0, &record, &mut mm)
            .expect("Failed to insert deleted record");

        let reusable_space = ledger
            .find_reusable_segment(&record, &mut mm)
            .expect("should find reusable space")
            .expect("should find reusable space");

        ledger
            .commit_reused_space(&record, reusable_space, &mut mm)
            .expect("Failed to commit reused space");

        // should be empty
        let record = find_record(&ledger, &mut mm, 4, 0, 100);
        assert!(record.is_none());

        // reload
        let reloaded_ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let record = find_record(&reloaded_ledger, &mut mm, 4, 0, 100);
        assert!(record.is_none());
    }

    #[test]
    fn test_should_commit_reused_space_creating_a_new_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");
        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let big_record = BigTestRecord { data: [1; 200] };

        ledger
            .insert_free_segment(4, 0, &big_record, &mut mm)
            .expect("Failed to insert deleted record");

        let small_record = TestRecord { data: [0; 100] };
        let reusable_space = ledger
            .find_reusable_segment(&small_record, &mut mm)
            .expect("memory error")
            .expect("should find reusable space");

        ledger
            .commit_reused_space(&small_record, reusable_space, &mut mm)
            .expect("Failed to commit reused space");

        // should have a new record for the remaining space
        let record = find_record(&ledger, &mut mm, 4, 100, 100);
        assert!(record.is_some());
    }

    #[test]
    fn test_should_commit_also_padding_with_dynamic_records() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");
        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        let dyn_record = DynamicTestRecord { data: [1; 200] };

        ledger
            .insert_free_segment(4, 0, &dyn_record, &mut mm)
            .expect("Failed to insert deleted record");

        // check if padded
        let reusable_space = ledger
            .find_reusable_segment(&dyn_record, &mut mm)
            .expect("memory error")
            .expect("should find reusable space");
        assert_eq!(reusable_space.segment.size, 224);

        // insert another
        let dyn_record = DynamicTestRecord { data: [1; 200] };

        ledger
            .insert_free_segment(4, reusable_space.segment.size, &dyn_record, &mut mm)
            .expect("Failed to insert deleted record");
        // there should be a contiguous free segment of size 448 now
        let reusable_space = ledger
            .find_reusable_segment(&dyn_record, &mut mm)
            .expect("memory error")
            .expect("should find reusable space");
        assert_eq!(reusable_space.segment.size, 448);
    }

    #[test]
    fn test_should_grow_out_of_first_page() {
        // let's store 20_500 segments, non contiguously
        let mut offset = 0;

        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.allocate_page().expect("Failed to allocate page");
        let mut ledger =
            FreeSegmentsLedger::load(page, &mut mm).expect("Failed to load DeletedRecordsLedger");

        for _ in 0..14_500 {
            let record = SmallRecord { data: 42 };

            ledger
                .insert_free_segment(4, offset, &record, &mut mm)
                .expect("Failed to insert deleted record");
            offset += SmallRecord::ALIGNMENT * 2; // leave gaps
        }
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
    struct BigTestRecord {
        data: [u8; 200],
    }

    impl Encode for BigTestRecord {
        const SIZE: DataSize = DataSize::Fixed(200);

        const ALIGNMENT: PageOffset = 200;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Borrowed(&self.data)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let mut record = BigTestRecord { data: [0; 200] };
            record.data.copy_from_slice(&data[0..200]);
            Ok(record)
        }

        fn size(&self) -> MSize {
            200
        }
    }

    #[derive(Debug, Clone)]
    struct DynamicTestRecord {
        data: [u8; 200],
    }

    impl Encode for DynamicTestRecord {
        const SIZE: DataSize = DataSize::Dynamic;

        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Borrowed(&self.data)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let mut record = DynamicTestRecord { data: [0; 200] };
            record.data.copy_from_slice(&data[0..200]);
            Ok(record)
        }

        fn size(&self) -> MSize {
            200
        }
    }

    #[derive(Debug, Clone)]
    struct SmallRecord {
        data: u16,
    }

    impl Encode for SmallRecord {
        const SIZE: DataSize = DataSize::Fixed(2);

        const ALIGNMENT: PageOffset = 2;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            let mut buf = [0u8; 2];
            buf[0] = (self.data & 0xFF) as u8;
            buf[1] = ((self.data >> 8) & 0xFF) as u8;
            std::borrow::Cow::Owned(buf.into())
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            if data.len() < 2 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }

            let value = (data[0] as u16) | ((data[1] as u16) << 8);
            Ok(SmallRecord { data: value })
        }

        fn size(&self) -> MSize {
            2
        }
    }

    fn find_record(
        ledger: &FreeSegmentsLedger,
        mm: &mut MemoryManager<HeapMemoryProvider>,
        page: Page,
        offset: PageOffset,
        size: MSize,
    ) -> Option<FreeSegment> {
        for table in ledger.tables(mm) {
            let table = table.expect("Failed to read table");
            if let Some(record) =
                table.find(|r| r.page == page && r.offset == offset && r.size == size)
            {
                return Some(record);
            }
        }
        None
    }
}

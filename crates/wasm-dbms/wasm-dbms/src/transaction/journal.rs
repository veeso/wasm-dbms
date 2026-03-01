// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Write-ahead journal for transaction atomicity.
//!
//! The journal records original byte values before each write so that
//! they can be replayed in reverse on rollback, restoring memory to
//! its pre-transaction state.

use wasm_dbms_api::prelude::{Encode, MemoryResult, Page, PageOffset};
use wasm_dbms_memory::prelude::{MemoryAccess, MemoryManager, MemoryProvider, align_up};

/// A single journal entry recording original bytes before a write.
#[derive(Debug)]
struct JournalEntry {
    /// Page where the write occurred.
    page: Page,
    /// Offset within the page where the write occurred.
    offset: PageOffset,
    /// Original bytes at `(page, offset)` before the write.
    original_bytes: Vec<u8>,
}

/// A write-ahead journal that captures pre-write byte snapshots for
/// rollback.
#[derive(Debug)]
pub struct Journal {
    entries: Vec<JournalEntry>,
}

impl Journal {
    /// Creates a new empty journal.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Commits the journal, discarding all entries.
    ///
    /// Call this after a successful atomic operation to confirm that
    /// the writes should be kept.
    pub fn commit(self) {
        // Dropping `self` discards all entries — nothing else needed.
    }

    /// Rolls back all recorded writes, restoring the original bytes.
    ///
    /// Entries are replayed in reverse order so that overlapping
    /// writes are undone correctly.
    pub fn rollback<P>(self, mm: &mut MemoryManager<P>) -> MemoryResult<()>
    where
        P: MemoryProvider,
    {
        for entry in self.entries.into_iter().rev() {
            mm.write_at_raw(entry.page, entry.offset, &entry.original_bytes)?;
        }
        Ok(())
    }

    /// Records original bytes for a region that is about to be
    /// written.
    fn record<P>(
        &mut self,
        mm: &MemoryManager<P>,
        page: Page,
        offset: PageOffset,
        len: usize,
    ) -> MemoryResult<()>
    where
        P: MemoryProvider,
    {
        let mut original = vec![0u8; len];
        mm.read_at_raw(page, offset, &mut original)?;
        self.entries.push(JournalEntry {
            page,
            offset,
            original_bytes: original,
        });
        Ok(())
    }
}

/// A wrapper that intercepts writes to record them in a [`Journal`]
/// before delegating to the underlying [`MemoryManager`].
pub struct JournaledWriter<'a, P>
where
    P: MemoryProvider,
{
    mm: &'a mut MemoryManager<P>,
    journal: &'a mut Journal,
}

impl<'a, P> JournaledWriter<'a, P>
where
    P: MemoryProvider,
{
    /// Creates a new journaled writer wrapping the given memory
    /// manager and journal.
    pub fn new(mm: &'a mut MemoryManager<P>, journal: &'a mut Journal) -> Self {
        Self { mm, journal }
    }
}

impl<P> MemoryAccess for JournaledWriter<'_, P>
where
    P: MemoryProvider,
{
    fn page_size(&self) -> u64 {
        self.mm.page_size()
    }

    fn allocate_page(&mut self) -> MemoryResult<Page> {
        // Page allocation is NOT journaled by design.
        self.mm.allocate_page()
    }

    fn read_at<D>(&self, page: Page, offset: PageOffset) -> MemoryResult<D>
    where
        D: Encode,
    {
        self.mm.read_at(page, offset)
    }

    fn write_at<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode,
    {
        // Compute total write footprint including padding.
        let total_len = align_up::<E>(data.size() as usize);
        self.journal.record(self.mm, page, offset, total_len)?;
        self.mm.write_at(page, offset, data)
    }

    fn zero<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode,
    {
        let total_len = align_up::<E>(data.size() as usize);
        self.journal.record(self.mm, page, offset, total_len)?;
        self.mm.zero(page, offset, data)
    }

    fn read_at_raw(&self, page: Page, offset: PageOffset, buf: &mut [u8]) -> MemoryResult<usize> {
        self.mm.read_at_raw(page, offset, buf)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use wasm_dbms_api::prelude::{DEFAULT_ALIGNMENT, DataSize, MSize, MemoryResult, PageOffset};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    const ACL_PAGE: Page = 1;

    #[test]
    fn test_journal_begin_commit_clears_journal() {
        let mut mm = make_mm();
        let mut journal = Journal::new();

        let data = FixedSizeData { a: 1, b: 2 };
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            writer
                .write_at(ACL_PAGE, 0, &data)
                .expect("Failed to write data");
        }

        journal.commit();
    }

    #[test]
    fn test_journal_rollback_restores_write_at() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original data");

        let mut journal = Journal::new();
        let overwrite = FixedSizeData { a: 99, b: 100 };
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            writer
                .write_at(ACL_PAGE, 0, &overwrite)
                .expect("Failed to overwrite data");
        }

        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, overwrite);

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_journal_rollback_restores_zero() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 42, b: 1337 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original data");

        let mut journal = Journal::new();
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            writer
                .zero(ACL_PAGE, 0, &original)
                .expect("Failed to zero data");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_writes_without_journal_work_normally() {
        let mut mm = make_mm();

        let data = FixedSizeData { a: 5, b: 10 };
        mm.write_at(ACL_PAGE, 0, &data)
            .expect("Failed to write data");

        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_journal_rollback_multiple_writes_in_reverse() {
        let mut mm = make_mm();

        let data_a = FixedSizeData { a: 1, b: 2 };
        let data_b = FixedSizeData { a: 3, b: 4 };
        mm.write_at(ACL_PAGE, 0, &data_a)
            .expect("Failed to write data_a");
        mm.write_at(ACL_PAGE, 6, &data_b)
            .expect("Failed to write data_b");

        let mut journal = Journal::new();
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            let overwrite_a = FixedSizeData { a: 100, b: 200 };
            let overwrite_b = FixedSizeData { a: 300, b: 400 };
            writer
                .write_at(ACL_PAGE, 0, &overwrite_a)
                .expect("Failed to overwrite data_a");
            writer
                .write_at(ACL_PAGE, 6, &overwrite_b)
                .expect("Failed to overwrite data_b");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let restored_a: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        let restored_b: FixedSizeData = mm.read_at(ACL_PAGE, 6).expect("Failed to read data");
        assert_eq!(restored_a, data_a);
        assert_eq!(restored_b, data_b);
    }

    #[test]
    fn test_journal_rollback_overlapping_writes_restores_original() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        let mut journal = Journal::new();
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            let first = FixedSizeData { a: 50, b: 60 };
            writer
                .write_at(ACL_PAGE, 0, &first)
                .expect("Failed to write first overwrite");
            let second = FixedSizeData { a: 90, b: 100 };
            writer
                .write_at(ACL_PAGE, 0, &second)
                .expect("Failed to write second overwrite");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_journal_committed_writes_persist() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 1, b: 2 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        let mut journal = Journal::new();
        let updated = FixedSizeData { a: 99, b: 100 };
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            writer
                .write_at(ACL_PAGE, 0, &updated)
                .expect("Failed to write updated data");
        }

        journal.commit();

        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, updated);
    }

    #[test]
    fn test_journal_allocate_page_is_not_rolled_back() {
        let mut mm = make_mm();
        let pages_before = mm.page_size(); // just need a count proxy
        let _ = pages_before;

        let mut journal = Journal::new();
        let new_page;
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            new_page = writer.allocate_page().expect("Failed to allocate page");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        // Page allocation is not journaled — verify we can still read from the page.
        let mut buf = vec![0u8; 8];
        mm.read_at_raw(new_page, 0, &mut buf)
            .expect("Page should still exist after rollback");
    }

    #[test]
    fn test_journal_rollback_mixed_write_at_and_zero() {
        let mut mm = make_mm();

        let data_a = FixedSizeData { a: 11, b: 22 };
        let data_b = FixedSizeData { a: 33, b: 44 };
        mm.write_at(ACL_PAGE, 0, &data_a)
            .expect("Failed to write data_a");
        mm.write_at(ACL_PAGE, 6, &data_b)
            .expect("Failed to write data_b");

        let mut journal = Journal::new();
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            let overwrite = FixedSizeData { a: 77, b: 88 };
            writer
                .write_at(ACL_PAGE, 0, &overwrite)
                .expect("Failed to overwrite data_a");
            writer
                .zero(ACL_PAGE, 6, &data_b)
                .expect("Failed to zero data_b");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let restored_a: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        let restored_b: FixedSizeData = mm.read_at(ACL_PAGE, 6).expect("Failed to read data");
        assert_eq!(restored_a, data_a);
        assert_eq!(restored_b, data_b);
    }

    #[test]
    fn test_journal_rollback_restores_padding_bytes() {
        let mut mm = make_mm();

        let original = DataWithAlignment { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        let mut original_raw = vec![0u8; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut original_raw)
            .expect("Failed to read raw");

        let mut journal = Journal::new();
        {
            let mut writer = JournaledWriter::new(&mut mm, &mut journal);
            let overwrite = DataWithAlignment { a: 99, b: 100 };
            writer
                .write_at(ACL_PAGE, 0, &overwrite)
                .expect("Failed to overwrite");
        }

        journal
            .rollback(&mut mm)
            .expect("Failed to rollback journal");

        let mut restored_raw = vec![0u8; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut restored_raw)
            .expect("Failed to read raw");
        assert_eq!(restored_raw, original_raw);
    }

    // -- test helpers --------------------------------------------------------

    #[derive(Debug, Clone, PartialEq)]
    struct FixedSizeData {
        a: u16,
        b: u32,
    }

    impl Encode for FixedSizeData {
        const SIZE: DataSize = DataSize::Fixed(6);
        const ALIGNMENT: PageOffset = 6;

        fn encode(&'_ self) -> Cow<'_, [u8]> {
            let mut buf = vec![0u8; self.size() as usize];
            buf[0..2].copy_from_slice(&self.a.to_le_bytes());
            buf[2..6].copy_from_slice(&self.b.to_le_bytes());
            Cow::Owned(buf)
        }

        fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let a = u16::from_le_bytes([data[0], data[1]]);
            let b = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
            Ok(FixedSizeData { a, b })
        }

        fn size(&self) -> MSize {
            6
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct DataWithAlignment {
        a: u16,
        b: u32,
    }

    impl Encode for DataWithAlignment {
        const SIZE: DataSize = DataSize::Dynamic;
        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> Cow<'_, [u8]> {
            let mut buf = vec![0u8; self.size() as usize];
            buf[0..2].copy_from_slice(&self.a.to_le_bytes());
            buf[2..6].copy_from_slice(&self.b.to_le_bytes());
            Cow::Owned(buf)
        }

        fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            let a = u16::from_le_bytes([data[0], data[1]]);
            let b = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
            Ok(DataWithAlignment { a, b })
        }

        fn size(&self) -> MSize {
            6
        }
    }
}

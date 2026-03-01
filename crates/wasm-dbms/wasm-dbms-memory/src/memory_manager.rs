// Rust guideline compliant 2026-03-01

//! Memory manager for page-level memory operations.

use wasm_dbms_api::prelude::{
    DataSize, Encode, MSize, MemoryError, MemoryResult, Page, PageOffset,
};

use crate::provider::MemoryProvider;

/// Schema page (reserved page 0).
pub const SCHEMA_PAGE: Page = 0;
/// ACL page (reserved page 1).
pub const ACL_PAGE: Page = 1;

/// A single journal entry recording original bytes before a write.
///
/// Used by the write-ahead journal to restore memory to its pre-write state
/// on rollback.
#[derive(Debug)]
struct JournalEntry {
    /// Absolute offset in memory where the write occurred.
    offset: u64,
    /// Original bytes at `offset` before the write.
    original_bytes: Vec<u8>,
}

/// The memory manager handles page-level memory operations on top of a
/// [`MemoryProvider`].
pub struct MemoryManager<P>
where
    P: MemoryProvider,
{
    provider: P,
    /// Active write-ahead journal, if any.
    ///
    /// When `Some`, all writes via [`Self::write_at`] and [`Self::zero`] are
    /// recorded so they can be rolled back on error.
    journal: Option<Vec<JournalEntry>>,
}

impl<P> MemoryManager<P>
where
    P: MemoryProvider,
{
    /// Initializes the memory manager and allocates the header and reserved
    /// pages.
    ///
    /// # Panics
    ///
    /// Panics if the memory provider fails to initialize.
    pub fn init(provider: P) -> Self {
        let mut manager = MemoryManager {
            provider,
            journal: None,
        };

        // Check whether two pages are already allocated.
        if manager.provider.pages() >= 2 {
            return manager;
        }

        // Request at least 2 pages for header and ACL.
        if let Err(err) = manager.provider.grow(2) {
            panic!("Failed to grow memory during initialization: {err}");
        }

        manager
    }

    /// Returns the size of a memory page.
    pub const fn page_size(&self) -> u64 {
        P::PAGE_SIZE
    }

    /// Returns the ACL page number.
    pub const fn acl_page(&self) -> Page {
        ACL_PAGE
    }

    /// Returns the schema page.
    pub const fn schema_page(&self) -> Page {
        SCHEMA_PAGE
    }

    /// Allocates an additional page in memory.
    ///
    /// On success returns the [`Page`] number.
    pub fn allocate_page(&mut self) -> MemoryResult<Page> {
        self.provider.grow(1)?;

        // Zero the newly allocated page.
        self.provider.write(
            self.absolute_offset(self.last_page().unwrap_or(0), 0),
            &vec![0u8; P::PAGE_SIZE as usize],
        )?;

        match self.last_page() {
            Some(page) => Ok(page),
            None => Err(MemoryError::FailedToAllocatePage),
        }
    }

    /// Begins a new journal session, recording all writes for potential
    /// rollback.
    ///
    /// While a journal is active, every call to [`Self::write_at`] and
    /// [`Self::zero`] saves the original bytes so they can be restored by
    /// [`Self::rollback_journal`].
    pub fn begin_journal(&mut self) {
        assert!(
            self.journal.is_none(),
            "begin_journal called while a journal is already active"
        );
        self.journal = Some(Vec::new());
    }

    /// Returns `true` if a journal session is currently active.
    pub fn is_journal_active(&self) -> bool {
        self.journal.is_some()
    }

    /// Commits the journal, discarding all recorded entries.
    ///
    /// Call this after a successful atomic operation to confirm that the
    /// writes should be kept.
    pub fn commit_journal(&mut self) {
        self.journal = None;
    }

    /// Rolls back all writes recorded in the journal, restoring original
    /// bytes.
    ///
    /// Entries are replayed in reverse order so that overlapping writes are
    /// undone correctly.
    pub fn rollback_journal(&mut self) -> MemoryResult<()> {
        if let Some(journal) = self.journal.take() {
            for entry in journal.into_iter().rev() {
                self.provider.write(entry.offset, &entry.original_bytes)?;
            }
        }
        Ok(())
    }

    /// Writes `buf` at `offset`, recording the original bytes if a journal
    /// is active.
    fn journaled_write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        if let Some(journal) = &mut self.journal {
            let mut original = vec![0u8; buf.len()];
            self.provider.read(offset, &mut original)?;
            journal.push(JournalEntry {
                offset,
                original_bytes: original,
            });
        }
        self.provider.write(offset, buf)
    }

    /// Reads data as an [`Encode`] implementor at the specified page and
    /// offset.
    pub fn read_at<D>(&self, page: Page, offset: PageOffset) -> MemoryResult<D>
    where
        D: Encode,
    {
        self.check_alignment::<D>(offset)?;

        let mut buf = vec![
            0u8;
            match D::SIZE {
                DataSize::Fixed(size) => size as usize,
                DataSize::Dynamic => (P::PAGE_SIZE as usize).saturating_sub(offset as usize),
            }
        ];

        self.read_at_raw(page, offset, &mut buf)?;

        D::decode(std::borrow::Cow::Owned(buf))
    }

    /// Writes data as an [`Encode`] implementor at the specified page and
    /// offset.
    pub fn write_at<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode,
    {
        self.check_unallocated_page(page, offset, data.size())?;
        self.check_alignment::<E>(offset)?;

        let encoded = data.encode();

        if offset as u64 + encoded.len() as u64 > P::PAGE_SIZE {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: encoded.len() as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        let absolute_offset = self.absolute_offset(page, offset);
        self.journaled_write(absolute_offset, encoded.as_ref())?;

        // Zero padding bytes if any.
        let padding = align_up::<E>(encoded.len()) - encoded.len();
        if padding > 0 {
            let padding_offset = absolute_offset + encoded.len() as u64;
            let padding_buffer = vec![0u8; padding];
            self.journaled_write(padding_offset, padding_buffer.as_ref())?;
        }

        Ok(())
    }

    /// Zeros out data at the specified page and offset.
    pub fn zero<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode,
    {
        self.check_unallocated_page(page, offset, data.size())?;
        self.check_alignment::<E>(offset)?;

        let length = align_up::<E>(data.size() as usize);

        if offset as u64 + (length as u64) > P::PAGE_SIZE {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: data.size() as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        let absolute_offset = self.absolute_offset(page, offset);
        let buffer = vec![0u8; length];
        self.journaled_write(absolute_offset, buffer.as_ref())
    }

    /// Reads raw bytes into the provided buffer at the specified page and
    /// offset.
    pub fn read_at_raw(
        &self,
        page: Page,
        offset: PageOffset,
        buf: &mut [u8],
    ) -> MemoryResult<usize> {
        if self.last_page().is_none_or(|last_page| page > last_page) {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: buf.len() as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        let read_len = ((P::PAGE_SIZE - offset as u64) as usize).min(buf.len());

        let absolute_offset = self.absolute_offset(page, offset);
        self.provider
            .read(absolute_offset, buf[..read_len].as_mut())?;

        Ok(read_len)
    }

    /// Gets the last allocated page number.
    fn last_page(&self) -> Option<Page> {
        match self.provider.pages() {
            0 => None,
            n => Some(n as Page - 1),
        }
    }

    /// Calculates the absolute offset in memory given a page number and an
    /// offset within that page.
    fn absolute_offset(&self, page: Page, offset: PageOffset) -> u64 {
        (page as u64)
            .checked_mul(P::PAGE_SIZE)
            .and_then(|page_offset| page_offset.checked_add(offset as u64))
            .expect("Overflow when calculating absolute offset")
    }

    /// Checks if the specified page is allocated.
    fn check_unallocated_page(
        &self,
        page: Page,
        offset: PageOffset,
        data_size: MSize,
    ) -> MemoryResult<()> {
        if self.last_page().is_none_or(|last_page| page > last_page) {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: data_size as u64,
                page_size: P::PAGE_SIZE,
            });
        }
        Ok(())
    }

    /// Checks if the given offset is aligned according to the alignment
    /// requirement of type `E`.
    fn check_alignment<E>(&self, offset: PageOffset) -> MemoryResult<()>
    where
        E: Encode,
    {
        let alignment = E::ALIGNMENT as PageOffset;
        if alignment != 0 && !offset.is_multiple_of(alignment) {
            return Err(MemoryError::OffsetNotAligned { offset, alignment });
        }
        Ok(())
    }
}

/// Gets the padding at the given offset to the next multiple of
/// [`E::ALIGNMENT`].
#[inline]
pub const fn align_up<E>(offset: usize) -> usize
where
    E: Encode,
{
    let alignment = E::ALIGNMENT as usize;
    offset.div_ceil(alignment) * alignment
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use wasm_dbms_api::prelude::{
        DEFAULT_ALIGNMENT, DataSize, MSize, MemoryError, MemoryResult, PageOffset, Text,
    };

    use super::*;
    use crate::provider::HeapMemoryProvider;

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    #[test]
    fn test_should_init_memory_manager() {
        let mm = make_mm();
        assert_eq!(mm.last_page(), Some(1));
    }

    #[test]
    fn test_should_get_last_page() {
        let mm = make_mm();
        let last_page = mm.last_page();
        assert_eq!(last_page, Some(1)); // header and ACL pages
    }

    #[test]
    fn test_should_get_memory_page_size() {
        let mm = make_mm();
        let page_size = mm.page_size();
        assert_eq!(page_size, HeapMemoryProvider::PAGE_SIZE);
    }

    #[test]
    fn test_should_write_and_read_fixed_data_size() {
        let mut mm = make_mm();
        let data_to_write = FixedSizeData { a: 42, b: 1337 };
        mm.write_at(ACL_PAGE, 0, &data_to_write)
            .expect("Failed to write data to ACL page");

        let out: FixedSizeData = mm
            .read_at(ACL_PAGE, 0)
            .expect("Failed to read data from ACL page");

        assert_eq!(out, data_to_write);
    }

    #[test]
    fn test_write_should_zero_padding() {
        let mut mm = make_mm();
        let data_to_write = Text("very_long_string".to_string());
        mm.write_at(ACL_PAGE, 0, &data_to_write)
            .expect("Failed to write data to ACL page");

        let mut buffer = vec![0; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut buffer)
            .expect("Failed to read data from ACL page");

        let non_zero_count = buffer.iter().filter(|&&b| b != 0).count();
        assert_eq!(non_zero_count, data_to_write.size() as usize - 1);

        let data_to_write_short = Text("short".to_string());
        mm.write_at(ACL_PAGE, 0, &data_to_write_short)
            .expect("Failed to write data to ACL page");

        let mut buffer = vec![0; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut buffer)
            .expect("Failed to read data from ACL page");

        let non_zero_count = buffer.iter().filter(|&&b| b != 0).count();
        assert_eq!(non_zero_count, data_to_write_short.size() as usize - 1);
    }

    #[test]
    fn test_should_zero_data() {
        let mut mm = make_mm();
        let data_to_write = FixedSizeData { a: 100, b: 200 };
        mm.write_at(ACL_PAGE, 48, &data_to_write)
            .expect("Failed to write data to ACL page");

        mm.zero(ACL_PAGE, 48, &data_to_write)
            .expect("Failed to zero data on ACL page");

        let mut buffer = vec![0; 50];
        mm.read_at_raw(ACL_PAGE, 48, &mut buffer)
            .expect("Failed to read data from ACL page");

        assert!(buffer.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_should_zero_with_alignment() {
        let mut mm = make_mm();
        let data_to_write = FixedSizeData { a: 100, b: 200 };
        mm.write_at(ACL_PAGE, 0, &data_to_write)
            .expect("Failed to write data to ACL page");
        let data_to_write = FixedSizeData { a: 100, b: 200 };
        mm.write_at(ACL_PAGE, 6, &data_to_write)
            .expect("Failed to write data to ACL page");

        let data_with_alignment = DataWithAlignment { a: 100, b: 200 };
        mm.zero(ACL_PAGE, 0, &data_with_alignment)
            .expect("Failed to zero data on ACL page");

        let mut buffer = vec![0; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut buffer)
            .expect("Failed to read data from ACL page");
        assert!(
            buffer.iter().all(|&b| b == 0),
            "First 32 bytes are not zeroed"
        );
    }

    #[test]
    fn test_should_check_whether_write_is_aligned() {
        let mut mm = make_mm();
        let data_to_write = FixedSizeData { a: 100, b: 200 };
        let res = mm.write_at(ACL_PAGE, 2, &data_to_write);
        assert!(matches!(res, Err(MemoryError::OffsetNotAligned { .. })));
    }

    #[test]
    fn test_should_check_whether_read_is_aligned() {
        let mm = make_mm();
        let result: MemoryResult<FixedSizeData> = mm.read_at(ACL_PAGE, 3);
        assert!(matches!(result, Err(MemoryError::OffsetNotAligned { .. })));
    }

    #[test]
    fn test_should_check_whether_zero_is_aligned() {
        let mut mm = make_mm();
        let data_to_zero = FixedSizeData { a: 1, b: 2 };
        let result = mm.zero(ACL_PAGE, 5, &data_to_zero);
        assert!(matches!(result, Err(MemoryError::OffsetNotAligned { .. })));
    }

    #[test]
    fn test_should_not_zero_unallocated_page() {
        let mut mm = make_mm();
        let data_to_zero = FixedSizeData { a: 1, b: 2 };
        let result = mm.zero(10, 0, &data_to_zero);
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));
    }

    #[test]
    fn test_should_not_zero_out_of_bounds() {
        let mut mm = make_mm();
        let data_to_zero = FixedSizeData { a: 1, b: 2 };
        let result = mm.zero(
            ACL_PAGE,
            (HeapMemoryProvider::PAGE_SIZE - 4) as PageOffset,
            &data_to_zero,
        );
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));
    }

    #[test]
    fn test_should_read_raw() {
        let mut mm = make_mm();
        let data_to_write = vec![1u8, 2, 3, 4, 5];
        mm.provider
            .write(mm.absolute_offset(ACL_PAGE, 20), &data_to_write)
            .expect("Failed to write raw data to ACL page");

        let mut buf = vec![0u8; 5];
        mm.read_at_raw(ACL_PAGE, 20, &mut buf)
            .expect("Failed to read raw data from ACL page");

        assert_eq!(buf, data_to_write);
    }

    #[test]
    fn test_should_fail_out_of_bounds_access() {
        let mut mm = make_mm();
        let data_to_write = FixedSizeData { a: 1, b: 2 };
        let result = mm.write_at(
            ACL_PAGE,
            (HeapMemoryProvider::PAGE_SIZE - 4) as PageOffset,
            &data_to_write,
        );
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));

        let result: MemoryResult<FixedSizeData> = mm.read_at(10, 0);
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));
        let result = mm.write_at(10, 0, &data_to_write);
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));
    }

    #[test]
    fn test_should_allocate_new_page() {
        let mut mm = make_mm();
        let initial_last_page = mm.last_page().unwrap();
        let new_page = mm.allocate_page().expect("Failed to allocate new page");
        assert_eq!(new_page, initial_last_page + 1);
        let updated_last_page = mm.last_page().unwrap();
        assert_eq!(updated_last_page, new_page);
    }

    #[test]
    fn test_should_check_unallocated_page() {
        let mm = make_mm();
        let result = mm.check_unallocated_page(100, 0, 10);
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));

        let last_page = mm.last_page().unwrap();
        let result = mm.check_unallocated_page(last_page, 0, 10);
        assert!(result.is_ok());
    }

    #[test]
    fn test_should_compute_padding() {
        assert_eq!(align_up::<DataWithAlignment>(0), 0);
        assert_eq!(align_up::<DataWithAlignment>(1), 32);
        assert_eq!(align_up::<DataWithAlignment>(2), 32);
        assert_eq!(align_up::<DataWithAlignment>(3), 32);
        assert_eq!(align_up::<DataWithAlignment>(31), 32);
        assert_eq!(align_up::<DataWithAlignment>(32), 32);
        assert_eq!(align_up::<DataWithAlignment>(48), 64);
        assert_eq!(align_up::<DataWithAlignment>(147), 160);
    }

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

    // -- journal tests -------------------------------------------------------

    #[test]
    fn test_journal_begin_commit_clears_journal() {
        let mut mm = make_mm();
        mm.begin_journal();
        assert!(mm.journal.is_some());

        let data = FixedSizeData { a: 1, b: 2 };
        mm.write_at(ACL_PAGE, 0, &data)
            .expect("Failed to write data");

        mm.commit_journal();
        assert!(mm.journal.is_none());
    }

    #[test]
    fn test_journal_rollback_restores_write_at() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original data");

        mm.begin_journal();

        let overwrite = FixedSizeData { a: 99, b: 100 };
        mm.write_at(ACL_PAGE, 0, &overwrite)
            .expect("Failed to overwrite data");

        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, overwrite);

        mm.rollback_journal().expect("Failed to rollback journal");

        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_journal_rollback_restores_zero() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 42, b: 1337 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original data");

        mm.begin_journal();

        mm.zero(ACL_PAGE, 0, &original)
            .expect("Failed to zero data");

        mm.rollback_journal().expect("Failed to rollback journal");

        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_writes_without_journal_work_normally() {
        let mut mm = make_mm();
        assert!(mm.journal.is_none());

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

        mm.begin_journal();

        let overwrite_a = FixedSizeData { a: 100, b: 200 };
        let overwrite_b = FixedSizeData { a: 300, b: 400 };
        mm.write_at(ACL_PAGE, 0, &overwrite_a)
            .expect("Failed to overwrite data_a");
        mm.write_at(ACL_PAGE, 6, &overwrite_b)
            .expect("Failed to overwrite data_b");

        mm.rollback_journal().expect("Failed to rollback journal");

        let restored_a: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        let restored_b: FixedSizeData = mm.read_at(ACL_PAGE, 6).expect("Failed to read data");
        assert_eq!(restored_a, data_a);
        assert_eq!(restored_b, data_b);
    }

    #[test]
    fn test_journal_rollback_with_no_active_journal_is_noop() {
        let mut mm = make_mm();
        assert!(mm.journal.is_none());

        let data = FixedSizeData { a: 7, b: 8 };
        mm.write_at(ACL_PAGE, 0, &data)
            .expect("Failed to write data");

        // Rollback without begin should be a no-op.
        mm.rollback_journal()
            .expect("Rollback without journal should succeed");

        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, data);
    }

    #[test]
    fn test_journal_commit_with_no_active_journal_is_noop() {
        let mut mm = make_mm();
        assert!(mm.journal.is_none());

        // Commit without begin should be a no-op (no panic, no error).
        mm.commit_journal();
        assert!(mm.journal.is_none());
    }

    #[test]
    fn test_journal_rollback_overlapping_writes_restores_original() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        mm.begin_journal();

        // First overwrite.
        let first = FixedSizeData { a: 50, b: 60 };
        mm.write_at(ACL_PAGE, 0, &first)
            .expect("Failed to write first overwrite");

        // Second overwrite at the same offset.
        let second = FixedSizeData { a: 90, b: 100 };
        mm.write_at(ACL_PAGE, 0, &second)
            .expect("Failed to write second overwrite");

        mm.rollback_journal().expect("Failed to rollback journal");

        // Should restore to the state before `begin_journal`, not to the
        // intermediate state.
        let restored: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(restored, original);
    }

    #[test]
    fn test_journal_committed_writes_persist() {
        let mut mm = make_mm();

        let original = FixedSizeData { a: 1, b: 2 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        mm.begin_journal();

        let updated = FixedSizeData { a: 99, b: 100 };
        mm.write_at(ACL_PAGE, 0, &updated)
            .expect("Failed to write updated data");

        mm.commit_journal();

        // After commit, the updated data should persist.
        let read_back: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        assert_eq!(read_back, updated);
    }

    #[test]
    fn test_journal_is_none_after_rollback() {
        let mut mm = make_mm();

        mm.begin_journal();
        assert!(mm.journal.is_some());

        let data = FixedSizeData { a: 1, b: 2 };
        mm.write_at(ACL_PAGE, 0, &data)
            .expect("Failed to write data");

        mm.rollback_journal().expect("Failed to rollback journal");
        assert!(mm.journal.is_none());
    }

    #[test]
    fn test_journal_allocate_page_is_not_rolled_back() {
        let mut mm = make_mm();
        let pages_before = mm.last_page().unwrap();

        mm.begin_journal();

        let new_page = mm.allocate_page().expect("Failed to allocate page");
        assert_eq!(new_page, pages_before + 1);

        mm.rollback_journal().expect("Failed to rollback journal");

        // Page allocation is not journaled — the page still exists.
        assert_eq!(mm.last_page().unwrap(), pages_before + 1);
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

        mm.begin_journal();

        // Overwrite slot A, zero slot B.
        let overwrite = FixedSizeData { a: 77, b: 88 };
        mm.write_at(ACL_PAGE, 0, &overwrite)
            .expect("Failed to overwrite data_a");
        mm.zero(ACL_PAGE, 6, &data_b)
            .expect("Failed to zero data_b");

        mm.rollback_journal().expect("Failed to rollback journal");

        let restored_a: FixedSizeData = mm.read_at(ACL_PAGE, 0).expect("Failed to read data");
        let restored_b: FixedSizeData = mm.read_at(ACL_PAGE, 6).expect("Failed to read data");
        assert_eq!(restored_a, data_a);
        assert_eq!(restored_b, data_b);
    }

    #[test]
    fn test_journal_rollback_restores_padding_bytes() {
        let mut mm = make_mm();

        // Write aligned data that uses padding (DataWithAlignment has
        // ALIGNMENT = 32).
        let original = DataWithAlignment { a: 10, b: 20 };
        mm.write_at(ACL_PAGE, 0, &original)
            .expect("Failed to write original");

        // Read raw bytes to capture the full padded region.
        let mut original_raw = vec![0u8; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut original_raw)
            .expect("Failed to read raw");

        mm.begin_journal();

        let overwrite = DataWithAlignment { a: 99, b: 100 };
        mm.write_at(ACL_PAGE, 0, &overwrite)
            .expect("Failed to overwrite");

        mm.rollback_journal().expect("Failed to rollback journal");

        // Verify the full padded region is restored, not just the data bytes.
        let mut restored_raw = vec![0u8; 32];
        mm.read_at_raw(ACL_PAGE, 0, &mut restored_raw)
            .expect("Failed to read raw");
        assert_eq!(restored_raw, original_raw);
    }
}

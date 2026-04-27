// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Memory manager for page-level memory operations.

use wasm_dbms_api::prelude::{
    DataSize, Encode, MSize, MemoryError, MemoryResult, Page, PageOffset,
};

use crate::memory_access::MemoryAccess;
use crate::provider::MemoryProvider;

/// Schema page (reserved page 0).
pub const SCHEMA_PAGE: Page = 0;
/// ACL page (reserved page 1).
pub const ACL_PAGE: Page = 1;
/// Unclaimed-pages ledger page (reserved page 2).
pub const UNCLAIMED_PAGES_PAGE: Page = 2;
/// Number of reserved pages allocated at initialization.
pub const RESERVED_PAGES: u64 = 3;

/// The memory manager handles page-level memory operations on top of a
/// [`MemoryProvider`].
pub struct MemoryManager<P>
where
    P: MemoryProvider,
{
    provider: P,
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
        let mut manager = MemoryManager { provider };

        // Check whether the reserved pages are already allocated.
        let current_pages = manager.provider.pages();
        if current_pages >= RESERVED_PAGES {
            return manager;
        }

        // Request the missing reserved pages (schema, ACL, unclaimed).
        let missing = RESERVED_PAGES - current_pages;
        if let Err(err) = manager.provider.grow(missing) {
            panic!("Failed to grow memory during initialization: {err}");
        }

        manager
    }

    /// Returns the ACL page number.
    pub const fn acl_page(&self) -> Page {
        ACL_PAGE
    }

    /// Returns the schema page.
    pub const fn schema_page(&self) -> Page {
        SCHEMA_PAGE
    }

    /// Returns the unclaimed-pages ledger page.
    pub const fn unclaimed_pages_page(&self) -> Page {
        UNCLAIMED_PAGES_PAGE
    }

    /// Consumes the manager and returns the underlying provider.
    ///
    /// Test-only helper that enables reload simulations without going
    /// through the full DBMS context.
    #[cfg(test)]
    pub(crate) fn into_provider(self) -> P {
        self.provider
    }

    /// Gets the last allocated page number.
    pub fn last_page(&self) -> Option<Page> {
        match self.provider.pages() {
            0 => None,
            n => Some(n as Page - 1),
        }
    }

    /// Returns the total number of pages currently backed by the provider.
    pub fn pages_count(&self) -> u64 {
        self.provider.pages()
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

impl<P> MemoryAccess for MemoryManager<P>
where
    P: MemoryProvider,
{
    fn page_size(&self) -> u64 {
        P::PAGE_SIZE
    }

    fn grow_one_page(&mut self) -> MemoryResult<Page> {
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

    fn zero_page(&mut self, page: Page) -> MemoryResult<()> {
        if self.last_page().is_none_or(|last_page| page > last_page) {
            return Err(MemoryError::SegmentationFault {
                page,
                offset: 0,
                data_size: P::PAGE_SIZE,
                page_size: P::PAGE_SIZE,
            });
        }

        let absolute_offset = self.absolute_offset(page, 0);
        let buffer = vec![0u8; P::PAGE_SIZE as usize];
        self.provider.write(absolute_offset, &buffer)
    }

    fn read_at<D>(&mut self, page: Page, offset: PageOffset) -> MemoryResult<D>
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

    fn write_at<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
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
        self.provider.write(absolute_offset, encoded.as_ref())?;

        // Zero padding bytes if any.
        let padding = align_up::<E>(encoded.len()) - encoded.len();
        if padding > 0 {
            let padding_offset = absolute_offset + encoded.len() as u64;
            let padding_buffer = vec![0u8; padding];
            self.provider
                .write(padding_offset, padding_buffer.as_ref())?;
        }

        Ok(())
    }

    fn write_at_raw(&mut self, page: Page, offset: PageOffset, buf: &[u8]) -> MemoryResult<()> {
        self.check_unallocated_page(page, offset, buf.len() as MSize)?;

        if offset as u64 + buf.len() as u64 > P::PAGE_SIZE {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: buf.len() as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        let absolute_offset = self.absolute_offset(page, offset);
        self.provider.write(absolute_offset, buf)
    }

    fn zero<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
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
        self.provider.write(absolute_offset, buffer.as_ref())
    }

    fn zero_raw(&mut self, page: Page, offset: PageOffset, len: PageOffset) -> MemoryResult<()> {
        if self.last_page().is_none_or(|last_page| page > last_page) {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: len as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        if offset as u64 + len as u64 > P::PAGE_SIZE {
            return Err(MemoryError::SegmentationFault {
                page,
                offset,
                data_size: len as u64,
                page_size: P::PAGE_SIZE,
            });
        }

        let absolute_offset = self.absolute_offset(page, offset);
        let buffer = vec![0u8; len as usize];
        self.provider.write(absolute_offset, buffer.as_ref())
    }

    fn read_at_raw(
        &mut self,
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
        assert_eq!(mm.last_page(), Some(2));
    }

    #[test]
    fn test_should_get_last_page() {
        let mm = make_mm();
        let last_page = mm.last_page();
        assert_eq!(last_page, Some(2)); // schema, ACL, and unclaimed-pages pages
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
        let mut mm = make_mm();
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
        mm.write_at_raw(ACL_PAGE, 20, &data_to_write)
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
    fn test_should_claim_new_page_by_growing() {
        let mut mm = make_mm();
        let initial_last_page = mm.last_page().unwrap();
        let new_page = mm.claim_page().expect("Failed to claim new page");
        assert_eq!(new_page, initial_last_page + 1);
        let updated_last_page = mm.last_page().unwrap();
        assert_eq!(updated_last_page, new_page);
    }

    #[test]
    fn test_unclaim_then_claim_returns_same_page() {
        let mut mm = make_mm();
        let first = mm.claim_page().expect("claim");
        // Write a non-zero pattern so we can assert it's zeroed on unclaim.
        mm.write_at_raw(first, 0, &[0xAB, 0xCD, 0xEF, 0x42])
            .expect("write");

        mm.unclaim_page(first).expect("unclaim");

        // The reserved page should hand the same page back.
        let reused = mm.claim_page().expect("claim again");
        assert_eq!(reused, first);

        // Page contents must be zeroed by unclaim.
        let mut buf = [0u8; 4];
        mm.read_at_raw(reused, 0, &mut buf).expect("read");
        assert_eq!(buf, [0u8; 4]);
    }

    #[test]
    fn test_claim_after_exhausting_unclaimed_grows_memory() {
        let mut mm = make_mm();
        let page_a = mm.claim_page().expect("claim a");
        let page_b = mm.claim_page().expect("claim b");

        mm.unclaim_page(page_a).expect("unclaim a");
        mm.unclaim_page(page_b).expect("unclaim b");

        // Pop both unclaimed pages. The third claim must grow.
        let _ = mm.claim_page().expect("reuse 1");
        let _ = mm.claim_page().expect("reuse 2");
        let high_water_before = mm.last_page().unwrap();
        let grown = mm.claim_page().expect("grow");
        assert_eq!(grown, high_water_before + 1);
    }

    #[test]
    fn test_unclaim_persists_across_reload() {
        let mut provider = HeapMemoryProvider::default();
        {
            let mut mm = MemoryManager::init(provider);
            let first = mm.claim_page().expect("claim");
            mm.unclaim_page(first).expect("unclaim");
            provider = mm.into_provider();
        }
        let mut mm = MemoryManager::init(provider);
        let reused = mm.claim_page().expect("claim after reload");
        assert_eq!(reused, RESERVED_PAGES as Page);
    }

    #[test]
    fn test_zero_page_zeros_full_page_contents() {
        let mut mm = make_mm();
        let page = mm.claim_page().expect("claim");
        // Write to several offsets across the page.
        mm.write_at_raw(page, 0, &[1, 2, 3, 4]).expect("write 0");
        mm.write_at_raw(page, 30_000, &[9, 9]).expect("write 30k");

        mm.zero_page(page).expect("zero_page");

        let mut buf = [0u8; 4];
        mm.read_at_raw(page, 0, &mut buf).expect("read");
        assert_eq!(buf, [0u8; 4]);
        let mut buf = [0u8; 2];
        mm.read_at_raw(page, 30_000, &mut buf).expect("read");
        assert_eq!(buf, [0u8; 2]);
    }

    #[test]
    fn test_zero_page_rejects_unallocated_page() {
        let mut mm = make_mm();
        let result = mm.zero_page(99);
        assert!(matches!(result, Err(MemoryError::SegmentationFault { .. })));
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
}

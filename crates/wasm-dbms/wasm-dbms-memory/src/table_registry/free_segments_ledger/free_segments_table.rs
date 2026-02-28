// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryResult, Page, PageOffset,
};

use super::FreeSegment;
use crate::{MemoryManager, MemoryProvider};

const TABLE_LEN_SIZE: MSize = 2;

/// [`Encode`]able representation of a table that keeps track of [`FreeSegment`]s.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct FreeSegmentsTable {
    max_records: usize,
    records: FreeSegmentsList,
    page: Page,
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct FreeSegmentsList(Vec<FreeSegment>);

impl FreeSegmentsTable {
    /// Loads the [`FreeSegmentsTable`] from the given page.
    pub fn load(page: Page, mm: &MemoryManager<impl MemoryProvider>) -> MemoryResult<Self> {
        let records = mm.read_at(page, 0)?;
        let max_records = Self::max_segments(mm);
        Ok(Self {
            max_records,
            page,
            records,
        })
    }

    /// Returns the page where the table is stored.
    pub fn page(&self) -> Page {
        self.page
    }

    /// Checks if the table is full.
    pub fn is_full(&self) -> bool {
        self.records.0.len() >= self.max_records
    }

    /// Inserts a new [`FreeSegment`] into the table.
    ///
    /// # Panics
    ///
    /// Panics if the table is full.
    pub fn insert_free_segment(
        &mut self,
        page: Page,
        offset: PageOffset,
        size: MSize,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<()> {
        assert!(
            !self.is_full(),
            "Cannot insert new free segment: table is full"
        );
        // check for adjacent segments and merge if found
        if let Some(adjacent) = self.has_adjacent_segment(page, offset, size) {
            match adjacent {
                AdjacentSegment::Before(seg) => {
                    // Merge with the segment before
                    let new_size = seg.size.saturating_add(size);
                    self.remove(seg, seg.size, mm)?;
                    self.insert_free_segment(page, seg.offset, new_size, mm)?;
                }
                AdjacentSegment::After(seg) => {
                    // Merge with the segment after
                    let new_size = size.saturating_add(seg.size);
                    self.remove(seg, seg.size, mm)?;
                    self.insert_free_segment(page, offset, new_size, mm)?;
                }
            }
        } else {
            // No adjacent segments found, insert as is
            let record = FreeSegment { page, offset, size };
            self.records.0.push(record);
        }
        self.commit(mm)
    }

    /// Finds a free segment that matches the given predicate.
    pub fn find<F>(&self, predicate: F) -> Option<FreeSegment>
    where
        F: Fn(&&FreeSegment) -> bool,
    {
        self.records.0.iter().find(predicate).copied()
    }

    /// Removes a free segment that matches the given parameters.
    ///
    /// If `used_size` is less than `size`, the old record is removed, but a new record is added
    /// for the remaining free space.
    pub fn remove(
        &mut self,
        FreeSegment { page, offset, size }: FreeSegment,
        used_size: MSize,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<()> {
        if let Some(pos) = self
            .records
            .0
            .iter()
            .position(|r| r.page == page && r.offset == offset && r.size == size)
        {
            self.records.0.swap_remove(pos);

            // If there is remaining space, add a new record for it.
            if used_size < size {
                let remaining_size = size.saturating_sub(used_size);
                let new_offset = offset.saturating_add(used_size);
                let new_record = FreeSegment {
                    page,
                    offset: new_offset,
                    size: remaining_size,
                };
                self.records.0.push(new_record);
            }
            self.commit(mm)?;
        }

        Ok(())
    }

    /// Commits the current state of the table back to memory.
    fn commit(&self, mm: &mut MemoryManager<impl MemoryProvider>) -> MemoryResult<()> {
        mm.write_at(self.page, 0, &self.records)
    }

    /// Checks for adjacent free segments before or after the given segment.
    fn has_adjacent_segment(
        &self,
        page: Page,
        offset: PageOffset,
        size: MSize,
    ) -> Option<AdjacentSegment> {
        self.has_adjacent_segment_before(page, offset)
            .or_else(|| self.has_adjacent_segment_after(page, offset, size))
    }

    /// Checks for an adjacent free segment before the given segment.
    fn has_adjacent_segment_before(
        &self,
        page: Page,
        offset: PageOffset,
    ) -> Option<AdjacentSegment> {
        self.find(|r| r.page == page && r.offset.saturating_add(r.size) == offset)
            .map(AdjacentSegment::Before)
    }

    /// Checks for an adjacent free segment after the given segment.
    fn has_adjacent_segment_after(
        &self,
        page: Page,
        offset: PageOffset,
        size: MSize,
    ) -> Option<AdjacentSegment> {
        self.find(|r| r.page == page && r.offset == offset.saturating_add(size))
            .map(AdjacentSegment::After)
    }

    /// Computes the maximum number of segments that can be stored in a single page.
    fn max_segments(mm: &MemoryManager<impl MemoryProvider>) -> usize {
        let page_size = mm.page_size();
        let record_size = FreeSegment::SIZE.get_fixed_size().expect("Should be fixed") as u64;
        page_size.div_ceil(record_size).saturating_sub(1) as usize // for header
    }
}

/// Represents an adjacent free segment, either before or after a given segment.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AdjacentSegment {
    Before(FreeSegment),
    After(FreeSegment),
}

impl Encode for FreeSegmentsList {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut buffer = Vec::with_capacity(self.size() as usize);

        // Encode the length of the records vector.
        let length = self.0.len() as MSize;
        buffer.extend_from_slice(&length.to_le_bytes());

        // Encode each DeletedRecord.
        for record in &self.0 {
            buffer.extend_from_slice(&record.encode());
        }

        std::borrow::Cow::Owned(buffer)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let length = MSize::from_le_bytes(data[0..TABLE_LEN_SIZE as usize].try_into()?);
        let mut records = Vec::with_capacity(length as usize);
        let record_size = FreeSegment::SIZE.get_fixed_size().expect("Should be fixed");

        let mut offset = TABLE_LEN_SIZE;
        for _ in 0..length {
            let record_data = data[offset as usize..(offset + record_size) as usize]
                .to_vec()
                .into();
            let record = FreeSegment::decode(record_data)?;
            records.push(record);
            offset += record_size;
        }

        Ok(Self(records))
    }

    fn size(&self) -> MSize {
        let mut size = TABLE_LEN_SIZE;
        for record in &self.0 {
            size = size.saturating_add(record.size); // This saturating won't happen, but just to prevent panic...
        }
        size
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::HeapMemoryProvider;

    #[test]
    fn test_should_encode_and_decode_free_segments_table() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 0, 50, &mut mm)
            .expect("insert_free_segment");
        table
            .insert_free_segment(2, 100, 75, &mut mm)
            .expect("Insert failed");

        let encoded = table.records.encode();
        let decoded = FreeSegmentsList::decode(encoded).expect("Decoding failed");
        assert_eq!(table.records, decoded);
    }

    #[test]
    fn test_should_insert_and_remove_free_segments() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);

        table
            .insert_free_segment(1, 0, 100, &mut mm)
            .expect("Insert failed");
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed"); // Adjacent segment, should merge

        assert_eq!(table.records.0.len(), 1);
        assert_eq!(
            table.records.0[0],
            FreeSegment {
                page: 1,
                offset: 0,
                size: 150
            }
        );

        table
            .remove(
                FreeSegment {
                    page: 1,
                    offset: 0,
                    size: 150,
                },
                100,
                &mut mm,
            )
            .expect("remove failed");

        assert_eq!(table.records.0.len(), 1);
        assert_eq!(
            table.records.0[0],
            FreeSegment {
                page: 1,
                offset: 100,
                size: 50
            }
        );
    }

    #[test]
    fn test_should_find_free_segment() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 75, &mut mm)
            .expect("Insert failed");
        table
            .insert_free_segment(2, 100, 75, &mut mm)
            .expect("Insert failed");

        let found = table.find(|r| r.page == 2 && r.offset == 100);
        assert_eq!(
            found,
            Some(FreeSegment {
                page: 2,
                offset: 100,
                size: 75
            })
        );
    }

    #[test]
    fn test_should_compute_max_segments() {
        let mm = MemoryManager::init(HeapMemoryProvider::default());
        // test size of page is 65536 => 65536 / 8 - 1 = 8191
        let expected_max = 8191;

        let computed_max = FreeSegmentsTable::max_segments(&mm);
        assert_eq!(expected_max, computed_max);
    }

    #[test]
    #[should_panic(expected = "Cannot insert new free segment: table is full")]
    fn test_should_panic_when_inserting_into_full_table() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table_with_size(&mut mm, 1);

        table
            .insert_free_segment(1, 0, 100, &mut mm)
            .expect("Insert failed");
        // This should panic
        table
            .insert_free_segment(1, 150, 50, &mut mm)
            .expect("Insert failed");
    }

    #[test]
    fn test_should_not_merge_non_adjacent_segments() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 0, 50, &mut mm)
            .expect("Insert failed");
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed"); // Non-adjacent
        assert_eq!(table.records.0.len(), 2);
    }

    #[test]
    fn test_should_handle_removal_of_nonexistent_segment_gracefully() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 0, 50, &mut mm)
            .expect("Insert failed");
        // Attempt to remove a segment that doesn't exist
        table
            .remove(
                FreeSegment {
                    page: 2,
                    offset: 0,
                    size: 50,
                },
                25,
                &mut mm,
            )
            .expect("remove failed");
        // Table should remain unchanged
        assert_eq!(table.records.0.len(), 1);
        assert_eq!(
            table.records.0[0],
            FreeSegment {
                page: 1,
                offset: 0,
                size: 50
            }
        );
    }

    #[test]
    fn test_should_tell_if_table_is_full() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table_with_size(&mut mm, 2);
        assert!(!table.is_full());
        table
            .insert_free_segment(1, 0, 50, &mut mm)
            .expect("Insert failed");
        assert!(!table.is_full());
        table
            .insert_free_segment(1, 150, 50, &mut mm)
            .expect("Insert failed");
        assert!(table.is_full());
    }

    #[test]
    fn test_should_remove_free_segment_with_same_size() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed");

        table
            .remove(
                FreeSegment {
                    page: 1,
                    offset: 100,
                    size: 50,
                },
                50,
                &mut mm,
            )
            .expect("remove failed");

        assert!(table.records.0.is_empty());
    }

    #[test]
    fn test_should_remove_free_segment_and_create_remaining() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed");

        table
            .remove(
                FreeSegment {
                    page: 1,
                    offset: 100,
                    size: 50,
                },
                30,
                &mut mm,
            )
            .expect("remove failed");

        assert_eq!(table.records.0.len(), 1);
        assert_eq!(table.records.0[0].page, 1);
        assert_eq!(table.records.0[0].offset, 130);
        assert_eq!(table.records.0[0].size, 20);
    }

    #[test]
    fn test_should_find_adjacent_segment_before() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed");

        let adjacent = table.has_adjacent_segment_before(1, 150);
        assert!(adjacent.is_some());
        match adjacent.unwrap() {
            AdjacentSegment::Before(seg) => {
                assert_eq!(seg.page, 1);
                assert_eq!(seg.offset, 100);
                assert_eq!(seg.size, 50);
            }
            _ => panic!("Expected AdjacentSegment::Before"),
        }
    }

    #[test]
    fn test_should_find_adjacent_segment_after() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed");

        let adjacent = table.has_adjacent_segment_after(1, 0, 100);
        assert!(adjacent.is_some());
        match adjacent.unwrap() {
            AdjacentSegment::After(seg) => {
                assert_eq!(seg.page, 1);
                assert_eq!(seg.offset, 100);
                assert_eq!(seg.size, 50);
            }
            _ => panic!("Expected AdjacentSegment::After"),
        }
    }

    #[test]
    fn test_should_insert_adjacent_segment() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 100, 50, &mut mm)
            .expect("Insert failed");
        table
            .insert_free_segment(1, 150, 50, &mut mm)
            .expect("Insert failed"); // Adjacent to the first

        assert_eq!(table.records.0.len(), 1);
        assert_eq!(table.records.0[0].page, 1);
        assert_eq!(table.records.0[0].offset, 100);
        assert_eq!(table.records.0[0].size, 100); // Merged size
    }

    #[test]
    fn test_should_commit_after_each_insert() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 0, 50, &mut mm)
            .expect("Insert failed");

        // Load the table again to verify persistence
        let reloaded_table = FreeSegmentsTable::load(table.page, &mm).expect("Reload failed");
        assert_eq!(reloaded_table.records.0.len(), 1);
        assert_eq!(
            reloaded_table.records.0[0],
            FreeSegment {
                page: 1,
                offset: 0,
                size: 50
            }
        );
    }

    #[test]
    fn test_should_commit_after_remove() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut table = mock_table(&mut mm);
        table
            .insert_free_segment(1, 0, 100, &mut mm)
            .expect("Insert failed");

        let mut table = FreeSegmentsTable::load(table.page, &mm).expect("Reload failed");
        assert_eq!(table.records.0.len(), 1);

        table
            .remove(
                FreeSegment {
                    page: 1,
                    offset: 0,
                    size: 100,
                },
                100,
                &mut mm,
            )
            .expect("remove failed");

        // Load the table again to verify persistence
        let reloaded_table = FreeSegmentsTable::load(table.page, &mm).expect("Reload failed");
        assert!(reloaded_table.records.0.is_empty());
    }

    #[test]
    fn test_should_get_page() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table = mock_table(&mut mm);
        let page = table.page();
        assert_eq!(table.page, page);
    }

    fn mock_table(mm: &mut MemoryManager<HeapMemoryProvider>) -> FreeSegmentsTable {
        let max = FreeSegmentsTable::max_segments(mm);
        mock_table_with_size(mm, max)
    }

    fn mock_table_with_size(
        mm: &mut MemoryManager<HeapMemoryProvider>,
        max_records: usize,
    ) -> FreeSegmentsTable {
        // create a page
        let page = mm.allocate_page().expect("alloc failed");
        FreeSegmentsTable {
            records: FreeSegmentsList::default(),
            max_records,
            page,
        }
    }
}

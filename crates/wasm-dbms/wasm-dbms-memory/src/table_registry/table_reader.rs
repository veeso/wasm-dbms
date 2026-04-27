// Rust guideline compliant 2026-02-28

use std::marker::PhantomData;

use wasm_dbms_api::prelude::{
    DecodeError, Encode, MSize, MemoryError, MemoryResult, Page, PageOffset,
};

use super::page_ledger::PageLedger;
use super::raw_record::{RAW_RECORD_HEADER_SIZE, RawRecord};
use crate::{MemoryAccess, align_up};

/// Stores the current position to read/write in memory.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Position {
    page: Page,
    offset: PageOffset,
}

/// Represents the next record to read from memory.
/// It also contains the new [`Position`] after reading the record.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct FoundRecord {
    page: Page,
    offset: PageOffset,
    length: MSize,
    new_position: Option<Position>,
}

/// Represents the next record read by the [`TableReader`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct NextRecord<E>
where
    E: Encode,
{
    pub record: E,
    pub page: Page,
    pub offset: PageOffset,
}

/// A reader for the table registry that allows reading records from memory.
///
/// The table reader provides methods to read records from the table registry one by one,
/// using the underlying [`PageLedger`] to locate the records in memory.
pub struct TableReader<'a, E, MA>
where
    E: Encode,
    MA: MemoryAccess,
{
    /// Buffer used to read records from memory.
    buffer: Vec<u8>,
    /// Reference to the memory access implementor.
    mm: &'a mut MA,
    page_ledger: &'a PageLedger,
    page_size: usize,
    phantom: PhantomData<E>,
    /// Current position in the table registry.
    /// If `None`, the reader has reached the end of the table.
    position: Option<Position>,
}

impl<'a, E, MA> TableReader<'a, E, MA>
where
    E: Encode,
    MA: MemoryAccess,
{
    /// Creates a new table reader starting from the beginning of the table registry.
    pub fn new(page_ledger: &'a PageLedger, mm: &'a mut MA) -> Self {
        // init position
        let position = page_ledger.pages().first().map(|page_record| Position {
            page: page_record.page,
            offset: 0,
        });
        let page_size = mm.page_size() as usize;
        Self {
            buffer: vec![0u8; page_size],
            mm,
            page_ledger,
            phantom: PhantomData,
            position,
            page_size,
        }
    }

    /// Reads the next record from the table registry.
    pub fn try_next(&mut self) -> MemoryResult<Option<NextRecord<E>>> {
        let Some(Position { page, offset }) = self.position else {
            return Ok(None);
        };

        // find next record segment
        let Some(next_record) = self.find_next_record(page, offset)? else {
            // no more records
            self.position = None;
            return Ok(None);
        };

        // read raw record
        let record: RawRecord<E> = self.mm.read_at(next_record.page, next_record.offset)?;

        // update position
        self.position = next_record.new_position;

        Ok(Some(NextRecord {
            record: record.data,
            page: next_record.page,
            offset: next_record.offset,
        }))
    }

    /// Finds the next record starting from the given position.
    ///
    /// If a record is found, returns [`Some<NextRecord<E>>`], otherwise returns [`None`].
    /// If [`None`] is returned, the reader has reached the end of the table.
    fn find_next_record(
        &mut self,
        mut page: Page,
        mut offset: PageOffset,
    ) -> MemoryResult<Option<FoundRecord>> {
        loop {
            // if offset is zero, read page; otherwise, just reuse buffer
            if offset == 0 {
                self.mm.read_at_raw(page, 0, &mut self.buffer)?;
            }

            // find next record in buffer; if found, return it
            if let Some((next_segment_offset, next_segment_size)) =
                self.find_next_record_position(&self.buffer, offset as usize)?
            {
                // found a record; return it
                let new_offset = next_segment_offset + next_segment_size as PageOffset;
                let new_position = if new_offset as usize >= self.page_size {
                    // move to next page
                    self.next_page(page)
                } else {
                    Some(Position {
                        page,
                        offset: new_offset,
                    })
                };
                return Ok(Some(FoundRecord {
                    page,
                    offset: next_segment_offset,
                    length: next_segment_size,
                    new_position,
                }));
            }

            // read next page
            match self.next_page(page) {
                Some(pos) => {
                    page = pos.page;
                    offset = pos.offset;
                }
                None => break,
            }
        }

        Ok(None)
    }

    /// Gets the next page after the given current page.
    fn next_page(&self, current_page: Page) -> Option<Position> {
        self.page_ledger
            .pages()
            .iter()
            .find(|p| p.page > current_page)
            .map(|page_record| Position {
                page: page_record.page,
                offset: 0,
            })
    }

    /// Finds the next record segment position.
    ///
    /// This is done by starting from the current offset
    /// and searching for each multiple of [`E::ALIGNMENT`] until we find a size different from `0x00`.
    ///
    /// Returns the offset and size of the next record segment if found.
    fn find_next_record_position(
        &self,
        buf: &[u8],
        mut offset: usize,
    ) -> MemoryResult<Option<(PageOffset, MSize)>> {
        // first round the offset to the next alignment
        offset = align_up::<RawRecord<E>>(offset);
        // search for the first non-zero record length
        let mut data_len;
        loop {
            // check whether we are at the end of the page
            if offset + 1 >= buf.len() {
                return Ok(None);
            }
            // read next two bytes
            data_len = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as MSize;
            if data_len != 0 {
                break;
            }
            // move to next alignment
            offset += E::ALIGNMENT as usize;
        }

        let data_offset = offset + RAW_RECORD_HEADER_SIZE as usize;
        if buf.len() < data_offset + data_len as usize {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        Ok(Some((
            offset as PageOffset,
            data_len + RAW_RECORD_HEADER_SIZE,
        )))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::table_registry::test_utils::User;
    use crate::{HeapMemoryProvider, MemoryManager, TableRegistry, TableRegistryPage};

    #[test]
    fn test_should_read_all_records() {
        const COUNT: u32 = 4_000;
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(COUNT, &mut mm);
        let mut reader = mocked(&table_registry, &mut mm);

        // should read all records
        let mut id = 0;
        while let Some(NextRecord { record: user, .. }) =
            reader.try_next().expect("failed to read user")
        {
            assert_eq!(user.id, id);
            assert_eq!(user.name, format!("User {}", id));

            id += 1;
        }
        assert_eq!(id, COUNT);
    }

    #[test]
    fn test_should_find_next_page() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(4_000, &mut mm);
        let reader = mocked(&table_registry, &mut mm);

        let page = reader.position.expect("should have position").page;

        let next_page = reader.next_page(page).expect("should have next page");
        assert_eq!(next_page.page, page + 1);
        let next_page = reader.next_page(next_page.page);
        assert!(next_page.is_some());
        let next_page = reader.next_page(next_page.unwrap().page);
        assert!(next_page.is_some());
        let next_page = reader.next_page(next_page.unwrap().page);
        assert!(
            next_page.is_none(),
            "should not have next page, but got {:?}",
            next_page
        );
    }

    #[test]
    fn test_should_find_next_record_position() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(1, &mut mm);
        let reader = mocked(&table_registry, &mut mm);

        let mut buf = vec![0u8; User::ALIGNMENT as usize];
        buf.extend_from_slice(&[5u8, 0u8, 0u8, 0, 0, 0, 0, 0, 0]);

        let (offset, size) = reader
            .find_next_record_position(&buf, 0)
            .expect("failed to get next record")
            .expect("should have next record");

        assert_eq!(offset, 32);
        assert_eq!(size, 7);
    }

    #[test]
    fn test_should_not_find_next_record_position_none() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(1, &mut mm);
        let reader = mocked(&table_registry, &mut mm);

        let buf = vec![0u8; User::ALIGNMENT as usize * 2];
        let result = reader
            .find_next_record_position(&buf, 0)
            .expect("failed to get next record");

        assert!(result.is_none());
    }

    #[test]
    fn test_should_not_find_next_record_position_too_short_for_length() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(1, &mut mm);
        let reader = mocked(&table_registry, &mut mm);

        let buf = [5u8, 16u8];
        let result = reader.find_next_record_position(&buf, 0);
        assert!(result.is_err(), "expected error but got {:?}", result);
        let err = result.unwrap_err();

        assert!(matches!(
            err,
            MemoryError::DecodeError(DecodeError::TooShort)
        ));
    }

    #[test]
    fn test_should_not_find_next_record_position_too_short_for_data() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let table_registry = mock_table_registry(1, &mut mm);
        let reader = mocked(&table_registry, &mut mm);

        let buf = [5u8, 0u8, 0u8, 0, 0];
        let result = reader.find_next_record_position(&buf, 0);

        assert!(matches!(
            result,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    fn mock_table_registry(
        entries: u32,
        mm: &mut MemoryManager<HeapMemoryProvider>,
    ) -> TableRegistry {
        let schema_snapshot_page = mm.claim_page().expect("failed to get page");
        let page_ledger_page = mm.claim_page().expect("failed to get page");
        let free_segments_page = mm.claim_page().expect("failed to get page");
        let index_registry_page = mm.claim_page().expect("failed to get page");
        super::super::test_utils::write_dummy_schema_snapshot(schema_snapshot_page, mm);
        let mut registry = TableRegistry::load(
            TableRegistryPage {
                schema_snapshot_page,
                pages_list_page: page_ledger_page,
                free_segments_page,
                index_registry_page,
                autoincrement_registry_page: None,
            },
            mm,
        )
        .expect("failed to load registry");

        // insert `entries` records
        for id in 0..entries {
            let user = User {
                id,
                name: format!("User {}", id),
                email: "new_user@example.com".to_string(),
                age: 20 + id,
            };
            registry.insert(user, mm).expect("failed to insert user");
        }

        registry
    }

    fn mocked<'a>(
        table_registry: &'a TableRegistry,
        mm: &'a mut MemoryManager<HeapMemoryProvider>,
    ) -> TableReader<'a, User, MemoryManager<HeapMemoryProvider>> {
        TableReader::new(&table_registry.page_ledger, mm)
    }
}

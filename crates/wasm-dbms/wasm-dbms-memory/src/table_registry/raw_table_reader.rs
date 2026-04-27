// Rust guideline compliant 2026-04-27

//! Raw-bytes table reader.
//!
//! Mirrors [`TableReader`](super::TableReader) but yields the encoded record
//! bytes plus its [`RecordAddress`], so callers can decode via a snapshot-
//! driven decoder (used by the migration apply pipeline to read records
//! under the **stored** snapshot independent of the compile-time `T`).

use wasm_dbms_api::prelude::{DecodeError, MSize, MemoryError, MemoryResult, Page, PageOffset};

use super::page_ledger::PageLedger;
use super::raw_record::RAW_RECORD_HEADER_SIZE;
use super::record_address::RecordAddress;
use crate::MemoryAccess;

/// Yielded by [`RawTableReader::try_next`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawRecordBytes {
    /// Address of the record (page + aligned offset to the length header).
    pub address: RecordAddress,
    /// Decoded record body bytes (header stripped).
    pub bytes: Vec<u8>,
}

/// Iterator over a table's records as raw bytes. Mirrors the scan logic of
/// [`TableReader`](super::TableReader) but parameterised over runtime
/// alignment (the migration codec slices records by snapshot, not by `T`).
pub struct RawTableReader<'a, MA>
where
    MA: MemoryAccess,
{
    mm: &'a mut MA,
    page_ledger: &'a PageLedger,
    page_size: usize,
    alignment: PageOffset,
    cursor: Option<Cursor>,
}

#[derive(Debug, Copy, Clone)]
struct Cursor {
    page: Page,
    offset: PageOffset,
}

impl<'a, MA> RawTableReader<'a, MA>
where
    MA: MemoryAccess,
{
    /// Build a reader. `alignment` must be the table's record alignment as
    /// stored in the snapshot (matches the on-disk layout written by
    /// `TableRegistry::insert`).
    pub fn new(page_ledger: &'a PageLedger, alignment: PageOffset, mm: &'a mut MA) -> Self {
        let page_size = mm.page_size() as usize;
        let cursor = page_ledger.pages().first().map(|p| Cursor {
            page: p.page,
            offset: 0,
        });
        Self {
            mm,
            page_ledger,
            page_size,
            alignment,
            cursor,
        }
    }

    /// Pop the next live record's bytes, or `Ok(None)` at end of table.
    pub fn try_next(&mut self) -> MemoryResult<Option<RawRecordBytes>> {
        loop {
            let Some(Cursor { page, offset }) = self.cursor else {
                return Ok(None);
            };
            let aligned_usize = align_up_usize(offset as usize, self.alignment as usize);
            // No room for a header on this page → advance to next page.
            if aligned_usize + (RAW_RECORD_HEADER_SIZE as usize) > self.page_size {
                self.cursor = self.next_page(page);
                continue;
            }
            let aligned = aligned_usize as PageOffset;
            let mut header = [0u8; RAW_RECORD_HEADER_SIZE as usize];
            self.mm.read_at_raw(page, aligned, &mut header)?;
            let length = u16::from_le_bytes(header) as MSize;
            if length == 0 {
                // Empty slot — skip one alignment.
                let next_offset = aligned_usize + self.alignment as usize;
                self.cursor = if next_offset >= self.page_size {
                    self.next_page(page)
                } else {
                    Some(Cursor {
                        page,
                        offset: next_offset as PageOffset,
                    })
                };
                continue;
            }
            let body_offset_usize = aligned_usize + RAW_RECORD_HEADER_SIZE as usize;
            if body_offset_usize + (length as usize) > self.page_size {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let body_offset = body_offset_usize as PageOffset;
            let mut body = vec![0u8; length as usize];
            self.mm.read_at_raw(page, body_offset, &mut body)?;
            let address = RecordAddress {
                page,
                offset: aligned,
            };
            // Advance past the record body, aligned to the next slot.
            let next_offset =
                align_up_usize(body_offset_usize + length as usize, self.alignment as usize);
            self.cursor = if next_offset >= self.page_size {
                self.next_page(page)
            } else {
                Some(Cursor {
                    page,
                    offset: next_offset as PageOffset,
                })
            };
            return Ok(Some(RawRecordBytes {
                address,
                bytes: body,
            }));
        }
    }

    fn next_page(&self, current: Page) -> Option<Cursor> {
        self.page_ledger
            .pages()
            .iter()
            .find(|p| p.page > current)
            .map(|p| Cursor {
                page: p.page,
                offset: 0,
            })
    }
}

fn align_up_usize(offset: usize, alignment: usize) -> usize {
    if alignment == 0 {
        return offset;
    }
    let rem = offset % alignment;
    if rem == 0 {
        offset
    } else {
        offset + (alignment - rem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::table_registry::test_utils::{User, write_dummy_schema_snapshot};
    use crate::{HeapMemoryProvider, MemoryManager, TableRegistry, TableRegistryPage};

    #[test]
    fn test_insert_raw_round_trips_through_raw_reader() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let schema_snapshot_page = mm.allocate_page().unwrap();
        let pages_list_page = mm.allocate_page().unwrap();
        let free_segments_page = mm.allocate_page().unwrap();
        let index_registry_page = mm.allocate_page().unwrap();
        write_dummy_schema_snapshot(schema_snapshot_page, &mut mm);

        let mut registry = TableRegistry::load(
            TableRegistryPage {
                schema_snapshot_page,
                pages_list_page,
                free_segments_page,
                index_registry_page,
                autoincrement_registry_page: None,
            },
            &mut mm,
        )
        .unwrap();

        let alignment = 32u16;
        let payload = vec![1u8, 2, 3, 4, 5, 6, 7];
        let address = registry.insert_raw(&payload, alignment, &mut mm).unwrap();
        let read_back = registry.read_raw_at(address, &mut mm).unwrap();
        assert_eq!(read_back, payload);

        let mut reader = RawTableReader::new(&registry.page_ledger, alignment, &mut mm);
        let row = reader.try_next().unwrap().expect("missing row");
        assert_eq!(row.bytes, payload);
        assert!(reader.try_next().unwrap().is_none());
    }

    #[test]
    fn test_delete_raw_frees_segment() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let schema_snapshot_page = mm.allocate_page().unwrap();
        let pages_list_page = mm.allocate_page().unwrap();
        let free_segments_page = mm.allocate_page().unwrap();
        let index_registry_page = mm.allocate_page().unwrap();
        write_dummy_schema_snapshot(schema_snapshot_page, &mut mm);

        let mut registry = TableRegistry::load(
            TableRegistryPage {
                schema_snapshot_page,
                pages_list_page,
                free_segments_page,
                index_registry_page,
                autoincrement_registry_page: None,
            },
            &mut mm,
        )
        .unwrap();

        let alignment = 32u16;
        let bytes = vec![9u8; 10];
        let addr = registry.insert_raw(&bytes, alignment, &mut mm).unwrap();
        registry
            .delete_raw(addr, bytes.len() as MSize, alignment, &mut mm)
            .unwrap();

        let mut reader = RawTableReader::new(&registry.page_ledger, alignment, &mut mm);
        assert!(reader.try_next().unwrap().is_none());
    }

    #[test]
    fn test_raw_table_reader_reads_all_records_as_bytes() {
        use wasm_dbms_api::prelude::Encode;

        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let schema_snapshot_page = mm.allocate_page().unwrap();
        let pages_list_page = mm.allocate_page().unwrap();
        let free_segments_page = mm.allocate_page().unwrap();
        let index_registry_page = mm.allocate_page().unwrap();
        write_dummy_schema_snapshot(schema_snapshot_page, &mut mm);

        let mut registry = TableRegistry::load(
            TableRegistryPage {
                schema_snapshot_page,
                pages_list_page,
                free_segments_page,
                index_registry_page,
                autoincrement_registry_page: None,
            },
            &mut mm,
        )
        .unwrap();

        for id in 0..3u32 {
            let user = User {
                id,
                name: format!("U{id}"),
                email: "x@x".into(),
                age: 20,
            };
            registry.insert(user, &mut mm).unwrap();
        }

        let mut reader = RawTableReader::new(&registry.page_ledger, User::ALIGNMENT, &mut mm);
        let mut count = 0;
        while reader.try_next().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 3);
    }
}

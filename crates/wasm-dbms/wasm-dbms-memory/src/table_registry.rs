// Rust guideline compliant 2026-02-28

mod autoincrement_ledger;
mod free_segments_ledger;
mod index_ledger;
mod page_ledger;
mod raw_record;
mod record_address;
mod schema_snapshot_ledger;
mod table_reader;
mod write_at;

use wasm_dbms_api::prelude::{Encode, MemoryResult, PageOffset, Value};

pub use self::autoincrement_ledger::AutoincrementLedger;
use self::free_segments_ledger::FreeSegmentsLedger;
pub use self::index_ledger::{IndexLedger, IndexTreeWalker};
use self::page_ledger::PageLedger;
use self::raw_record::RawRecord;
pub use self::record_address::RecordAddress;
pub use self::schema_snapshot_ledger::SchemaSnapshotLedger;
pub use self::table_reader::{NextRecord, TableReader};
use self::write_at::WriteAt;
use crate::{MemoryAccess, TableRegistryPage, align_up};

/// The table registry takes care of storing the records for each table,
/// using the [`FreeSegmentsLedger`] and [`PageLedger`] to derive exactly where to read/write.
///
/// A registry is generic over a record, which must implement [`Encode`].
///
/// The CRUD operations provided by the table registry do NOT perform any logical checks,
/// but just allow to read/write records from/to memory.
/// So CRUD checks must be performed by a higher layer, prior to calling these methods.
pub struct TableRegistry {
    schema_snapshot_ledger: SchemaSnapshotLedger,
    pub(crate) page_ledger: PageLedger,
    free_segments_ledger: FreeSegmentsLedger,
    index_ledger: IndexLedger,
    auto_increment_ledger: Option<AutoincrementLedger>,
}

impl TableRegistry {
    /// Loads the table registry from memory.
    pub fn load(table_pages: TableRegistryPage, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        Ok(Self {
            schema_snapshot_ledger: SchemaSnapshotLedger::load(
                table_pages.schema_snapshot_page,
                mm,
            )?,
            page_ledger: PageLedger::load(table_pages.pages_list_page, mm)?,
            free_segments_ledger: FreeSegmentsLedger::load(table_pages.free_segments_page, mm)?,
            index_ledger: IndexLedger::load(table_pages.index_registry_page, mm)?,
            auto_increment_ledger: if let Some(page) = table_pages.autoincrement_registry_page {
                Some(AutoincrementLedger::load(page, mm)?)
            } else {
                None
            },
        })
    }

    /// Inserts a new record into the table registry.
    ///
    /// Returns the address where the record was inserted, which can be used to read it back or to update/delete it.
    ///
    /// NOTE: this function does NOT make any logical checks on the record being inserted.
    pub fn insert<E>(
        &mut self,
        record: E,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<RecordAddress>
    where
        E: Encode,
    {
        // get position to write the record
        let raw_record = RawRecord::new(record);
        let write_at = self.get_write_position(&raw_record, mm)?;

        // align insert to RawRecord<E> alignment (includes the 2-byte header)
        let aligned_offset = align_up::<RawRecord<E>>(write_at.offset() as usize) as PageOffset;

        // write record
        mm.write_at(write_at.page(), aligned_offset, &raw_record)?;

        let pointer = RecordAddress {
            page: write_at.page(),
            offset: aligned_offset,
        };

        // commit post-write actions
        self.post_write(write_at, &raw_record, mm)?;

        Ok(pointer)
    }

    /// Creates a [`TableReader`] to read records from the table registry.
    ///
    /// Use [`TableReader::try_next`] to read records one by one.
    pub fn read<'a, E, MA>(&'a self, mm: &'a mut MA) -> TableReader<'a, E, MA>
    where
        E: Encode,
        MA: MemoryAccess,
    {
        TableReader::new(&self.page_ledger, mm)
    }

    /// Reads a single record at the given address.
    pub fn read_at<E, MA>(&self, address: RecordAddress, mm: &mut MA) -> MemoryResult<E>
    where
        E: Encode,
        MA: MemoryAccess,
    {
        let raw_record: RawRecord<E> = mm.read_at(address.page, address.offset)?;
        Ok(raw_record.data)
    }

    /// Deletes a record at the given page and offset.
    ///
    /// The space occupied by the record is marked as free and zeroed.
    pub fn delete(
        &mut self,
        record: impl Encode,
        address: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        let raw_record = RawRecord::new(record);

        // zero the record in memory
        mm.zero(address.page, address.offset, &raw_record)?;

        // insert a free segment for the deleted record
        self.free_segments_ledger
            .insert_free_segment(address.page, address.offset, &raw_record, mm)
    }

    /// Updates a record at the given page and offset.
    ///
    /// The [`RecordAddress`] of the new record is returned, which can be different from the old one if the record was reallocated.
    ///
    /// The logic is the following:
    ///
    /// 1. If the new record has exactly the same size of the old record, overwrite it in place.
    /// 2. If the new record does not fit, delete the old record and insert the new record.
    pub fn update(
        &mut self,
        new_record: impl Encode,
        old_record: impl Encode,
        old_address: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<RecordAddress> {
        if new_record.size() == old_record.size() {
            self.update_in_place(new_record, old_address, mm)
        } else {
            self.update_by_realloc(new_record, old_record, old_address, mm)
        }
    }

    /// Get a reference to the index ledger, allowing to read the indexes.
    pub fn index_ledger(&self) -> &IndexLedger {
        &self.index_ledger
    }

    /// Get a mutable reference to the index ledger, allowing to modify the indexes.
    pub fn index_ledger_mut(&mut self) -> &mut IndexLedger {
        &mut self.index_ledger
    }

    /// Get a reference to the [`SchemaSnapshotLedger`], allowing to read the schema snapshot.
    pub fn schema_snapshot_ledger(&self) -> &SchemaSnapshotLedger {
        &self.schema_snapshot_ledger
    }

    /// Get a mutable reference to the [`SchemaSnapshotLedger`], allowing to modify the schema snapshot.
    pub fn schema_snapshot_ledger_mut(&mut self) -> &mut SchemaSnapshotLedger {
        &mut self.schema_snapshot_ledger
    }

    /// Get next value for an autoincrement column of the given type, and increment it in the ledger.
    pub fn next_autoincrement(
        &mut self,
        column_name: &str,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Option<Value>> {
        if let Some(ledger) = &mut self.auto_increment_ledger {
            ledger.next(column_name, mm).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Update a [`RawRecord`] in place at the given page and offset.
    ///
    /// The [`RecordAddress`] of the record is returned, which is the same as the old one.
    ///
    /// This must be used IF AND ONLY if the new record has the SAME size as the old record.
    fn update_in_place(
        &mut self,
        record: impl Encode,
        address: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<RecordAddress> {
        let raw_record = RawRecord::new(record);
        mm.write_at(address.page, address.offset, &raw_record)?;

        Ok(address)
    }

    /// Updates a record by reallocating it.
    ///
    /// The old record is deleted and the new record is inserted.
    ///
    /// The [`RecordAddress`] of the new record is returned, which can be different from the old one.
    fn update_by_realloc(
        &mut self,
        new_record: impl Encode,
        old_record: impl Encode,
        old_address: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<RecordAddress> {
        // delete old record
        self.delete(old_record, old_address, mm)?;

        // insert new record
        self.insert(new_record, mm)
    }

    /// Gets the position where to write a record of the given size.
    fn get_write_position<E>(
        &mut self,
        record: &RawRecord<E>,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<WriteAt>
    where
        E: Encode,
    {
        // check if there is a free segment that can hold the record
        if let Some(segment) = self
            .free_segments_ledger
            .find_reusable_segment(record, mm)?
        {
            return Ok(WriteAt::ReusedSegment(segment));
        }

        // otherwise, write at the end of the table
        self.page_ledger
            .get_page_and_offset_for_record(record, mm)
            .map(|(page, offset)| WriteAt::End(page, offset))
    }

    /// Commits the post-write actions after writing a record at the given position.
    ///
    /// - If the record was a [`WriteAt::ReusedSegment`], the free segment is marked as used.
    /// - If the record was a [`WriteAt::End`], the page ledger is updated.
    fn post_write<E>(
        &mut self,
        write_at: WriteAt,
        record: &RawRecord<E>,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        E: Encode,
    {
        match write_at {
            WriteAt::ReusedSegment(free_segment) => {
                // mark segment as used
                self.free_segments_ledger
                    .commit_reused_space(record, free_segment, mm)
            }
            WriteAt::End(page, ..) => {
                // update page ledger
                self.page_ledger.commit(page, record, mm)
            }
        }
    }
}

/// Test utilities shared across the table_registry submodules.
#[cfg(test)]
pub(crate) mod test_utils {
    use wasm_dbms_api::prelude::{
        DEFAULT_ALIGNMENT, DataSize, DecodeError, Encode, MSize, MemoryError, MemoryResult,
        PageOffset,
    };

    /// A simple user struct for testing purposes (no macro dependencies).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct User {
        pub id: u32,
        pub name: String,
        pub email: String,
        pub age: u32,
    }

    impl Encode for User {
        const SIZE: DataSize = DataSize::Dynamic;

        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            let mut buf = Vec::new();
            // id: 4 bytes
            buf.extend_from_slice(&self.id.to_le_bytes());
            // name length: 2 bytes + name bytes
            buf.extend_from_slice(&(self.name.len() as u16).to_le_bytes());
            buf.extend_from_slice(self.name.as_bytes());
            // email length: 2 bytes + email bytes
            buf.extend_from_slice(&(self.email.len() as u16).to_le_bytes());
            buf.extend_from_slice(self.email.as_bytes());
            // age: 4 bytes
            buf.extend_from_slice(&self.age.to_le_bytes());
            std::borrow::Cow::Owned(buf)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            if data.len() < 12 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let mut offset = 0;
            // id
            let id = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            offset += 4;
            // name
            let name_len =
                u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as usize;
            offset += 2;
            let name = String::from_utf8_lossy(&data[offset..offset + name_len]).to_string();
            offset += name_len;
            // email
            let email_len =
                u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap()) as usize;
            offset += 2;
            let email = String::from_utf8_lossy(&data[offset..offset + email_len]).to_string();
            offset += email_len;
            // age
            let age = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());

            Ok(User {
                id,
                name,
                email,
                age,
            })
        }

        fn size(&self) -> MSize {
            (4 + 2 + self.name.len() + 2 + self.email.len() + 4) as MSize
        }
    }
}

#[cfg(test)]
mod tests {

    use self::test_utils::User;
    use super::free_segments_ledger::FreeSegment;
    use super::table_reader::NextRecord;
    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    #[test]
    fn test_should_create_table_registry() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let schema_snapshot_page = mm.allocate_page().expect("failed to get page");
        let page_ledger_page = mm.allocate_page().expect("failed to get page");
        let free_segments_page = mm.allocate_page().expect("failed to get page");
        let index_registry_page = mm.allocate_page().expect("failed to get page");
        let autoincrement_page = mm.allocate_page().expect("failed to get page");
        let table_pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page: page_ledger_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page: Some(autoincrement_page),
        };

        let registry: MemoryResult<TableRegistry> = TableRegistry::load(table_pages, &mut mm);
        assert!(registry.is_ok());
    }

    #[test]
    fn test_should_get_write_at_end() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let record = RawRecord::new(User {
            id: 1,
            name: "Test".to_string(),
            email: "new_user@example.com".to_string(),
            age: 25,
        });
        let write_at = registry
            .get_write_position(&record, &mut mm)
            .expect("failed to get write at");

        assert!(matches!(write_at, WriteAt::End(_, 0)));
    }

    #[test]
    fn test_should_get_write_at_free_segment() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let record = RawRecord::new(User {
            id: 1,
            name: "Test".to_string(),
            email: "new_user@example.com".to_string(),
            age: 25,
        });
        // allocate a page to insert a free segment
        let (page, _) = registry
            .page_ledger
            .get_page_and_offset_for_record(&record, &mut mm)
            .expect("failed to get page and offset");
        registry
            .page_ledger
            .commit(page, &record, &mut mm)
            .expect("failed to commit page ledger");
        // insert data about a free segment
        registry
            .free_segments_ledger
            .insert_free_segment(page, 256, &record, &mut mm)
            .expect("failed to insert free segment");

        let write_at = registry
            .get_write_position(&record, &mut mm)
            .expect("failed to get write at");

        let reused_segment = match write_at {
            WriteAt::ReusedSegment(segment) => segment.segment,
            _ => panic!("expected reused segment"),
        };

        assert_eq!(
            reused_segment,
            FreeSegment {
                page,
                offset: 256,
                size: 64, // padded size
            }
        );
    }

    #[test]
    fn test_should_insert_record_into_table_registry() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let record = User {
            id: 1,
            name: "Test".to_string(),
            email: "new_user@example.com".to_string(),
            age: 25,
        };

        // insert record
        assert!(registry.insert(record, &mut mm).is_ok());
    }

    #[test]
    fn test_should_manage_to_insert_users_to_exceed_one_page() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        for id in 0..4000 {
            let record = User {
                id,
                name: format!("User {}", id),
                email: "new_user@example.com".to_string(),
                age: 20 + id,
            };
            registry
                .insert(record, &mut mm)
                .expect("failed to insert record");
        }
    }

    #[test]
    fn test_should_delete_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let record = User {
            id: 1,
            name: "Test".to_string(),
            email: "new_user@example.com".to_string(),
            age: 25,
        };

        // insert record
        registry
            .insert(record.clone(), &mut mm)
            .expect("failed to insert");

        // find where it was written
        let mut reader = registry.read(&mut mm);
        let next_record: NextRecord<User> = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        let page = next_record.page;
        let offset = next_record.offset;
        let record = next_record.record;
        let raw_user = RawRecord::new(record.clone());
        let raw_user_size = raw_user.size();

        // delete record
        assert!(
            registry
                .delete(record, RecordAddress { page, offset }, &mut mm)
                .is_ok()
        );

        // should have been deleted
        let mut reader = registry.read::<User, _>(&mut mm);
        assert!(reader.try_next().expect("failed to read").is_none());

        // should have a free segment
        let free_segment = registry
            .free_segments_ledger
            .find_reusable_segment(
                &User {
                    id: 2,
                    name: "Test".to_string(),
                    email: "new_user@example.com".to_string(),
                    age: 25,
                },
                &mut mm,
            )
            .expect("failed to find free segment")
            .expect("could not find the free segment after free")
            .segment;
        assert_eq!(free_segment.page, page);
        assert_eq!(free_segment.offset, offset);
        assert_eq!(free_segment.size, 64); // padded

        // should have zeroed the memory
        let mut buffer = vec![0u8; raw_user_size as usize];
        mm.read_at_raw(page, offset, &mut buffer)
            .expect("failed to read memory");
        assert!(buffer.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_read_at_returns_record_at_address() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);
        let record = User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        };

        let address = registry
            .insert(record.clone(), &mut mm)
            .expect("failed to insert record");

        let stored: User = registry
            .read_at(address, &mut mm)
            .expect("failed to read record");
        assert_eq!(stored, record);
    }

    #[test]
    fn test_read_at_after_update_returns_updated_record() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);
        let old_record = User {
            id: 1,
            name: "Alice".to_string(),
            email: "alice@example.com".to_string(),
            age: 30,
        };
        let new_record = User {
            id: 1,
            name: "Alice Updated".to_string(),
            email: "alice.updated@example.com".to_string(),
            age: 31,
        };

        let old_address = registry
            .insert(old_record.clone(), &mut mm)
            .expect("failed to insert record");
        let new_address = registry
            .update(new_record.clone(), old_record, old_address, &mut mm)
            .expect("failed to update record");

        let stored: User = registry
            .read_at(new_address, &mut mm)
            .expect("failed to read updated record");
        assert_eq!(stored, new_record);
    }

    #[test]
    fn test_should_update_record_in_place() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let old_record = User {
            id: 1,
            name: "John".to_string(),
            email: "new_user@example.com".to_string(),
            age: 28,
        };
        let new_record = User {
            id: 1,
            name: "Mark".to_string(), // same length as "John"
            email: "new_user@example.com".to_string(),
            age: 30,
        };

        // insert old record
        registry
            .insert(old_record.clone(), &mut mm)
            .expect("failed to insert");

        // find where it was written
        let mut reader = registry.read::<User, _>(&mut mm);
        let next_record = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        let page = next_record.page;
        let offset = next_record.offset;

        // update in place
        let old_address = RecordAddress { page, offset };
        let new_location = registry
            .update(
                new_record.clone(),
                next_record.record.clone(),
                old_address,
                &mut mm,
            )
            .expect("failed to update record");
        assert_eq!(new_location, old_address); // should be same address

        // read back the record
        let mut reader = registry.read::<User, _>(&mut mm);
        let next_record = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        assert_eq!(next_record.page, page); // should be same page
        assert_eq!(next_record.offset, offset); // should be same offset
        assert_eq!(next_record.record, new_record);
    }

    #[test]
    fn test_should_update_record_reallocating() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        let old_record = User {
            id: 1,
            name: "John".to_string(),
            email: "new_user@example.com".to_string(),
            age: 28,
        };
        // this user creates a record with same size as old_record to avoid reusing the free segment
        let extra_user = User {
            id: 2,
            name: "Extra".to_string(),
            email: "new_user@example.com".to_string(),
            age: 25,
        };
        let new_record = User {
            id: 1,
            name: "Alexanderejruwgjowergjioewrgjioewrigjewriogjweoirgjiowerjgoiwerjiogewirogjowejrgiwer".to_string(), // must exceed padding
            email: "new_user@example.com".to_string(),
            age: 30,
        };

        // insert old record
        registry
            .insert(old_record.clone(), &mut mm)
            .expect("failed to insert");
        // insert extra record to avoid reusing the free segment
        registry
            .insert(extra_user.clone(), &mut mm)
            .expect("failed to insert extra user");

        // find where it was written
        let mut reader = registry.read::<User, _>(&mut mm);
        let old_record_from_db = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        assert_eq!(old_record_from_db.record, old_record);
        let page = old_record_from_db.page;
        let offset = old_record_from_db.offset;

        // update by reallocating
        let old_address = RecordAddress { page, offset };
        let new_location = registry
            .update(
                new_record.clone(),
                old_record_from_db.record.clone(),
                old_address,
                &mut mm,
            )
            .expect("failed to update record");
        assert_ne!(new_location, old_address); // should be different page

        // read back the record
        let mut reader = registry.read::<User, _>(&mut mm);

        // find extra record first
        let _ = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");

        let updated_record = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        assert_ne!(updated_record.offset, offset); // should be different offset
        assert_eq!(updated_record.record, new_record);
    }

    #[test]
    fn test_should_insert_delete_insert_many() {
        const COUNT: u32 = 1_000;
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);
        for id in 0..COUNT {
            let record = User {
                id,
                name: format!("User {id}"),
                email: format!("user_{id}@example.com"),
                age: 20,
            };

            // insert record
            registry
                .insert(record.clone(), &mut mm)
                .expect("failed to insert");
        }

        // delete odd records
        for id in (0..COUNT).filter(|id| id % 2 == 1) {
            let record = User {
                id,
                name: format!("User {id}"),
                email: format!("user_{id}@example.com"),
                age: 20,
            };
            // find where it was written
            let mut reader = registry.read::<User, _>(&mut mm);
            let mut deleted = false;
            while let Some(next_record) = reader.try_next().expect("failed to read") {
                if next_record.record.id == id {
                    registry
                        .delete(
                            record.clone(),
                            RecordAddress {
                                page: next_record.page,
                                offset: next_record.offset,
                            },
                            &mut mm,
                        )
                        .expect("failed to delete");
                    deleted = true;
                    break;
                }
            }
            assert!(deleted, "record with id {} was not found", id);
        }

        // now delete also the others
        for id in (0..COUNT).filter(|id| id % 2 == 0) {
            let record = User {
                id,
                name: format!("User {id}"),
                email: format!("user_{id}@example.com"),
                age: 20,
            };
            // find where it was written
            let mut reader = registry.read::<User, _>(&mut mm);
            let mut deleted = false;
            while let Some(next_record) = reader.try_next().expect("failed to read") {
                if next_record.record.id == id {
                    registry
                        .delete(
                            record.clone(),
                            RecordAddress {
                                page: next_record.page,
                                offset: next_record.offset,
                            },
                            &mut mm,
                        )
                        .expect("failed to delete");
                    deleted = true;
                    break;
                }
            }
            assert!(deleted, "record with id {} was not found", id);
        }

        // insert back
        for id in 0..COUNT {
            let record = User {
                id,
                name: format!("User {id}"),
                email: format!("user_{id}@example.com"),
                age: 20,
            };

            // insert record
            registry
                .insert(record.clone(), &mut mm)
                .expect("failed to insert");
        }
    }

    #[test]
    fn test_should_reduce_free_segment_size_with_padding() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        // first insert a user
        let long_name = vec!['A'; 1024].into_iter().collect::<String>();
        let record = User {
            id: 1,
            name: "Test User".to_string(),
            email: long_name,
            age: 30,
        };
        registry
            .insert(record.clone(), &mut mm)
            .expect("failed to insert");
        // get record page
        let mut reader = registry.read::<User, _>(&mut mm);
        let next_record = reader
            .try_next()
            .expect("failed to read")
            .expect("no record");
        // delete user
        registry
            .delete(
                next_record.record,
                RecordAddress {
                    page: next_record.page,
                    offset: next_record.offset,
                },
                &mut mm,
            )
            .expect("failed to delete");

        // get the free segment
        let raw_record = RawRecord::new(record.clone());
        let free_segment = registry
            .free_segments_ledger
            .find_reusable_segment(&raw_record, &mut mm)
            .expect("failed to find reusable segment")
            .expect("could not find the free segment after free")
            .segment;
        // size should be at least 1024
        assert!(free_segment.size >= 1024);
        let previous_size = free_segment.size;

        // now insert a small user at 0
        let small_record = User {
            id: 2,
            name: "Bob The Builder".to_string(),
            email: "bob@hotmail.com".to_string(),
            age: 22,
        };
        registry
            .insert(small_record.clone(), &mut mm)
            .expect("failed to insert small user");

        // get free segment
        let free_segment_after = registry
            .free_segments_ledger
            .find_reusable_segment(&small_record, &mut mm)
            .expect("failed to find reusable segment")
            .expect("could not find the free segment after inserting small user")
            .segment;

        // size should be reduced
        assert_eq!(
            free_segment_after.offset, 64,
            "expected offset to be 64, but had: {}",
            free_segment_after.offset
        ); // which is the padding
        assert_eq!(
            free_segment_after.size,
            previous_size - 64,
            "Expected free segment to have size: {} but got: {}",
            previous_size - 64,
            free_segment_after.size
        );
    }

    fn registry(mm: &mut MemoryManager<HeapMemoryProvider>) -> TableRegistry {
        let schema_snapshot_page = mm.allocate_page().expect("failed to get page");
        let page_ledger_page = mm.allocate_page().expect("failed to get page");
        let free_segments_page = mm.allocate_page().expect("failed to get page");
        let index_registry_page = mm.allocate_page().expect("failed to get page");
        let autoincrement_page = mm.allocate_page().expect("failed to get page");
        let table_pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page: page_ledger_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page: Some(autoincrement_page),
        };

        TableRegistry::load(table_pages, mm).expect("failed to load")
    }

    /// Creates a [`TableRegistry`] with a properly initialized autoincrement ledger
    /// via [`SchemaRegistry::register_table`].
    fn registry_with_autoincrement(mm: &mut MemoryManager<HeapMemoryProvider>) -> TableRegistry {
        use crate::SchemaRegistry;

        let mut schema = SchemaRegistry::load(mm).expect("failed to load schema");
        let pages = schema
            .register_table::<AutoincUser>(mm)
            .expect("failed to register table");
        TableRegistry::load(pages, mm).expect("failed to load")
    }

    /// Creates a [`TableRegistry`] without an autoincrement ledger.
    fn registry_without_autoincrement(mm: &mut MemoryManager<HeapMemoryProvider>) -> TableRegistry {
        let schema_snapshot_page = mm.allocate_page().expect("failed to get page");
        let page_ledger_page = mm.allocate_page().expect("failed to get page");
        let free_segments_page = mm.allocate_page().expect("failed to get page");
        let index_registry_page = mm.allocate_page().expect("failed to get page");
        let table_pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page: page_ledger_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page: None,
        };

        TableRegistry::load(table_pages, mm).expect("failed to load")
    }

    // -- AutoincUser mock: a table with an autoincrement Uint32 column --

    use candid::CandidType;
    use serde::{Deserialize, Serialize};
    use wasm_dbms_api::prelude::{
        ColumnDef, DbmsResult, IndexDef, InsertRecord, NoForeignFetcher, TableColumns, TableRecord,
        TableSchema, UpdateRecord,
    };

    #[derive(Clone, CandidType)]
    struct AutoincUser;

    impl Encode for AutoincUser {
        const SIZE: wasm_dbms_api::prelude::DataSize = wasm_dbms_api::prelude::DataSize::Dynamic;
        const ALIGNMENT: PageOffset = wasm_dbms_api::prelude::DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![])
        }

        fn decode(_data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            Ok(Self)
        }

        fn size(&self) -> wasm_dbms_api::prelude::MSize {
            0
        }
    }

    #[derive(Clone, CandidType, Deserialize)]
    struct AutoincUserRecord;

    impl TableRecord for AutoincUserRecord {
        type Schema = AutoincUser;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AutoincUserInsert;

    impl InsertRecord for AutoincUserInsert {
        type Record = AutoincUserRecord;
        type Schema = AutoincUser;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            AutoincUser
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AutoincUserUpdate;

    impl UpdateRecord for AutoincUserUpdate {
        type Record = AutoincUserRecord;
        type Schema = AutoincUser;

        fn from_values(
            _values: &[(ColumnDef, Value)],
            _where_clause: Option<wasm_dbms_api::prelude::Filter>,
        ) -> Self {
            Self
        }

        fn update_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<wasm_dbms_api::prelude::Filter> {
            None
        }
    }

    impl TableSchema for AutoincUser {
        type Record = AutoincUserRecord;
        type Insert = AutoincUserInsert;
        type Update = AutoincUserUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "autoinc_users"
        }

        fn columns() -> &'static [ColumnDef] {
            use wasm_dbms_api::prelude::DataTypeKind;

            &[ColumnDef {
                name: "id",
                data_type: DataTypeKind::Uint32,
                auto_increment: true,
                nullable: false,
                primary_key: true,
                unique: true,
                foreign_key: None,
                default: None,
                renamed_from: &[],
            }]
        }

        fn primary_key() -> &'static str {
            "id"
        }

        fn indexes() -> &'static [IndexDef] {
            &[IndexDef(&["id"])]
        }

        fn to_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn sanitizer(
            _column_name: &'static str,
        ) -> Option<Box<dyn wasm_dbms_api::prelude::Sanitize>> {
            None
        }

        fn validator(
            _column_name: &'static str,
        ) -> Option<Box<dyn wasm_dbms_api::prelude::Validate>> {
            None
        }
    }

    // -- next_autoincrement tests --

    #[test]
    fn test_next_autoincrement_returns_sequential_values() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry_with_autoincrement(&mut mm);

        let v1 = registry
            .next_autoincrement("id", &mut mm)
            .expect("failed")
            .expect("expected Some");
        let v2 = registry
            .next_autoincrement("id", &mut mm)
            .expect("failed")
            .expect("expected Some");
        let v3 = registry
            .next_autoincrement("id", &mut mm)
            .expect("failed")
            .expect("expected Some");

        assert_eq!(v1, Value::Uint32(1u32.into()));
        assert_eq!(v2, Value::Uint32(2u32.into()));
        assert_eq!(v3, Value::Uint32(3u32.into()));
    }

    #[test]
    fn test_next_autoincrement_returns_none_without_ledger() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry_without_autoincrement(&mut mm);

        let result = registry.next_autoincrement("id", &mut mm).expect("failed");
        assert!(result.is_none());
    }

    #[test]
    fn test_next_autoincrement_persists_across_reload() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());

        use crate::SchemaRegistry;
        let mut schema = SchemaRegistry::load(&mut mm).expect("failed to load schema");
        let pages = schema
            .register_table::<AutoincUser>(&mut mm)
            .expect("failed to register table");

        // advance 5 times
        let mut registry = TableRegistry::load(pages, &mut mm).expect("failed to load");
        for _ in 0..5 {
            let _ = registry
                .next_autoincrement("id", &mut mm)
                .expect("next failed");
        }

        // reload the registry from the same pages
        let mut reloaded = TableRegistry::load(pages, &mut mm).expect("failed to reload");
        let value = reloaded
            .next_autoincrement("id", &mut mm)
            .expect("failed")
            .expect("expected Some");
        assert_eq!(value, Value::Uint32(6u32.into()));
    }

    #[test]
    fn test_next_autoincrement_overflow_returns_error() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());

        // manually set up a Uint8 autoincrement to hit overflow quickly
        let schema_snapshot_page = mm.allocate_page().expect("failed to get page");
        let page_ledger_page = mm.allocate_page().expect("failed to get page");
        let free_segments_page = mm.allocate_page().expect("failed to get page");
        let index_registry_page = mm.allocate_page().expect("failed to get page");
        let autoinc_page = mm.allocate_page().expect("failed to get page");

        // Use the Uint8AutoincTable from the autoincrement_ledger tests — we replicate the
        // TableSchema inline since it's in a sibling test module.
        // Instead, just init the ledger page directly with a Uint8 value.
        {
            let mut registry_data = super::autoincrement_ledger::AutoincrementLedger::init::<
                Uint8AutoincSchema,
            >(autoinc_page, &mut mm)
            .expect("failed to init autoinc ledger");

            // advance to 255
            for _ in 0..255 {
                let _ = registry_data.next("val", &mut mm).expect("next failed");
            }
        }

        // init index ledger
        IndexLedger::init(index_registry_page, &[], &mut mm).expect("failed to init index ledger");

        let table_pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page: page_ledger_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page: Some(autoinc_page),
        };

        let mut registry = TableRegistry::load(table_pages, &mut mm).expect("failed to load");
        let result = registry.next_autoincrement("val", &mut mm);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            wasm_dbms_api::prelude::MemoryError::AutoincrementOverflow(_)
        ));
    }

    // Minimal Uint8 autoincrement schema for overflow test

    #[derive(Clone, CandidType)]
    struct Uint8AutoincSchema;

    impl Encode for Uint8AutoincSchema {
        const SIZE: wasm_dbms_api::prelude::DataSize = wasm_dbms_api::prelude::DataSize::Dynamic;
        const ALIGNMENT: PageOffset = wasm_dbms_api::prelude::DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![])
        }

        fn decode(_data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            Ok(Self)
        }

        fn size(&self) -> wasm_dbms_api::prelude::MSize {
            0
        }
    }

    #[derive(Clone, CandidType, Deserialize)]
    struct Uint8AutoincSchemaRecord;

    impl TableRecord for Uint8AutoincSchemaRecord {
        type Schema = Uint8AutoincSchema;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct Uint8AutoincSchemaInsert;

    impl InsertRecord for Uint8AutoincSchemaInsert {
        type Record = Uint8AutoincSchemaRecord;
        type Schema = Uint8AutoincSchema;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            Uint8AutoincSchema
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct Uint8AutoincSchemaUpdate;

    impl UpdateRecord for Uint8AutoincSchemaUpdate {
        type Record = Uint8AutoincSchemaRecord;
        type Schema = Uint8AutoincSchema;

        fn from_values(
            _values: &[(ColumnDef, Value)],
            _where_clause: Option<wasm_dbms_api::prelude::Filter>,
        ) -> Self {
            Self
        }

        fn update_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<wasm_dbms_api::prelude::Filter> {
            None
        }
    }

    impl TableSchema for Uint8AutoincSchema {
        type Record = Uint8AutoincSchemaRecord;
        type Insert = Uint8AutoincSchemaInsert;
        type Update = Uint8AutoincSchemaUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "uint8_autoinc_schema"
        }

        fn columns() -> &'static [ColumnDef] {
            use wasm_dbms_api::prelude::DataTypeKind;

            &[ColumnDef {
                name: "val",
                data_type: DataTypeKind::Uint8,
                auto_increment: true,
                nullable: false,
                primary_key: true,
                unique: true,
                foreign_key: None,
                default: None,
                renamed_from: &[],
            }]
        }

        fn primary_key() -> &'static str {
            "val"
        }

        fn indexes() -> &'static [IndexDef] {
            &[IndexDef(&["val"])]
        }

        fn to_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn sanitizer(
            _column_name: &'static str,
        ) -> Option<Box<dyn wasm_dbms_api::prelude::Sanitize>> {
            None
        }

        fn validator(
            _column_name: &'static str,
        ) -> Option<Box<dyn wasm_dbms_api::prelude::Validate>> {
            None
        }
    }

    use wasm_dbms_api::prelude::{DataSize, DecodeError, MSize, MemoryError};

    /// A fixed-size record for regression testing (issue #80).
    ///
    /// Layout: u64 (8) + u64 (8) + u8 (1) = 17 bytes, all fixed-size.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FixedSizeRecord {
        id: u64,
        timestamp: u64,
        tag: u8,
    }

    impl Encode for FixedSizeRecord {
        const SIZE: DataSize = DataSize::Fixed(17);
        const ALIGNMENT: PageOffset = 17;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            let mut buf = Vec::with_capacity(17);
            buf.extend_from_slice(&self.id.to_le_bytes());
            buf.extend_from_slice(&self.timestamp.to_le_bytes());
            buf.push(self.tag);
            std::borrow::Cow::Owned(buf)
        }

        fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            if data.len() < 17 {
                return Err(MemoryError::DecodeError(DecodeError::TooShort));
            }
            let id = u64::from_le_bytes(data[0..8].try_into().unwrap());
            let timestamp = u64::from_le_bytes(data[8..16].try_into().unwrap());
            let tag = data[16];
            Ok(Self { id, timestamp, tag })
        }

        fn size(&self) -> MSize {
            17
        }
    }

    #[test]
    fn test_should_insert_multiple_fixed_size_records() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut registry = registry(&mut mm);

        for i in 0..10 {
            let record = FixedSizeRecord {
                id: i,
                timestamp: 1000 + i,
                tag: (i % 2) as u8,
            };
            registry
                .insert(record, &mut mm)
                .unwrap_or_else(|e| panic!("failed to insert record {i}: {e}"));
        }

        // verify all records can be read back
        let mut reader = registry.read::<FixedSizeRecord, _>(&mut mm);
        let mut count = 0;
        while let Some(next) = reader.try_next().expect("failed to read") {
            assert_eq!(next.record.id, count);
            count += 1;
        }
        assert_eq!(count, 10);
    }
}

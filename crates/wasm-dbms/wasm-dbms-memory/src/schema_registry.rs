// Rust guideline compliant 2026-02-28

use std::collections::HashMap;

use wasm_dbms_api::memory::MemoryError;
use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryResult, Page, PageOffset, TableFingerprint,
    TableSchema, TableSchemaSnapshot, fingerprint_for_name,
};

use crate::table_registry::{AutoincrementLedger, IndexLedger, SchemaSnapshotLedger};
use crate::{MemoryAccess, MemoryManager, MemoryProvider};

/// The dictionary of tables, mapping the table schema fingerprint to the pages where the table data and metadata are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRegistryPage {
    /// The page where the Schema Snapshot for this table is stored.
    pub schema_snapshot_page: Page,
    /// The page where the list of pages for this table is stored.
    pub pages_list_page: Page,
    /// The page where the free segments for this table are stored.
    pub free_segments_page: Page,
    /// The page where the index registry for this table is stored.
    pub index_registry_page: Page,
    /// The page where the autoincrement registry for this table is stored.
    /// Only used if the table has an autoincrement column.
    pub autoincrement_registry_page: Option<Page>,
}

/// The schema registry takes care of storing and retrieving table schemas from memory.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SchemaRegistry {
    tables: HashMap<TableFingerprint, TableRegistryPage>,
}

impl SchemaRegistry {
    /// Load the schema registry from memory.
    pub fn load(mm: &mut MemoryManager<impl MemoryProvider>) -> MemoryResult<Self> {
        let page = mm.schema_page();
        let registry: Self = mm.read_at(page, 0)?;
        Ok(registry)
    }

    /// Registers a table and allocates it registry page.
    ///
    /// The [`TableSchema`] type parameter is used to get the [`TableSchema::fingerprint`] of the
    /// table schema. The fingerprint is derived from the table name, so two distinct names that
    /// hash to the same value are detected eagerly: when the slot is already occupied by a table
    /// with a different name, [`MemoryError::NameCollision`] is returned and no allocation is
    /// performed.
    ///
    /// # Errors
    ///
    /// - [`MemoryError::NameCollision`] when the fingerprint slot is occupied by a table whose
    ///   persisted snapshot carries a different name.
    /// - Any [`MemoryError`] propagated from page allocation, snapshot init, or the registry
    ///   write-back.
    pub fn register_table<TS>(
        &mut self,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<TableRegistryPage>
    where
        TS: TableSchema,
    {
        // check if already registered, and detect name-hash collisions eagerly
        let fingerprint = TS::fingerprint();
        let candidate_name = TS::table_name();
        if let Some(pages) = self.tables.get(&fingerprint).copied() {
            let existing = SchemaSnapshotLedger::load(pages.schema_snapshot_page, mm)?;
            if existing.get().name != candidate_name {
                return Err(MemoryError::NameCollision {
                    candidate: candidate_name.to_string(),
                    existing: existing.get().name.clone(),
                });
            }
            return Ok(pages);
        }

        // allocate table registry page
        let schema_snapshot_page = mm.allocate_page()?;
        let pages_list_page = mm.allocate_page()?;
        let free_segments_page = mm.allocate_page()?;
        let index_registry_page = mm.allocate_page()?;
        // allocate autoincrement registry page if needed
        let has_autoincrement = TS::columns().iter().any(|col| col.auto_increment);
        let autoincrement_registry_page = if has_autoincrement {
            Some(mm.allocate_page()?)
        } else {
            None
        };

        // insert into tables map
        let pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page,
        };
        self.tables.insert(fingerprint, pages);

        // get schema page
        let page = mm.schema_page();
        // write self to schema page
        mm.write_at(page, 0, self)?;

        // init snapshot ledger for this table
        SchemaSnapshotLedger::init::<TS>(pages.schema_snapshot_page, mm)?;
        // init index ledger for this table
        IndexLedger::init(pages.index_registry_page, TS::indexes(), mm)?;
        // init autoincrement ledger for this table if needed
        if let Some(autoinc_page) = pages.autoincrement_registry_page {
            AutoincrementLedger::init::<TS>(autoinc_page, mm)?;
        }

        Ok(pages)
    }

    /// Save the schema registry to memory.
    pub fn save(&self, mm: &mut MemoryManager<impl MemoryProvider>) -> MemoryResult<()> {
        let page = mm.schema_page();
        mm.write_at(page, 0, self)
    }

    /// Returns the table registry page for a given table schema.
    pub fn table_registry_page<TS>(&self) -> Option<TableRegistryPage>
    where
        TS: TableSchema,
    {
        self.tables.get(&TS::fingerprint()).copied()
    }

    /// Returns the table registry page for the table with the given name.
    ///
    /// Used by the migration engine, which knows tables only by name when
    /// applying ops decoded from snapshots.
    pub fn table_registry_page_by_name(&self, name: &str) -> Option<TableRegistryPage> {
        self.tables.get(&fingerprint_for_name(name)).copied()
    }

    /// Registers a table from a snapshot, allocating its registry pages.
    ///
    /// The migration engine uses this entry point when applying a
    /// `MigrationOp::CreateTable`: the source of truth is a
    /// [`TableSchemaSnapshot`], not a `TableSchema` type.
    ///
    /// # Errors
    ///
    /// - [`MemoryError::NameCollision`] when the fingerprint slot is occupied
    ///   by a table with a different name.
    /// - Any [`MemoryError`] propagated from page allocation, snapshot init,
    ///   index init, or registry persistence.
    pub fn register_table_from_snapshot(
        &mut self,
        snapshot: &TableSchemaSnapshot,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<TableRegistryPage> {
        let fingerprint = fingerprint_for_name(&snapshot.name);
        let candidate_name = snapshot.name.as_str();
        if let Some(pages) = self.tables.get(&fingerprint).copied() {
            let existing = SchemaSnapshotLedger::load(pages.schema_snapshot_page, mm)?;
            if existing.get().name != candidate_name {
                return Err(MemoryError::NameCollision {
                    candidate: candidate_name.to_string(),
                    existing: existing.get().name.clone(),
                });
            }
            return Ok(pages);
        }

        let schema_snapshot_page = mm.allocate_page()?;
        let pages_list_page = mm.allocate_page()?;
        let free_segments_page = mm.allocate_page()?;
        let index_registry_page = mm.allocate_page()?;
        let has_autoincrement = snapshot.columns.iter().any(|col| col.auto_increment);
        let autoincrement_registry_page = if has_autoincrement {
            Some(mm.allocate_page()?)
        } else {
            None
        };

        let pages = TableRegistryPage {
            schema_snapshot_page,
            pages_list_page,
            free_segments_page,
            index_registry_page,
            autoincrement_registry_page,
        };
        self.tables.insert(fingerprint, pages);

        let page = mm.schema_page();
        mm.write_at(page, 0, self)?;

        mm.write_at(pages.schema_snapshot_page, 0, snapshot)?;
        IndexLedger::init_from_keys(
            pages.index_registry_page,
            snapshot.indexes.iter().map(|idx| idx.columns.clone()),
            mm,
        )?;

        Ok(pages)
    }

    /// Removes the table identified by `name` from the registry and persists
    /// the change.
    ///
    /// Used by the migration engine when applying a `MigrationOp::DropTable`.
    /// The pages owned by the dropped table are leaked in v1 (issue #90 tracks
    /// page reclamation).
    ///
    /// Returns the [`TableRegistryPage`] previously associated with the table,
    /// or `None` if no such table was registered.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`] if persisting the updated registry fails.
    pub fn unregister_table(
        &mut self,
        name: &str,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<Option<TableRegistryPage>> {
        let fingerprint = fingerprint_for_name(name);
        let removed = self.tables.remove(&fingerprint);
        if removed.is_some() {
            let page = mm.schema_page();
            mm.write_at(page, 0, self)?;
        }
        Ok(removed)
    }

    /// Returns the persisted [`TableSchemaSnapshot`] for every registered table.
    ///
    /// The order is unspecified. Callers that need a stable order (e.g. for
    /// drift hashing) must sort by [`TableSchemaSnapshot::name`].
    ///
    /// # Errors
    ///
    /// Returns the first [`MemoryError`] encountered while loading any
    /// snapshot page.
    pub fn stored_snapshots(
        &self,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<Vec<TableSchemaSnapshot>> {
        self.tables
            .values()
            .map(|pages| {
                SchemaSnapshotLedger::load(pages.schema_snapshot_page, mm)
                    .map(|ledger| ledger.get().clone())
            })
            .collect()
    }
}

impl Encode for SchemaRegistry {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        // prepare buffer; size is 8 bytes for len + (8 + (4 * 2)) bytes for each entry
        let mut buffer = Vec::with_capacity(self.size() as usize);
        // write 8 bytes len of map
        buffer.extend_from_slice(&(self.tables.len() as u64).to_le_bytes());
        // write each entry
        for (fingerprint, page) in &self.tables {
            buffer.extend_from_slice(&fingerprint.to_le_bytes());
            buffer.extend_from_slice(&page.schema_snapshot_page.to_le_bytes());
            buffer.extend_from_slice(&page.pages_list_page.to_le_bytes());
            buffer.extend_from_slice(&page.free_segments_page.to_le_bytes());
            buffer.extend_from_slice(&page.index_registry_page.to_le_bytes());
            // autoincrement registry page is optional, so we write a flag and then the page if it exists
            if let Some(autoinc_page) = page.autoincrement_registry_page {
                buffer.push(1); // flag for presence of autoincrement registry page
                buffer.extend_from_slice(&autoinc_page.to_le_bytes());
            } else {
                buffer.push(0); // flag for absence of autoincrement registry page
            }
        }
        std::borrow::Cow::Owned(buffer)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let mut offset = 0;
        // read len
        let len = u64::from_le_bytes(
            data[offset..offset + 8]
                .try_into()
                .expect("failed to read length"),
        ) as usize;
        offset += 8;
        let mut tables = HashMap::with_capacity(len);
        // read each entry
        for _ in 0..len {
            let fingerprint = u64::from_le_bytes(data[offset..offset + 8].try_into()?);
            offset += 8;
            let schema_snapshot_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let pages_list_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let free_segments_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let index_registry_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let has_autoincrement = data[offset] == 1;
            offset += 1;
            let autoincrement_registry_page = if has_autoincrement {
                let page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
                offset += 4;
                Some(page)
            } else {
                None
            };
            tables.insert(
                fingerprint,
                TableRegistryPage {
                    schema_snapshot_page,
                    pages_list_page,
                    free_segments_page,
                    index_registry_page,
                    autoincrement_registry_page,
                },
            );
        }
        Ok(Self { tables })
    }

    fn size(&self) -> MSize {
        // - 8 bytes for `self.tables.len()`
        // - for each entry:
        //  - 8 bytes for the fingerprint
        //  - 4 bytes for the schema_snapshot_page
        //  - 4 bytes for the pages_list_page
        //  - 4 bytes for the free_segments_page
        //  - 4 bytes for the index_registry_page
        //  - 1 byte for the autoincrement registry page flag
        //  - 4 bytes for the autoincrement registry page if it exists
        let autoinc_pages = self
            .tables
            .values()
            .filter(|page| page.autoincrement_registry_page.is_some())
            .count() as MSize;

        8 + (self.tables.len() as MSize * (4 * 4 + 8 + 1)) + (autoinc_pages * 4)
    }
}

#[cfg(test)]
mod tests {

    use candid::CandidType;
    use serde::{Deserialize, Serialize};
    use wasm_dbms_api::prelude::{
        ColumnDef, DbmsResult, IndexDef, InsertRecord, Int32, NoForeignFetcher, TableColumns,
        TableRecord, UpdateRecord,
    };

    use super::*;
    use crate::{HeapMemoryProvider, RecordAddress};

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    #[test]
    fn test_should_encode_and_decode_schema_registry() {
        let mut mm = make_mm();

        // load
        let mut registry =
            SchemaRegistry::load(&mut mm).expect("failed to load init schema registry");

        // register table
        let registry_page = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register table");

        // get table registry page
        let fetched_page = registry
            .table_registry_page::<User>()
            .expect("failed to get table registry page");
        assert_eq!(registry_page, fetched_page);

        // encode
        let encoded = registry.encode();
        // decode
        let decoded = SchemaRegistry::decode(encoded).expect("failed to decode");
        assert_eq!(registry, decoded);

        // try to actually add another
        let another_registry_page = registry
            .register_table::<AnotherTable>(&mut mm)
            .expect("failed to register another table");
        let another_fetched_page = registry
            .table_registry_page::<AnotherTable>()
            .expect("failed to get another table registry page");
        assert_eq!(another_registry_page, another_fetched_page);

        // re-init
        let reloaded = SchemaRegistry::load(&mut mm).expect("failed to reload schema registry");
        assert_eq!(registry, reloaded);
        // should have two
        assert_eq!(reloaded.tables.len(), 2);
        assert_eq!(
            reloaded
                .table_registry_page::<User>()
                .expect("failed to get first table registry page after reload"),
            registry_page
        );
        assert_eq!(
            reloaded
                .table_registry_page::<AnotherTable>()
                .expect("failed to get second table registry page after reload"),
            another_registry_page
        );
    }

    #[test]
    fn test_register_table_writes_snapshot_to_ledger() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register table");

        let ledger = SchemaSnapshotLedger::load(pages.schema_snapshot_page, &mut mm)
            .expect("failed to load snapshot ledger after register_table");

        assert_eq!(ledger.get(), &User::schema_snapshot());
        assert_eq!(ledger.get().name, "users");
    }

    #[test]
    fn test_register_table_returns_name_collision_when_hash_slot_belongs_to_another_name() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        // register `User` so its snapshot lives on disk
        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register user");

        // simulate a hash collision by rewriting the persisted snapshot to carry a different name
        let mut tampered = User::schema_snapshot();
        tampered.name = "imposter".to_string();
        mm.write_at(pages.schema_snapshot_page, 0, &tampered)
            .expect("failed to overwrite snapshot");

        let result = registry.register_table::<User>(&mut mm);
        match result {
            Err(MemoryError::NameCollision {
                candidate,
                existing,
            }) => {
                assert_eq!(candidate, "users");
                assert_eq!(existing, "imposter");
            }
            other => panic!("expected NameCollision, got {other:?}"),
        }
    }

    #[test]
    fn test_should_not_register_same_table_twice() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        let first_page = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register table first time");
        let second_page = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register table second time");

        assert_eq!(first_page, second_page);
        assert_eq!(registry.tables.len(), 1);
    }

    #[test]
    fn test_should_init_index_ledger() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register table");

        // check that index ledger is initialized with the correct indexes
        let mut index_ledger = IndexLedger::load(pages.index_registry_page, &mut mm)
            .expect("failed to load index ledger");

        // insert an index for id
        index_ledger
            .insert(
                &["id"],
                Int32::from(1i32),
                RecordAddress { page: 1, offset: 0 },
                &mut mm,
            )
            .expect("failed to insert index");
        // search the index
        let result = index_ledger
            .search(&["id"], &Int32::from(1i32), &mut mm)
            .expect("failed to search index")
            .get(0)
            .copied()
            .expect("no index at 0");
        assert_eq!(result, RecordAddress { page: 1, offset: 0 });
    }

    #[derive(Clone, CandidType)]
    struct AnotherTable;

    impl Encode for AnotherTable {
        const SIZE: DataSize = DataSize::Dynamic;

        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![])
        }

        fn decode(_data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            Ok(AnotherTable)
        }

        fn size(&self) -> MSize {
            0
        }
    }

    #[derive(Clone, CandidType, Deserialize)]
    struct AnotherTableRecord;

    impl TableRecord for AnotherTableRecord {
        type Schema = AnotherTable;

        fn from_values(_values: TableColumns) -> Self {
            AnotherTableRecord
        }

        fn to_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AnotherTableInsert;

    impl InsertRecord for AnotherTableInsert {
        type Record = AnotherTableRecord;
        type Schema = AnotherTable;

        fn from_values(_values: &[(ColumnDef, wasm_dbms_api::prelude::Value)]) -> DbmsResult<Self> {
            Ok(AnotherTableInsert)
        }

        fn into_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            AnotherTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AnotherTableUpdate;

    impl UpdateRecord for AnotherTableUpdate {
        type Record = AnotherTableRecord;
        type Schema = AnotherTable;

        fn from_values(
            _values: &[(ColumnDef, wasm_dbms_api::prelude::Value)],
            _where_clause: Option<wasm_dbms_api::prelude::Filter>,
        ) -> Self {
            AnotherTableUpdate
        }

        fn update_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<wasm_dbms_api::prelude::Filter> {
            None
        }
    }

    impl TableSchema for AnotherTable {
        type Record = AnotherTableRecord;
        type Insert = AnotherTableInsert;
        type Update = AnotherTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "another_table"
        }

        fn columns() -> &'static [wasm_dbms_api::prelude::ColumnDef] {
            &[]
        }

        fn primary_key() -> &'static str {
            ""
        }

        fn indexes() -> &'static [wasm_dbms_api::prelude::IndexDef] {
            &[]
        }

        fn to_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
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

    // -- User mock for tests --

    #[derive(Clone, CandidType)]
    struct User;

    impl Encode for User {
        const SIZE: DataSize = DataSize::Dynamic;
        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![])
        }

        fn decode(_data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            Ok(User)
        }

        fn size(&self) -> MSize {
            0
        }
    }

    #[derive(Clone, CandidType, Deserialize)]
    struct UserRecord;

    impl TableRecord for UserRecord {
        type Schema = User;

        fn from_values(_values: TableColumns) -> Self {
            UserRecord
        }

        fn to_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct UserInsert;

    impl InsertRecord for UserInsert {
        type Record = UserRecord;
        type Schema = User;

        fn from_values(_values: &[(ColumnDef, wasm_dbms_api::prelude::Value)]) -> DbmsResult<Self> {
            Ok(UserInsert)
        }

        fn into_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            User
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct UserUpdate;

    impl UpdateRecord for UserUpdate {
        type Record = UserRecord;
        type Schema = User;

        fn from_values(
            _values: &[(ColumnDef, wasm_dbms_api::prelude::Value)],
            _where_clause: Option<wasm_dbms_api::prelude::Filter>,
        ) -> Self {
            UserUpdate
        }

        fn update_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<wasm_dbms_api::prelude::Filter> {
            None
        }
    }

    impl TableSchema for User {
        type Record = UserRecord;
        type Insert = UserInsert;
        type Update = UserUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "users"
        }

        fn columns() -> &'static [wasm_dbms_api::prelude::ColumnDef] {
            &[]
        }

        fn primary_key() -> &'static str {
            "id"
        }

        fn indexes() -> &'static [wasm_dbms_api::prelude::IndexDef] {
            &[IndexDef(&["id"])]
        }

        fn to_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
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

    #[test]
    fn test_table_registry_page_returns_none_for_unregistered_table() {
        let registry = SchemaRegistry::default();
        assert!(registry.table_registry_page::<User>().is_none());
    }

    #[test]
    fn test_empty_registry_encode_decode() {
        let registry = SchemaRegistry::default();
        let encoded = registry.encode();
        let decoded = SchemaRegistry::decode(encoded).expect("failed to decode empty registry");
        assert_eq!(registry, decoded);
        assert_eq!(decoded.tables.len(), 0);
    }

    #[test]
    fn test_load_fresh_memory_returns_empty_registry() {
        let mut mm = make_mm();
        let registry = SchemaRegistry::load(&mut mm).expect("failed to load from fresh memory");
        assert_eq!(registry.tables.len(), 0);
    }

    #[test]
    fn test_save_and_reload() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        registry
            .register_table::<User>(&mut mm)
            .expect("failed to register");
        // Modify in-memory, then explicitly save
        registry
            .register_table::<AnotherTable>(&mut mm)
            .expect("failed to register another");
        registry.save(&mut mm).expect("failed to save");

        let reloaded = SchemaRegistry::load(&mut mm).expect("failed to reload");
        assert_eq!(reloaded.tables.len(), 2);
        assert_eq!(registry, reloaded);
    }

    #[test]
    fn test_schema_registry_size() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        // Empty size: 8 bytes for length
        assert_eq!(registry.size(), 8);
        registry
            .register_table::<User>(&mut mm)
            .expect("failed to register");
        // One entry without autoincrement: 8 + (8 + 4 + 4 + 4 + 4 + 1) = 33
        // (1 byte for autoincrement flag, no page bytes since User has no autoincrement column)
        assert_eq!(registry.size(), 33);
    }

    #[test]
    fn test_should_allocate_autoincrement_page_when_column_has_autoincrement() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        let pages = registry
            .register_table::<AutoincrementTable>(&mut mm)
            .expect("failed to register autoincrement table");

        assert!(
            pages.autoincrement_registry_page.is_some(),
            "autoincrement registry page should be allocated for tables with autoincrement columns"
        );
    }

    #[test]
    fn test_should_not_allocate_autoincrement_page_when_no_autoincrement_column() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register user table");

        assert!(
            pages.autoincrement_registry_page.is_none(),
            "autoincrement registry page should not be allocated for tables without autoincrement columns"
        );
    }

    #[test]
    fn test_schema_registry_size_with_autoincrement() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        registry
            .register_table::<AutoincrementTable>(&mut mm)
            .expect("failed to register");
        // One entry with autoincrement: 8 + (8 + 4 + 4 + 4 + 4 + 1 + 4) = 37
        // (1 byte for autoincrement flag + 4 bytes for the autoincrement page)
        assert_eq!(registry.size(), 37);
    }

    #[test]
    fn test_should_encode_and_decode_registry_with_autoincrement() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();

        registry
            .register_table::<AutoincrementTable>(&mut mm)
            .expect("failed to register");

        let encoded = registry.encode();
        let decoded = SchemaRegistry::decode(encoded).expect("failed to decode");
        assert_eq!(registry, decoded);

        let page = decoded
            .table_registry_page::<AutoincrementTable>()
            .expect("missing autoincrement table");
        assert!(page.autoincrement_registry_page.is_some());
    }

    // -- AutoincrementTable mock for tests --

    #[derive(Clone, CandidType)]
    struct AutoincrementTable;

    impl Encode for AutoincrementTable {
        const SIZE: DataSize = DataSize::Dynamic;
        const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

        fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
            std::borrow::Cow::Owned(vec![])
        }

        fn decode(_data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
        where
            Self: Sized,
        {
            Ok(AutoincrementTable)
        }

        fn size(&self) -> MSize {
            0
        }
    }

    #[derive(Clone, CandidType, Deserialize)]
    struct AutoincrementTableRecord;

    impl TableRecord for AutoincrementTableRecord {
        type Schema = AutoincrementTable;

        fn from_values(_values: TableColumns) -> Self {
            AutoincrementTableRecord
        }

        fn to_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AutoincrementTableInsert;

    impl InsertRecord for AutoincrementTableInsert {
        type Record = AutoincrementTableRecord;
        type Schema = AutoincrementTable;

        fn from_values(_values: &[(ColumnDef, wasm_dbms_api::prelude::Value)]) -> DbmsResult<Self> {
            Ok(AutoincrementTableInsert)
        }

        fn into_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            AutoincrementTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct AutoincrementTableUpdate;

    impl UpdateRecord for AutoincrementTableUpdate {
        type Record = AutoincrementTableRecord;
        type Schema = AutoincrementTable;

        fn from_values(
            _values: &[(ColumnDef, wasm_dbms_api::prelude::Value)],
            _where_clause: Option<wasm_dbms_api::prelude::Filter>,
        ) -> Self {
            AutoincrementTableUpdate
        }

        fn update_values(&self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<wasm_dbms_api::prelude::Filter> {
            None
        }
    }

    impl TableSchema for AutoincrementTable {
        type Record = AutoincrementTableRecord;
        type Insert = AutoincrementTableInsert;
        type Update = AutoincrementTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "autoincrement_table"
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

        fn to_values(self) -> Vec<(ColumnDef, wasm_dbms_api::prelude::Value)> {
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

    // -- Migration-engine entry points -------------------------------------

    use wasm_dbms_api::prelude::{ColumnSnapshot, DataTypeSnapshot, TableSchemaSnapshot};

    fn dummy_snapshot(name: &str) -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: name.to_string(),
            primary_key: "id".to_string(),
            alignment: 8,
            columns: vec![ColumnSnapshot {
                name: "id".to_string(),
                data_type: DataTypeSnapshot::Uint32,
                nullable: false,
                auto_increment: false,
                unique: true,
                primary_key: true,
                foreign_key: None,
                default: None,
            }],
            indexes: vec![],
        }
    }

    #[test]
    fn test_table_registry_page_by_name_returns_pages_for_registered_table() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register user");

        let by_name = registry
            .table_registry_page_by_name("users")
            .expect("missing pages by name");
        assert_eq!(by_name, pages);
    }

    #[test]
    fn test_table_registry_page_by_name_returns_none_for_unknown_table() {
        let registry = SchemaRegistry::default();
        assert!(registry.table_registry_page_by_name("missing").is_none());
    }

    #[test]
    fn test_stored_snapshots_returns_empty_for_unregistered_registry() {
        let mut mm = make_mm();
        let registry = SchemaRegistry::default();
        let snapshots = registry
            .stored_snapshots(&mut mm)
            .expect("failed to read snapshots");
        assert!(snapshots.is_empty());
    }

    #[test]
    fn test_stored_snapshots_returns_one_entry_per_registered_table() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        registry
            .register_table::<User>(&mut mm)
            .expect("failed to register user");
        registry
            .register_table::<AnotherTable>(&mut mm)
            .expect("failed to register another");

        let snapshots = registry
            .stored_snapshots(&mut mm)
            .expect("failed to load snapshots");
        assert_eq!(snapshots.len(), 2);
        let names: Vec<&str> = snapshots.iter().map(|snap| snap.name.as_str()).collect();
        assert!(names.contains(&"users"));
        assert!(names.contains(&"another_table"));
    }

    #[test]
    fn test_register_table_from_snapshot_allocates_pages_and_persists_snapshot() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let snapshot = dummy_snapshot("fresh");

        let pages = registry
            .register_table_from_snapshot(&snapshot, &mut mm)
            .expect("failed to register from snapshot");

        let loaded = SchemaSnapshotLedger::load(pages.schema_snapshot_page, &mut mm).expect("load");
        assert_eq!(loaded.get(), &snapshot);
        assert!(registry.table_registry_page_by_name("fresh").is_some());
    }

    #[test]
    fn test_register_table_from_snapshot_is_idempotent_for_same_name() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let snapshot = dummy_snapshot("fresh");

        let first = registry
            .register_table_from_snapshot(&snapshot, &mut mm)
            .expect("first");
        let second = registry
            .register_table_from_snapshot(&snapshot, &mut mm)
            .expect("second");
        assert_eq!(first, second);
    }

    #[test]
    fn test_register_table_from_snapshot_detects_name_collision() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let snapshot = dummy_snapshot("users");

        let pages = registry
            .register_table_from_snapshot(&snapshot, &mut mm)
            .expect("first");

        // Tamper persisted snapshot to simulate a colliding fingerprint with a
        // different name.
        let mut tampered = snapshot.clone();
        tampered.name = "imposter".to_string();
        mm.write_at(pages.schema_snapshot_page, 0, &tampered)
            .expect("overwrite");

        let result = registry.register_table_from_snapshot(&snapshot, &mut mm);
        assert!(matches!(
            result,
            Err(MemoryError::NameCollision {
                ref candidate,
                ref existing,
            }) if candidate == "users" && existing == "imposter"
        ));
    }

    #[test]
    fn test_unregister_table_removes_entry_and_returns_previous_pages() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let pages = registry
            .register_table::<User>(&mut mm)
            .expect("failed to register");

        let removed = registry
            .unregister_table("users", &mut mm)
            .expect("unregister");
        assert_eq!(removed, Some(pages));
        assert!(registry.table_registry_page_by_name("users").is_none());
    }

    #[test]
    fn test_unregister_table_returns_none_for_unknown_table() {
        let mut mm = make_mm();
        let mut registry = SchemaRegistry::default();
        let removed = registry
            .unregister_table("missing", &mut mm)
            .expect("unregister");
        assert!(removed.is_none());
    }
}

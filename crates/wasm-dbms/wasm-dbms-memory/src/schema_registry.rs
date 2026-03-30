// Rust guideline compliant 2026-02-28

use std::collections::HashMap;

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryResult, Page, PageOffset, TableFingerprint,
    TableSchema,
};

use crate::table_registry::IndexLedger;
use crate::{MemoryAccess, MemoryManager, MemoryProvider};

/// Data regarding the table registry page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRegistryPage {
    pub pages_list_page: Page,
    pub free_segments_page: Page,
    pub index_registry_page: Page,
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
    /// The [`TableSchema`] type parameter is used to get the [`TableSchema::fingerprint`] of the table schema.
    pub fn register_table<TS>(
        &mut self,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<TableRegistryPage>
    where
        TS: TableSchema,
    {
        // check if already registered
        let fingerprint = TS::fingerprint();
        if let Some(pages) = self.tables.get(&fingerprint) {
            return Ok(*pages);
        }

        // allocate table registry page
        let pages_list_page = mm.allocate_page()?;
        let free_segments_page = mm.allocate_page()?;
        let index_registry_page = mm.allocate_page()?;

        // insert into tables map
        let pages = TableRegistryPage {
            pages_list_page,
            free_segments_page,
            index_registry_page,
        };
        self.tables.insert(fingerprint, pages);

        // get schema page
        let page = mm.schema_page();
        // write self to schema page
        mm.write_at(page, 0, self)?;

        // init index ledger for this table
        IndexLedger::init(pages.index_registry_page, TS::indexes(), mm)?;

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
            buffer.extend_from_slice(&page.pages_list_page.to_le_bytes());
            buffer.extend_from_slice(&page.free_segments_page.to_le_bytes());
            buffer.extend_from_slice(&page.index_registry_page.to_le_bytes());
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
            let pages_list_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let deleted_records_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            let index_registry_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            tables.insert(
                fingerprint,
                TableRegistryPage {
                    pages_list_page,
                    free_segments_page: deleted_records_page,
                    index_registry_page,
                },
            );
        }
        Ok(Self { tables })
    }

    fn size(&self) -> MSize {
        // - 8 bytes for `self.tables.len()`
        // - for each entry:
        //  - 8 bytes for the fingerprint
        //  - 4 bytes for the pages_list_page
        //  - 4 bytes for the free_segments_page
        //  - 4 bytes for the index_registry_page
        8 + (self.tables.len() as MSize * (4 * 3 + 8))
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
        // One entry: 8 + (8 + 4 + 4 + 4) = 28
        assert_eq!(registry.size(), 28);
    }
}

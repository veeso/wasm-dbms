//! Schema snapshot ledger management for the table registry.
//!
//! The schema snapshot ledger persists a single [`TableSchemaSnapshot`] on a dedicated page in
//! stable memory. The snapshot is the frozen, comparable view of a [`TableSchema`] at the time
//! the table was registered, and is used by the migration engine to diff the on-disk schema
//! against the compile-time schema and derive the migration steps required to bring the table
//! up to date.

use wasm_dbms_api::memory::{MemoryResult, Page};
use wasm_dbms_api::prelude::{TableSchema, TableSchemaSnapshot};

use crate::MemoryAccess;

/// Ledger that owns the in-memory cache of the on-disk [`TableSchemaSnapshot`] for one table.
///
/// The ledger is the single read/write entry point for the snapshot stored on the
/// `schema_snapshot_page` allocated by the schema registry. After [`Self::init`] has been called
/// once for a table, subsequent loads use [`Self::load`] to deserialize the persisted snapshot,
/// and [`Self::write`] to replace it (e.g. after a successful migration).
pub struct SchemaSnapshotLedger {
    /// Cached copy of the snapshot persisted on the dedicated page, kept in sync with stable
    /// memory by [`Self::init`], [`Self::load`] and [`Self::write`].
    snapshot: TableSchemaSnapshot,
}

impl SchemaSnapshotLedger {
    /// Builds the [`TableSchemaSnapshot`] for [`TableSchema`] and writes it to `page`.
    ///
    /// Must be called exactly once per table, when the schema registry first allocates the
    /// `schema_snapshot_page`. The snapshot is captured from the compile-time schema definition
    /// via [`TableSchema::schema_snapshot`].
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`](wasm_dbms_api::memory::MemoryError) if the underlying memory
    /// write fails.
    pub fn init<Schema>(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<()>
    where
        Schema: TableSchema,
    {
        let schema_snapshot = Schema::schema_snapshot();
        mm.write_at(page, 0, &schema_snapshot)
    }

    /// Loads a previously persisted snapshot from `page` and returns the ledger wrapping it.
    ///
    /// The page must have been initialized by a prior call to [`Self::init`]; loading from an
    /// uninitialized page will fail at decode time.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`](wasm_dbms_api::memory::MemoryError) if the page cannot be read
    /// or the persisted bytes do not decode into a valid [`TableSchemaSnapshot`].
    pub fn load(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        let snapshot = mm.read_at::<TableSchemaSnapshot>(page, 0)?;
        Ok(Self { snapshot })
    }

    /// Replaces the persisted snapshot with `snapshot` and updates the in-memory cache.
    ///
    /// Used to record a new schema version after a migration has been applied, so that subsequent
    /// loads observe the post-migration shape of the table.
    ///
    /// # Errors
    ///
    /// Returns a [`MemoryError`](wasm_dbms_api::memory::MemoryError) if the underlying memory
    /// write fails. On error the in-memory cache is left untouched.
    pub fn write(
        &mut self,
        page: Page,
        snapshot: TableSchemaSnapshot,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        mm.write_at(page, 0, &snapshot)?;
        self.snapshot = snapshot;
        Ok(())
    }

    /// Returns the cached [`TableSchemaSnapshot`] held by the ledger.
    pub fn get(&self) -> &TableSchemaSnapshot {
        &self.snapshot
    }
}

#[cfg(test)]
mod tests {

    use candid::CandidType;
    use serde::{Deserialize, Serialize};
    use wasm_dbms_api::prelude::{
        ColumnDef, ColumnSnapshot, DEFAULT_ALIGNMENT, DataSize, DataTypeKind, DataTypeSnapshot,
        DbmsResult, Encode, Filter, IndexDef, IndexSnapshot, InsertRecord, MSize, MemoryResult,
        NoForeignFetcher, PageOffset, Sanitize, TableColumns, TableRecord, UpdateRecord, Validate,
        Value,
    };

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    // -- Mock: User table with two columns and one index --

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
            Ok(Self)
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
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct UserInsert;

    impl InsertRecord for UserInsert {
        type Record = UserRecord;
        type Schema = User;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
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

        fn from_values(_values: &[(ColumnDef, Value)], _where_clause: Option<Filter>) -> Self {
            Self
        }

        fn update_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn where_clause(&self) -> Option<Filter> {
            None
        }
    }

    impl wasm_dbms_api::prelude::TableSchema for User {
        type Record = UserRecord;
        type Insert = UserInsert;
        type Update = UserUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "users"
        }

        fn columns() -> &'static [ColumnDef] {
            &[
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Uint32,
                    auto_increment: true,
                    nullable: false,
                    primary_key: true,
                    unique: true,
                    foreign_key: None,
                    default: None,
                    renamed_from: &[],
                },
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    auto_increment: false,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
                    default: None,
                    renamed_from: &[],
                },
            ]
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

        fn sanitizer(_column_name: &'static str) -> Option<Box<dyn Sanitize>> {
            None
        }

        fn validator(_column_name: &'static str) -> Option<Box<dyn Validate>> {
            None
        }
    }

    fn other_snapshot() -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: "other".to_string(),
            primary_key: "id".to_string(),
            alignment: DEFAULT_ALIGNMENT as u32,
            columns: vec![ColumnSnapshot {
                name: "id".to_string(),
                data_type: DataTypeSnapshot::Uint64,
                nullable: false,
                auto_increment: false,
                unique: true,
                primary_key: true,
                foreign_key: None,
                default: None,
            }],
            indexes: vec![IndexSnapshot {
                columns: vec!["id".to_string()],
                unique: true,
            }],
        }
    }

    // -- Tests --

    #[test]
    fn test_init_persists_schema_snapshot_to_page() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");

        let persisted = mm
            .read_at::<TableSchemaSnapshot>(page, 0)
            .expect("failed to read snapshot from page");

        assert_eq!(persisted, User::schema_snapshot());
    }

    #[test]
    fn test_load_returns_snapshot_written_by_init() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");
        let ledger = SchemaSnapshotLedger::load(page, &mut mm).expect("load failed");

        assert_eq!(ledger.get(), &User::schema_snapshot());
    }

    #[test]
    fn test_load_uninitialized_page_returns_error() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let result = SchemaSnapshotLedger::load(page, &mut mm);

        assert!(result.is_err());
    }

    #[test]
    fn test_get_returns_cached_snapshot() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");
        let ledger = SchemaSnapshotLedger::load(page, &mut mm).expect("load failed");

        let cached = ledger.get();
        assert_eq!(cached.name, "users");
        assert_eq!(cached.primary_key, "id");
        assert_eq!(cached.columns.len(), 2);
        assert_eq!(cached.indexes.len(), 1);
    }

    #[test]
    fn test_write_updates_in_memory_cache() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");
        let mut ledger = SchemaSnapshotLedger::load(page, &mut mm).expect("load failed");

        let new_snapshot = other_snapshot();
        ledger
            .write(page, new_snapshot.clone(), &mut mm)
            .expect("write failed");

        assert_eq!(ledger.get(), &new_snapshot);
    }

    #[test]
    fn test_write_persists_new_snapshot_to_page() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");
        let mut ledger = SchemaSnapshotLedger::load(page, &mut mm).expect("load failed");

        let new_snapshot = other_snapshot();
        ledger
            .write(page, new_snapshot.clone(), &mut mm)
            .expect("write failed");

        // reload from the same page to confirm the new snapshot is on disk
        let reloaded = SchemaSnapshotLedger::load(page, &mut mm).expect("reload failed");
        assert_eq!(reloaded.get(), &new_snapshot);
    }

    #[test]
    fn test_write_can_overwrite_multiple_times() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        SchemaSnapshotLedger::init::<User>(page, &mut mm).expect("init failed");
        let mut ledger = SchemaSnapshotLedger::load(page, &mut mm).expect("load failed");

        let first = other_snapshot();
        ledger
            .write(page, first.clone(), &mut mm)
            .expect("first write failed");
        assert_eq!(ledger.get(), &first);

        let mut second = other_snapshot();
        second.name = "second".to_string();
        ledger
            .write(page, second.clone(), &mut mm)
            .expect("second write failed");

        assert_eq!(ledger.get(), &second);
        let reloaded = SchemaSnapshotLedger::load(page, &mut mm).expect("reload failed");
        assert_eq!(reloaded.get(), &second);
    }

    #[test]
    fn test_init_isolates_pages_for_different_schemas() {
        let mut mm = make_mm();
        let user_page = mm.allocate_page().expect("failed to allocate user page");
        let other_page = mm.allocate_page().expect("failed to allocate other page");

        SchemaSnapshotLedger::init::<User>(user_page, &mut mm).expect("user init failed");

        // write a different snapshot on the other page directly
        let other = other_snapshot();
        mm.write_at(other_page, 0, &other)
            .expect("failed to write other snapshot");

        let user_ledger = SchemaSnapshotLedger::load(user_page, &mut mm).expect("user load failed");
        let other_ledger =
            SchemaSnapshotLedger::load(other_page, &mut mm).expect("other load failed");

        assert_eq!(user_ledger.get(), &User::schema_snapshot());
        assert_eq!(other_ledger.get(), &other);
        assert_ne!(user_ledger.get(), other_ledger.get());
    }
}

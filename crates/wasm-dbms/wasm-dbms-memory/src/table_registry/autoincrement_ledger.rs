//! Ledger for autoincrement values, used by tables with autoincrement columns.

mod registry;

use wasm_dbms_api::memory::{MemoryResult, Page};
use wasm_dbms_api::prelude::{DataTypeKind, TableSchema, Value};

use self::registry::AutoincrementRegistry;
use crate::MemoryAccess;

/// Ledger for autoincrement values, used by tables with autoincrement columns.
#[derive(Debug)]
pub struct AutoincrementLedger {
    /// page used to store autoincrement values
    page: Page,
    /// mapping between the column name and the current autoincrement value for that column
    registry: AutoincrementRegistry,
}

impl AutoincrementLedger {
    /// Initialize the [`AutoincrementLedger`] for the given table schema, and write it to the given page.
    ///
    /// Each autoincrement column in the table schema will be initialized in the registry with the appropriate zero value.
    pub fn init<TS>(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self>
    where
        TS: TableSchema,
    {
        let mut registry = AutoincrementRegistry::default();
        // init each autoincrement column in the registry with the appropriate zero value
        for auto_increment_column in TS::columns().iter().filter(|c| c.auto_increment) {
            let zero = Self::zero(auto_increment_column.data_type);
            registry.init(auto_increment_column.name, zero);
        }
        // write the registry to the page
        mm.write_at(page, 0, &registry)?;

        Ok(Self { page, registry })
    }

    /// Load the [`AutoincrementLedger`] from the given page.
    pub fn load(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        Ok(Self {
            page,
            registry: mm.read_at(page, 0)?,
        })
    }

    /// Returns the next autoincrement [`Value`] for the given column, and persists the updated registry to memory.
    ///
    /// # Errors
    ///
    /// Returns [`MemoryError::AutoincrementOverflow`] if the column has reached its maximum value.
    pub fn next(&mut self, column: &str, mm: &mut impl MemoryAccess) -> MemoryResult<Value> {
        let value = self.registry.next(column)?;
        mm.write_at(self.page, 0, &self.registry)?;
        Ok(value)
    }

    /// Returns the zero [`Value`] based on the column [`DataTypeKind`].
    fn zero(data_type: DataTypeKind) -> Value {
        match data_type {
            DataTypeKind::Int8 => Value::Int8(0.into()),
            DataTypeKind::Int16 => Value::Int16(0.into()),
            DataTypeKind::Int32 => Value::Int32(0.into()),
            DataTypeKind::Int64 => Value::Int64(0.into()),
            DataTypeKind::Uint8 => Value::Uint8(0.into()),
            DataTypeKind::Uint16 => Value::Uint16(0.into()),
            DataTypeKind::Uint32 => Value::Uint32(0.into()),
            DataTypeKind::Uint64 => Value::Uint64(0.into()),
            data_type => panic!("unsupported autoincrement type: {data_type:?}"),
        }
    }
}

#[cfg(test)]
mod tests {

    use candid::CandidType;
    use serde::{Deserialize, Serialize};
    use wasm_dbms_api::prelude::{
        ColumnDef, DEFAULT_ALIGNMENT, DataSize, DbmsResult, Encode, IndexDef, InsertRecord, MSize,
        MemoryError, MemoryResult, NoForeignFetcher, PageOffset, TableColumns, TableRecord,
        TableSchema, UpdateRecord, Value,
    };

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    // -- Mock: single Uint32 autoincrement column --

    #[derive(Clone, CandidType)]
    struct SingleAutoincTable;

    impl Encode for SingleAutoincTable {
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
    struct SingleAutoincTableRecord;

    impl TableRecord for SingleAutoincTableRecord {
        type Schema = SingleAutoincTable;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct SingleAutoincTableInsert;

    impl InsertRecord for SingleAutoincTableInsert {
        type Record = SingleAutoincTableRecord;
        type Schema = SingleAutoincTable;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            SingleAutoincTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct SingleAutoincTableUpdate;

    impl UpdateRecord for SingleAutoincTableUpdate {
        type Record = SingleAutoincTableRecord;
        type Schema = SingleAutoincTable;

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

    impl TableSchema for SingleAutoincTable {
        type Record = SingleAutoincTableRecord;
        type Insert = SingleAutoincTableInsert;
        type Update = SingleAutoincTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "single_autoinc"
        }

        fn columns() -> &'static [ColumnDef] {
            &[ColumnDef {
                name: "id",
                data_type: DataTypeKind::Uint32,
                auto_increment: true,
                nullable: false,
                primary_key: true,
                unique: true,
                foreign_key: None,
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

    // -- Mock: two autoincrement columns --

    #[derive(Clone, CandidType)]
    struct MultiAutoincTable;

    impl Encode for MultiAutoincTable {
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
    struct MultiAutoincTableRecord;

    impl TableRecord for MultiAutoincTableRecord {
        type Schema = MultiAutoincTable;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct MultiAutoincTableInsert;

    impl InsertRecord for MultiAutoincTableInsert {
        type Record = MultiAutoincTableRecord;
        type Schema = MultiAutoincTable;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            MultiAutoincTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct MultiAutoincTableUpdate;

    impl UpdateRecord for MultiAutoincTableUpdate {
        type Record = MultiAutoincTableRecord;
        type Schema = MultiAutoincTable;

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

    impl TableSchema for MultiAutoincTable {
        type Record = MultiAutoincTableRecord;
        type Insert = MultiAutoincTableInsert;
        type Update = MultiAutoincTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "multi_autoinc"
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
                },
                ColumnDef {
                    name: "seq",
                    data_type: DataTypeKind::Uint64,
                    auto_increment: true,
                    nullable: false,
                    primary_key: false,
                    unique: false,
                    foreign_key: None,
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

    // -- Mock: no autoincrement columns --

    #[derive(Clone, CandidType)]
    struct NoAutoincTable;

    impl Encode for NoAutoincTable {
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
    struct NoAutoincTableRecord;

    impl TableRecord for NoAutoincTableRecord {
        type Schema = NoAutoincTable;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct NoAutoincTableInsert;

    impl InsertRecord for NoAutoincTableInsert {
        type Record = NoAutoincTableRecord;
        type Schema = NoAutoincTable;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            NoAutoincTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct NoAutoincTableUpdate;

    impl UpdateRecord for NoAutoincTableUpdate {
        type Record = NoAutoincTableRecord;
        type Schema = NoAutoincTable;

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

    impl TableSchema for NoAutoincTable {
        type Record = NoAutoincTableRecord;
        type Insert = NoAutoincTableInsert;
        type Update = NoAutoincTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "no_autoinc"
        }

        fn columns() -> &'static [ColumnDef] {
            &[ColumnDef {
                name: "id",
                data_type: DataTypeKind::Uint32,
                auto_increment: false,
                nullable: false,
                primary_key: true,
                unique: true,
                foreign_key: None,
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

    // -- Tests --

    #[test]
    fn test_init_single_autoincrement_column() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let ledger = AutoincrementLedger::init::<SingleAutoincTable>(page, &mut mm)
            .expect("failed to init ledger");

        // verify the page was stored
        assert_eq!(ledger.page, page);
    }

    #[test]
    fn test_init_multiple_autoincrement_columns() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let mut ledger = AutoincrementLedger::init::<MultiAutoincTable>(page, &mut mm)
            .expect("failed to init ledger");

        // both columns should produce their first value
        let id_val = ledger.next("id", &mut mm).expect("failed to get next id");
        let seq_val = ledger.next("seq", &mut mm).expect("failed to get next seq");

        assert_eq!(id_val, Value::Uint32(1u32.into()));
        assert_eq!(seq_val, Value::Uint64(1u64.into()));
    }

    #[test]
    fn test_init_no_autoincrement_columns() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        // should succeed but the registry is empty
        let ledger = AutoincrementLedger::init::<NoAutoincTable>(page, &mut mm)
            .expect("failed to init ledger");

        assert_eq!(ledger.page, page);
    }

    #[test]
    fn test_load_after_init() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let mut ledger =
            AutoincrementLedger::init::<SingleAutoincTable>(page, &mut mm).expect("failed to init");

        // advance once
        let _ = ledger.next("id", &mut mm).expect("next failed");

        // load from memory and continue
        let mut reloaded = AutoincrementLedger::load(page, &mut mm).expect("failed to load ledger");

        let value = reloaded
            .next("id", &mut mm)
            .expect("next after reload failed");
        assert_eq!(value, Value::Uint32(2u32.into()));
    }

    #[test]
    fn test_next_returns_sequential_values() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let mut ledger =
            AutoincrementLedger::init::<SingleAutoincTable>(page, &mut mm).expect("failed to init");

        for expected in 1u32..=100 {
            let value = ledger.next("id", &mut mm).expect("next failed");
            assert_eq!(value, Value::Uint32(expected.into()));
        }
    }

    #[test]
    fn test_next_persists_to_memory() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let mut ledger =
            AutoincrementLedger::init::<SingleAutoincTable>(page, &mut mm).expect("failed to init");

        // advance 5 times
        for _ in 0..5 {
            let _ = ledger.next("id", &mut mm).expect("next failed");
        }

        // reload and verify the counter continued from where it was
        let mut reloaded = AutoincrementLedger::load(page, &mut mm).expect("failed to load ledger");
        let value = reloaded
            .next("id", &mut mm)
            .expect("next after reload failed");
        assert_eq!(value, Value::Uint32(6u32.into()));
    }

    #[test]
    fn test_next_overflow_returns_error() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        // init with Uint8 to quickly reach max
        let mut ledger =
            AutoincrementLedger::init::<Uint8AutoincTable>(page, &mut mm).expect("failed to init");

        // advance to 255
        for _ in 0..255 {
            let _ = ledger.next("counter", &mut mm).expect("next failed");
        }

        // next should fail with overflow
        let result = ledger.next("counter", &mut mm);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MemoryError::AutoincrementOverflow(_)
        ));
    }

    #[test]
    fn test_multi_column_independence_across_reload() {
        let mut mm = make_mm();
        let page = mm.allocate_page().expect("failed to allocate page");

        let mut ledger =
            AutoincrementLedger::init::<MultiAutoincTable>(page, &mut mm).expect("failed to init");

        // advance id 3 times, seq 1 time
        for _ in 0..3 {
            let _ = ledger.next("id", &mut mm).expect("id next failed");
        }
        let _ = ledger.next("seq", &mut mm).expect("seq next failed");

        // reload and verify both columns preserved their independent state
        let mut reloaded = AutoincrementLedger::load(page, &mut mm).expect("failed to load ledger");

        let id_val = reloaded
            .next("id", &mut mm)
            .expect("id next after reload failed");
        let seq_val = reloaded
            .next("seq", &mut mm)
            .expect("seq next after reload failed");

        assert_eq!(id_val, Value::Uint32(4u32.into()));
        assert_eq!(seq_val, Value::Uint64(2u64.into()));
    }

    // -- Mock: Uint8 autoincrement column (for overflow testing) --

    #[derive(Clone, CandidType)]
    struct Uint8AutoincTable;

    impl Encode for Uint8AutoincTable {
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
    struct Uint8AutoincTableRecord;

    impl TableRecord for Uint8AutoincTableRecord {
        type Schema = Uint8AutoincTable;

        fn from_values(_values: TableColumns) -> Self {
            Self
        }

        fn to_values(&self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct Uint8AutoincTableInsert;

    impl InsertRecord for Uint8AutoincTableInsert {
        type Record = Uint8AutoincTableRecord;
        type Schema = Uint8AutoincTable;

        fn from_values(_values: &[(ColumnDef, Value)]) -> DbmsResult<Self> {
            Ok(Self)
        }

        fn into_values(self) -> Vec<(ColumnDef, Value)> {
            vec![]
        }

        fn into_record(self) -> Self::Schema {
            Uint8AutoincTable
        }
    }

    #[derive(Clone, CandidType, Serialize)]
    struct Uint8AutoincTableUpdate;

    impl UpdateRecord for Uint8AutoincTableUpdate {
        type Record = Uint8AutoincTableRecord;
        type Schema = Uint8AutoincTable;

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

    impl TableSchema for Uint8AutoincTable {
        type Record = Uint8AutoincTableRecord;
        type Insert = Uint8AutoincTableInsert;
        type Update = Uint8AutoincTableUpdate;
        type ForeignFetcher = NoForeignFetcher;

        fn table_name() -> &'static str {
            "uint8_autoinc"
        }

        fn columns() -> &'static [ColumnDef] {
            &[ColumnDef {
                name: "counter",
                data_type: DataTypeKind::Uint8,
                auto_increment: true,
                nullable: false,
                primary_key: true,
                unique: true,
                foreign_key: None,
            }]
        }

        fn primary_key() -> &'static str {
            "counter"
        }

        fn indexes() -> &'static [IndexDef] {
            &[IndexDef(&["counter"])]
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

    #[test]
    fn test_zero_values_for_all_integer_types() {
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Int8),
            Value::Int8(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Int16),
            Value::Int16(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Int32),
            Value::Int32(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Int64),
            Value::Int64(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Uint8),
            Value::Uint8(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Uint16),
            Value::Uint16(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Uint32),
            Value::Uint32(0.into())
        );
        assert_eq!(
            AutoincrementLedger::zero(DataTypeKind::Uint64),
            Value::Uint64(0.into())
        );
    }

    #[test]
    #[should_panic(expected = "unsupported autoincrement type")]
    fn test_zero_panics_on_unsupported_type() {
        let _ = AutoincrementLedger::zero(DataTypeKind::Text);
    }
}

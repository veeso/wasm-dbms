// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, DbmsResult, DeleteBehavior, Filter, Query, Value,
};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::database::WasmDbmsDatabase;

/// Provides schema-driven dynamic dispatch for database operations.
///
/// Implementations of this trait know which concrete table types exist
/// and forward generic operations (identified by table name) to the
/// appropriate typed methods on [`WasmDbmsDatabase`].
///
/// This trait is typically implemented by generated code from the
/// `#[derive(DbmsCanister)]` macro.
pub trait DatabaseSchema<M: MemoryProvider> {
    /// Performs a generic select for the given table name and query.
    fn select(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>>;

    /// Performs a join query, returning results with column definitions
    /// that include source table names.
    fn select_join(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        from_table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
        crate::join::JoinEngine::new(self).join(dbms, from_table, query)
    }

    /// Returns tables and columns that reference the given table via foreign keys.
    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)>;

    /// Performs an insert for the given table name.
    fn insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()>;

    /// Performs a delete for the given table name.
    fn delete(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> DbmsResult<u64>;

    /// Performs an update for the given table name.
    fn update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> DbmsResult<u64>;

    /// Validates an insert operation.
    fn validate_insert(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<()>;

    /// Validates an update operation.
    fn validate_update(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> DbmsResult<()>;
}

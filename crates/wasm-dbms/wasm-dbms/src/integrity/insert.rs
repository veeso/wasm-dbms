// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Integrity validator for insert operations.

use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DbmsError, DbmsResult, Filter, Query, QueryError, TableSchema, Value,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryProvider};

use super::common;
use crate::database::WasmDbmsDatabase;

/// Integrity validator for insert operations.
pub struct InsertIntegrityValidator<'a, T, M, A = AccessControlList>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    database: &'a WasmDbmsDatabase<'a, M, A>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T, M, A> InsertIntegrityValidator<'a, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Creates a new insert integrity validator.
    pub fn new(dbms: &'a WasmDbmsDatabase<'a, M, A>) -> Self {
        Self {
            database: dbms,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<T, M, A> InsertIntegrityValidator<'_, T, M, A>
where
    T: TableSchema,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Verifies whether the given insert record is valid.
    pub fn validate(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        for (col, value) in record_values {
            common::check_column_validate::<T>(col, value)?;
        }
        self.check_primary_key_conflict(record_values)?;
        common::check_foreign_keys::<T>(self.database, record_values)?;
        common::check_non_nullable_fields::<T>(record_values)?;

        Ok(())
    }

    /// Checks for primary key conflicts.
    fn check_primary_key_conflict(&self, record_values: &[(ColumnDef, Value)]) -> DbmsResult<()> {
        let pk_name = T::primary_key();
        let pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.name == pk_name)
            .map(|(_, value)| value.clone())
            .ok_or(DbmsError::Query(QueryError::MissingNonNullableField(
                pk_name.to_string(),
            )))?;

        let query: Query = Query::builder()
            .field(pk_name)
            .and_where(Filter::Eq(pk_name.to_string(), pk))
            .build();

        let res = self.database.select::<T>(query)?;
        if res.is_empty() {
            Ok(())
        } else {
            Err(DbmsError::Query(QueryError::PrimaryKeyConflict))
        }
    }
}

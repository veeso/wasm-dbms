use crate::error::DbmsResult;
use crate::prelude::{
    ColumnDef, DeleteBehavior, Filter, InsertRecord, JoinColumnDef, Query, TableSchema,
    UpdateRecord, Value,
};

/// This module defines the Database trait and related database functionalities.
pub trait Database {
    /// Executes a SELECT query and returns the results.
    ///
    /// # Arguments
    ///
    /// - `query` - The SELECT [`Query`] to be executed.
    ///
    /// # Returns
    ///
    /// The returned results are a vector of [`table::TableRecord`] matching the query.
    fn select<T>(&self, query: Query) -> DbmsResult<Vec<T::Record>>
    where
        T: TableSchema;

    /// Executes a generic SELECT and returns raw column-value pairs.
    ///
    /// Unlike [`Database::select`], this method does not require a concrete
    /// table type. It takes a table name and dispatches internally.
    fn select_raw(&self, table: &str, query: Query) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>>;

    /// Executes a join query, returning results with column definitions
    /// that include source table names.
    ///
    /// Use `table.column` syntax in field selection, filters, and ordering
    /// to disambiguate columns that share the same name across joined tables.
    fn select_join(
        &self,
        table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(JoinColumnDef, Value)>>>;

    /// Executes an INSERT query.
    ///
    /// # Arguments
    ///
    /// - `record` - The INSERT record to be executed.
    fn insert<T>(&self, record: T::Insert) -> DbmsResult<()>
    where
        T: TableSchema,
        T::Insert: InsertRecord<Schema = T>;

    /// Executes an UPDATE query.
    ///
    /// # Arguments
    ///
    /// - `patch` - The UPDATE patch to be applied.
    /// - `filter` - An optional [`Filter`] to specify which records to update.
    ///
    /// # Returns
    ///
    /// The number of rows updated.
    fn update<T>(&self, patch: T::Update) -> DbmsResult<u64>
    where
        T: TableSchema,
        T::Update: UpdateRecord<Schema = T>;

    /// Executes a DELETE query.
    ///
    /// # Arguments
    ///
    /// - `behaviour` - The [`DeleteBehavior`] to apply for foreign key constraints.
    /// - `filter` - An optional [`Filter`] to specify which records to delete.
    ///
    /// # Returns
    ///
    /// The number of rows deleted.
    fn delete<T>(&self, behaviour: DeleteBehavior, filter: Option<Filter>) -> DbmsResult<u64>
    where
        T: TableSchema;

    /// Commits the current transaction.
    ///
    /// The transaction is consumed.
    ///
    /// Any error during commit will panic to ensure consistency.
    fn commit(&mut self) -> DbmsResult<()>;

    /// Rolls back the current transaction.
    ///
    /// The transaction is consumed.
    fn rollback(&mut self) -> DbmsResult<()>;
}

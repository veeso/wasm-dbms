use crate::prelude::{
    DeleteBehavior, Filter, IcDbmsResult, InsertRecord, Query, TableSchema, UpdateRecord,
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
    fn select<T>(&self, query: Query) -> IcDbmsResult<Vec<T::Record>>
    where
        T: TableSchema;

    /// Executes an INSERT query.
    ///
    /// # Arguments
    ///
    /// - `record` - The INSERT record to be executed.
    fn insert<T>(&self, record: T::Insert) -> IcDbmsResult<()>
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
    fn update<T>(&self, patch: T::Update) -> IcDbmsResult<u64>
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
    fn delete<T>(&self, behaviour: DeleteBehavior, filter: Option<Filter>) -> IcDbmsResult<u64>
    where
        T: TableSchema;

    /// Commits the current transaction.
    ///
    /// The transaction is consumed.
    ///
    /// Any error during commit will trap the canister to ensure consistency.
    fn commit(&mut self) -> IcDbmsResult<()>;

    /// Rolls back the current transaction.
    ///
    /// The transaction is consumed.
    fn rollback(&mut self) -> IcDbmsResult<()>;
}

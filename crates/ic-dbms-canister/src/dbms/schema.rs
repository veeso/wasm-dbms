use ic_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, DeleteBehavior, Filter, IcDbmsResult, Query, Value,
};

use crate::dbms::IcDbmsDatabase;

/// This trait provides the schema operation for the current database.
///
/// It must provide the functionalities to validate the operations and perform them using the [`Database`] instance.
///
/// This is required because all of the [`Database`] operations rely on `T`, a [`crate::prelude::TableSchema`], but we can't store them inside
/// of transactions without knowing the concrete type at compile time.
pub trait DatabaseSchema {
    /// Performs a generic select for the given table name and query.
    ///
    /// Returns raw column-value pairs instead of typed records.
    fn select(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &str,
        query: Query,
    ) -> IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>>;

    /// Performs a join query, returning results with [`CandidColumnDef`] (which includes table names).
    fn select_join(
        &self,
        dbms: &IcDbmsDatabase,
        from_table: &str,
        query: Query,
    ) -> IcDbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
        crate::dbms::join::JoinEngine::new(self).join(dbms, from_table, query)
    }

    /// Returns the foreign key definitions referencing other tables for the given table name.
    ///
    /// So if a table `Post` has a foreign key referencing the `User` table, calling
    /// `referenced_tables("User")` would return a list containing:
    /// `[("Post`, &["user_id"])]`.
    fn referenced_tables(&self, table: &'static str) -> Vec<(&'static str, Vec<&'static str>)>;

    /// Performs an insert operation for the given table name and record values.
    ///
    /// Use [`Database::insert`] internally to perform the operation.
    fn insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> IcDbmsResult<()>;

    /// Performs a delete operation for the given table name, delete behavior, and optional filter.
    ///
    /// Use [`Database::delete`] internally to perform the operation.
    fn delete(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        delete_behavior: DeleteBehavior,
        filter: Option<Filter>,
    ) -> IcDbmsResult<u64>;

    /// Performs an update operation for the given table name, patch values, and optional filter.
    ///
    /// Use [`Database::update`] internally to perform the operation.
    fn update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        patch_values: &[(ColumnDef, Value)],
        filter: Option<Filter>,
    ) -> IcDbmsResult<u64>;

    /// Validates an insert operation for the given table name and record values.
    ///
    /// Use a [`crate::prelude::InsertIntegrityValidator`] to perform the validation.
    fn validate_insert(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
    ) -> IcDbmsResult<()>;

    /// Validates an update operation for the given table name and record values.
    ///
    /// The `old_pk` is the current primary key value of the record being updated, used to
    /// distinguish a PK conflict from the record simply keeping its own PK.
    ///
    /// Use a [`crate::prelude::UpdateIntegrityValidator`] to perform the validation.
    fn validate_update(
        &self,
        dbms: &IcDbmsDatabase,
        table_name: &'static str,
        record_values: &[(ColumnDef, Value)],
        old_pk: Value,
    ) -> IcDbmsResult<()>;
}

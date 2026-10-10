use std::collections::HashMap;

use crate::dbms::table::TableColumns;
use crate::dbms::value::Value;
use crate::error::DbmsResult;
use crate::prelude::{ColumnDef, Database};

/// Fetches related records from foreign tables referenced by foreign keys.
///
/// A relation is identified by the referenced table and the local foreign key column; records
/// of the referenced table are looked up by the referenced column declared for that foreign key
/// (`ForeignKeyDef::foreign_column`), which is not necessarily the primary key.
///
/// This trait provides two methods:
///
/// - [`ForeignFetcher::fetch`] retrieves a single foreign record by referenced column value.
///   Used during integrity checks (insert/update validation) to verify that a
///   foreign key reference points to an existing record.
///
/// - [`ForeignFetcher::fetch_batch`] retrieves multiple foreign records in one
///   query using `Filter::In`. Used during eager relation loading to resolve the
///   N+1 query problem by batching all FK lookups for a result set.
pub trait ForeignFetcher: Default {
    /// Fetches a single foreign record for integrity validation.
    ///
    /// # Arguments
    ///
    /// * `database` - The database from which to fetch the data.
    /// * `table` - The name of the foreign table to query.
    /// * `local_column` - The local column that holds the foreign key reference.
    /// * `value` - The referenced column value to look up in the foreign table.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::BrokenForeignKeyReference`](crate::prelude::QueryError::BrokenForeignKeyReference)
    /// if no record holds `value`, or an error if the relation is unknown or the query fails.
    fn fetch(
        &self,
        database: &impl Database,
        table: &str,
        local_column: &'static str,
        value: Value,
    ) -> DbmsResult<TableColumns>;

    /// Batch-fetches foreign records for eager relation loading.
    ///
    /// Resolves the N+1 query problem by fetching all foreign records whose
    /// referenced column value is contained in `values` in a single `Filter::In` query.
    ///
    /// # Arguments
    ///
    /// * `database` - The database from which to fetch the data.
    /// * `table` - The name of the foreign table to query.
    /// * `local_column` - The local column that holds the foreign key reference.
    /// * `values` - The distinct referenced column values to look up.
    ///
    /// # Returns
    ///
    /// A map from each referenced column value to its fetched column data.
    ///
    /// # Errors
    ///
    /// Returns an error if the relation is unknown or the query fails.
    fn fetch_batch(
        &self,
        database: &impl Database,
        table: &str,
        local_column: &'static str,
        values: &[Value],
    ) -> DbmsResult<HashMap<Value, Vec<(ColumnDef, Value)>>>;
}

/// A no-op foreign fetcher that does not perform any fetching.
#[derive(Default)]
pub struct NoForeignFetcher;

impl ForeignFetcher for NoForeignFetcher {
    fn fetch(
        &self,
        _database: &impl Database,
        _table: &str,
        _local_column: &'static str,
        _value: Value,
    ) -> DbmsResult<TableColumns> {
        unimplemented!("NoForeignFetcher should have a table without foreign keys");
    }

    fn fetch_batch(
        &self,
        _database: &impl Database,
        _table: &str,
        _local_column: &'static str,
        _values: &[Value],
    ) -> DbmsResult<HashMap<Value, Vec<(ColumnDef, Value)>>> {
        unimplemented!("NoForeignFetcher should have a table without foreign keys");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "NoForeignFetcher should have a table without foreign keys")]
    fn test_no_foreign_fetcher() {
        let fetcher = NoForeignFetcher;
        let _ = fetcher.fetch(
            &MockDatabase,
            "some_table",
            "some_column",
            Value::Uint32(1.into()),
        );
    }

    #[test]
    #[should_panic(expected = "NoForeignFetcher should have a table without foreign keys")]
    fn test_no_foreign_fetcher_batch() {
        let fetcher = NoForeignFetcher;
        let _ = fetcher.fetch_batch(
            &MockDatabase,
            "some_table",
            "some_column",
            &[Value::Uint32(1.into())],
        );
    }

    struct MockDatabase;

    impl Database for MockDatabase {
        fn select<T>(&self, _query: crate::prelude::Query) -> DbmsResult<Vec<T::Record>>
        where
            T: crate::prelude::TableSchema,
        {
            unimplemented!()
        }

        fn insert<T>(&self, _record: T::Insert) -> DbmsResult<()>
        where
            T: crate::prelude::TableSchema,
            T::Insert: crate::prelude::InsertRecord<Schema = T>,
        {
            unimplemented!()
        }

        fn update<T>(&self, _patch: T::Update) -> DbmsResult<u64>
        where
            T: crate::prelude::TableSchema,
            T::Update: crate::prelude::UpdateRecord<Schema = T>,
        {
            unimplemented!()
        }

        fn aggregate<T>(
            &self,
            _query: crate::prelude::Query,
            _aggregates: &[crate::prelude::AggregateFunction],
        ) -> DbmsResult<Vec<crate::prelude::AggregatedRow>>
        where
            T: crate::prelude::TableSchema,
        {
            todo!();
        }

        fn select_raw(
            &self,
            _table: &str,
            _query: crate::prelude::Query,
        ) -> DbmsResult<Vec<Vec<(crate::prelude::ColumnDef, crate::prelude::Value)>>> {
            unimplemented!()
        }

        fn select_join(
            &self,
            _table: &str,
            _query: crate::prelude::Query,
        ) -> DbmsResult<crate::prelude::JoinResultSet> {
            unimplemented!()
        }

        fn delete<T>(
            &self,
            _behaviour: crate::prelude::DeleteBehavior,
            _filter: Option<crate::prelude::Filter>,
        ) -> DbmsResult<u64>
        where
            T: crate::prelude::TableSchema,
        {
            unimplemented!()
        }

        fn commit(&mut self) -> DbmsResult<()> {
            unimplemented!()
        }

        fn rollback(&mut self) -> DbmsResult<()> {
            unimplemented!()
        }

        fn has_drift(&self) -> DbmsResult<bool> {
            unimplemented!()
        }

        fn pending_migrations(&self) -> DbmsResult<Vec<crate::prelude::MigrationOp>> {
            unimplemented!()
        }

        fn migrate(&mut self, _policy: crate::prelude::MigrationPolicy) -> DbmsResult<()> {
            unimplemented!()
        }
    }
}

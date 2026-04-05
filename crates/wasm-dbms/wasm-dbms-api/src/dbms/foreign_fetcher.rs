use std::collections::HashMap;

use crate::dbms::table::TableColumns;
use crate::dbms::value::Value;
use crate::error::DbmsResult;
use crate::prelude::{ColumnDef, Database};

/// Fetches related records from foreign tables referenced by foreign keys.
///
/// This trait provides two methods:
///
/// - [`ForeignFetcher::fetch`] retrieves a single foreign record by primary key.
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
    /// * `pk_value` - The primary key value to look up in the foreign table.
    ///
    /// # Returns
    ///
    /// A result containing the fetched table columns or an error.
    fn fetch(
        &self,
        database: &impl Database,
        table: &str,
        local_column: &'static str,
        pk_value: Value,
    ) -> DbmsResult<TableColumns>;

    /// Batch-fetches foreign records for eager relation loading.
    ///
    /// Resolves the N+1 query problem by fetching all foreign records whose
    /// primary key is contained in `pk_values` in a single `Filter::In` query.
    ///
    /// # Arguments
    ///
    /// * `database` - The database from which to fetch the data.
    /// * `table` - The name of the foreign table to query.
    /// * `pk_values` - The distinct primary key values to look up.
    ///
    /// # Returns
    ///
    /// A map from each primary key value to its fetched column data.
    fn fetch_batch(
        &self,
        database: &impl Database,
        table: &str,
        pk_values: &[Value],
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
        _pk_value: Value,
    ) -> DbmsResult<TableColumns> {
        unimplemented!("NoForeignFetcher should have a table without foreign keys");
    }

    fn fetch_batch(
        &self,
        _database: &impl Database,
        _table: &str,
        _pk_values: &[Value],
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
        let _ = fetcher.fetch_batch(&MockDatabase, "some_table", &[Value::Uint32(1.into())]);
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
        ) -> DbmsResult<Vec<Vec<(crate::prelude::JoinColumnDef, crate::prelude::Value)>>> {
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
    }
}

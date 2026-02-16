use crate::dbms::table::TableColumns;
use crate::dbms::value::Value;
use crate::prelude::{Database, IcDbmsResult};

/// This trait defines the behavior of a foreign fetcher, which is responsible for
/// fetching data from foreign sources or databases.
///
/// It takes a table name and returns the values associated with that table.
pub trait ForeignFetcher: Default {
    /// Fetches the data for the specified table and primary key values.
    ///
    /// # Arguments
    ///
    /// * `database` - The database from which to fetch the data.
    /// * `table` - The name of the table to fetch data from.
    /// * `local_column` - The local column that references the foreign key.
    /// * `pk_value` - The primary key to look for.
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
    ) -> IcDbmsResult<TableColumns>;
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
    ) -> IcDbmsResult<TableColumns> {
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

    struct MockDatabase;

    impl Database for MockDatabase {
        fn select<T>(&self, _query: crate::prelude::Query) -> IcDbmsResult<Vec<T::Record>>
        where
            T: crate::prelude::TableSchema,
        {
            unimplemented!()
        }

        fn insert<T>(&self, _record: T::Insert) -> IcDbmsResult<()>
        where
            T: crate::prelude::TableSchema,
            T::Insert: crate::prelude::InsertRecord<Schema = T>,
        {
            unimplemented!()
        }

        fn update<T>(&self, _patch: T::Update) -> IcDbmsResult<u64>
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
        ) -> IcDbmsResult<Vec<Vec<(crate::prelude::ColumnDef, crate::prelude::Value)>>> {
            unimplemented!()
        }

        fn delete<T>(
            &self,
            _behaviour: crate::prelude::DeleteBehavior,
            _filter: Option<crate::prelude::Filter>,
        ) -> IcDbmsResult<u64>
        where
            T: crate::prelude::TableSchema,
        {
            unimplemented!()
        }

        fn commit(&mut self) -> IcDbmsResult<()> {
            unimplemented!()
        }

        fn rollback(&mut self) -> IcDbmsResult<()> {
            unimplemented!()
        }
    }
}

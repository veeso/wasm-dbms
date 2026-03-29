use std::hash::{Hash as _, Hasher as _};

use crate::dbms::foreign_fetcher::ForeignFetcher;
use crate::dbms::table::column_def::{ColumnDef, IndexDef};
use crate::dbms::table::{InsertRecord, TableRecord, UpdateRecord};
use crate::memory::Encode;
use crate::prelude::{Sanitize, Validate};

/// A type representing a unique fingerprint for a table schema.
pub type TableFingerprint = u64;

/// Table schema representation.
///
/// It is used to define the structure of a database table.
pub trait TableSchema
where
    Self: Encode + 'static,
{
    /// The [`TableRecord`] type associated with this table schema;
    /// which is the data returned by a query.
    type Record: TableRecord<Schema = Self>;
    /// The [`InsertRecord`] type associated with this table schema.
    type Insert: InsertRecord<Schema = Self>;
    /// The [`UpdateRecord`] type associated with this table schema.
    type Update: UpdateRecord<Schema = Self>;
    /// The [`ForeignFetcher`] type associated with this table schema.
    type ForeignFetcher: ForeignFetcher;

    /// Returns the name of the table.
    fn table_name() -> &'static str;

    /// Returns the column definitions of the table.
    fn columns() -> &'static [ColumnDef];

    /// Returns the name of the primary key column.
    fn primary_key() -> &'static str;

    /// Returns the list of indexes defined on the table, where each index
    /// is represented by the list of column names it includes.
    fn indexes() -> &'static [IndexDef] {
        &[]
    }

    /// Converts itself into a vector of column-value pairs.
    fn to_values(self) -> Vec<(ColumnDef, crate::dbms::value::Value)>;

    /// Returns the [`Sanitize`] implementation for the given column name, if any.
    fn sanitizer(column_name: &'static str) -> Option<Box<dyn Sanitize>>;

    /// Returns the [`Validate`] implementation for the given column name, if any.
    fn validator(column_name: &'static str) -> Option<Box<dyn Validate>>;

    /// Returns an instance of the [`ForeignFetcher`] for this table schema.
    fn foreign_fetcher() -> Self::ForeignFetcher {
        Default::default()
    }

    /// Returns the fingerprint of the table schema.
    fn fingerprint() -> TableFingerprint {
        let mut hasher = std::hash::DefaultHasher::new();
        std::any::TypeId::of::<Self>().hash(&mut hasher);
        hasher.finish()
    }
}

// Rust guideline compliant 2026-02-28

//! Database overlay for tracking uncommitted transaction changes.

mod reader;
mod table;

use std::collections::HashMap;

use wasm_dbms_api::prelude::{ColumnDef, DbmsError, DbmsResult, QueryError, TableSchema, Value};
use wasm_dbms_memory::prelude::{MemoryAccess, TableReader};

pub use self::reader::DatabaseOverlayReader;
pub use self::table::IndexOverlay;
pub(crate) use self::table::TableOverlay;

/// Manages uncommitted changes during a transaction.
///
/// Provides an overlay over the existing database state to track
/// uncommitted inserts, updates, and deletes per table.
#[derive(Debug, Default, Clone)]
pub struct DatabaseOverlay {
    tables: HashMap<String, TableOverlay>,
}

impl DatabaseOverlay {
    /// Returns a reader that merges base table data with overlay changes.
    pub fn reader<'a, T, MA>(
        &'a mut self,
        table_reader: TableReader<'a, T, MA>,
    ) -> DatabaseOverlayReader<'a, T, MA>
    where
        T: TableSchema,
        MA: MemoryAccess,
    {
        let table_name = T::table_name();
        let table_overlay = self
            .tables
            .entry(table_name.to_string())
            .or_insert_with(|| TableOverlay::new(T::indexes()));
        DatabaseOverlayReader::new(table_overlay, table_reader)
    }

    /// Inserts a record into the overlay for the specified table.
    pub fn insert<T>(&mut self, values: Vec<(ColumnDef, Value)>) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let pk = T::primary_key();
        let pk = Self::primary_key(pk, &values)?;
        let overlay = self
            .tables
            .entry(table_name.to_string())
            .or_insert_with(|| TableOverlay::new(T::indexes()));
        overlay.insert(pk, values);

        Ok(())
    }

    /// Updates a record in the overlay for the specified table.
    ///
    /// `current_row` is the full row before the update, used to track old indexed values.
    pub fn update<T>(
        &mut self,
        pk: Value,
        updates: Vec<(&'static str, Value)>,
        current_row: &[(ColumnDef, Value)],
    ) where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let overlay = self
            .tables
            .entry(table_name.to_string())
            .or_insert_with(|| TableOverlay::new(T::indexes()));
        overlay.update(pk, updates, current_row);
    }

    /// Deletes a record in the overlay for the specified table.
    ///
    /// `current_row` is the full row being deleted, used to track removed indexed values.
    pub fn delete<T>(&mut self, pk: Value, current_row: &[(ColumnDef, Value)])
    where
        T: TableSchema,
    {
        let table_name = T::table_name();
        let overlay = self
            .tables
            .entry(table_name.to_string())
            .or_insert_with(|| TableOverlay::new(T::indexes()));
        overlay.delete(pk, current_row);
    }

    /// Retrieves the index overlay for a given table, if it exists.
    pub fn index_overlay(&self, table: &str) -> Option<&IndexOverlay> {
        self.tables.get(table).map(|t| &t.index_overlay)
    }

    /// Retrieves the table overlay for a given table, if it exists.
    pub(crate) fn table_overlay(&self, table: &str) -> Option<&TableOverlay> {
        self.tables.get(table)
    }

    fn primary_key(pk: &'static str, values: &[(ColumnDef, Value)]) -> DbmsResult<Value> {
        for (col_def, value) in values {
            if col_def.name == pk {
                return Ok(value.clone());
            }
        }
        Err(DbmsError::Query(QueryError::MissingNonNullableField(
            pk.to_string(),
        )))
    }
}

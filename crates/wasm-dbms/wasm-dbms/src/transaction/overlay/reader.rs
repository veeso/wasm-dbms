// Rust guideline compliant 2026-03-01

//! Overlay reader that merges base table data with overlay changes.

use wasm_dbms_api::prelude::{ColumnDef, DbmsResult, TableSchema, Value};
use wasm_dbms_memory::prelude::{MemoryProvider, TableReader};

use super::table::TableOverlay;

/// A reader that merges base table data with overlay changes.
pub struct DatabaseOverlayReader<'a, T, P>
where
    T: TableSchema,
    P: MemoryProvider,
{
    /// Pre-collected inserted rows from the overlay.
    inserted_rows: Vec<Vec<(ColumnDef, Value)>>,
    /// Track the position in the inserted rows.
    new_rows_cursor: usize,
    /// Reference to the table overlay.
    table_overlay: &'a TableOverlay,
    /// The underlying table reader.
    table_reader: TableReader<'a, T, P>,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T, P> DatabaseOverlayReader<'a, T, P>
where
    T: TableSchema,
    P: MemoryProvider,
{
    /// Creates a new overlay reader.
    pub fn new(table_overlay: &'a TableOverlay, table_reader: TableReader<'a, T, P>) -> Self {
        let inserted_rows: Vec<_> = table_overlay.iter_inserted().collect();
        Self {
            inserted_rows,
            new_rows_cursor: 0,
            table_overlay,
            table_reader,
            _marker: std::marker::PhantomData,
        }
    }

    /// Attempts to get the next row, applying overlay changes.
    pub fn try_next(&mut self) -> DbmsResult<Option<Vec<(ColumnDef, Value)>>> {
        loop {
            let next_base_row = self
                .table_reader
                .try_next()?
                .map(|row| row.record.to_values());

            let Some(next_row) = next_base_row.or_else(|| self.next_overlay_row()) else {
                return Ok(None);
            };

            // NOTE: None from patch_row means deleted, not end-of-stream
            if let Some(patched) = self.table_overlay.patch_row(next_row) {
                return Ok(Some(patched));
            }
        }
    }

    /// Gets the next row from the pre-collected inserted records.
    fn next_overlay_row(&mut self) -> Option<Vec<(ColumnDef, Value)>> {
        let row = self.inserted_rows.get(self.new_rows_cursor)?.clone();
        self.new_rows_cursor += 1;
        Some(row)
    }
}

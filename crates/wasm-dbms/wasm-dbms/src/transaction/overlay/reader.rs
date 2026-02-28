// Rust guideline compliant 2026-02-28

//! Overlay reader that merges base table data with overlay changes.

use wasm_dbms_api::prelude::{ColumnDef, DbmsResult, TableSchema, Value};

use super::table::TableOverlay;
use wasm_dbms_memory::prelude::{MemoryProvider, TableReader};

/// A reader that merges base table data with overlay changes.
pub struct DatabaseOverlayReader<'a, T, P>
where
    T: TableSchema,
    P: MemoryProvider,
{
    /// Track the position in the new rows.
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
        Self {
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

    /// Get the next row from the overlay's inserted records.
    fn next_overlay_row(&mut self) -> Option<Vec<(ColumnDef, Value)>> {
        let row_to_get = self.new_rows_cursor;
        self.new_rows_cursor += 1;
        self.table_overlay.iter_inserted().nth(row_to_get)
    }
}

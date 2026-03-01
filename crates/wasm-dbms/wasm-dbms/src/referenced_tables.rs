// Rust guideline compliant 2026-03-01

//! Foreign key reference tracking.
//!
//! Identifies which tables reference a given target table via foreign keys.

use wasm_dbms_api::prelude::ColumnDef;

/// Returns the list of tables that reference the target table.
pub fn get_referenced_tables(
    target: &'static str,
    tables: &[(&'static str, &'static [ColumnDef])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    tables
        .iter()
        .filter_map(|(table_name, columns)| {
            let fk_columns: Vec<&'static str> = columns
                .iter()
                .filter_map(|col| col.foreign_key.as_ref())
                .filter(|fk| fk.foreign_table == target)
                .map(|fk| fk.local_column)
                .collect();

            if fk_columns.is_empty() {
                None
            } else {
                Some((*table_name, fk_columns))
            }
        })
        .collect()
}

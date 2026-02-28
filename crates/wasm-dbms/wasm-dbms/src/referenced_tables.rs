// Rust guideline compliant 2026-02-28

//! Foreign key reference tracking.
//!
//! Identifies which tables reference a given target table via foreign keys.

use wasm_dbms_api::prelude::ColumnDef;

type CacheMap = std::collections::HashMap<&'static str, Vec<(&'static str, Vec<&'static str>)>>;

thread_local! {
    /// Cache for referenced tables results.
    static CACHED_REFERENCED_TABLES: std::cell::RefCell<CacheMap> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// Returns the list of tables that reference the target table.
///
/// Results are cached per target table for performance.
pub fn get_referenced_tables(
    target: &'static str,
    tables: &[(&'static str, &'static [ColumnDef])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    if let Some(cached) = CACHED_REFERENCED_TABLES.with_borrow(|cache| cache.get(target).cloned()) {
        return cached;
    }

    let referenced_tables = compute_referenced_tables(target, tables);
    CACHED_REFERENCED_TABLES.with_borrow_mut(|cache| {
        cache.insert(target, referenced_tables.clone());
    });
    referenced_tables
}

fn compute_referenced_tables(
    target: &'static str,
    tables: &[(&'static str, &'static [ColumnDef])],
) -> Vec<(&'static str, Vec<&'static str>)> {
    let mut referenced_tables = vec![];
    for (table_name, columns) in tables {
        let mut referenced_tables_columns = vec![];
        for fk in columns
            .iter()
            .filter_map(|col| col.foreign_key.as_ref())
            .filter(|fk| fk.foreign_table == target)
        {
            referenced_tables_columns.push(fk.local_column);
        }
        if !referenced_tables_columns.is_empty() {
            referenced_tables.push((*table_name, referenced_tables_columns));
        }
    }

    referenced_tables
}

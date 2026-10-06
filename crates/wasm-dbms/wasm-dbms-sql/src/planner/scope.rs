//! Tables visible to a statement, and column name resolution against them.

use wasm_dbms_api::prelude::{ColumnDef, SqlError};

use super::Catalog;
use crate::ast::{ColumnRef, TableRef};

/// A table of a `FROM` or `JOIN` clause.
struct ScopeTable {
    /// The table name in the schema.
    name: String,
    /// The name that qualifies its columns: the alias, or the table name.
    qualifier: String,
    columns: &'static [ColumnDef],
}

/// The tables a statement can reference, in clause order.
#[derive(Default)]
pub(super) struct Scope {
    tables: Vec<ScopeTable>,
}

/// A column reference resolved to a table of the scope.
#[derive(Debug, Clone, Copy)]
pub(super) struct Resolved {
    /// Index of the table in the scope.
    pub(super) table: usize,
    pub(super) def: ColumnDef,
}

impl Scope {
    /// Creates the scope of a statement that works on one table.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::UnknownTable`] when the schema has no such table.
    pub(super) fn single(catalog: &Catalog<'_>, table: &str) -> Result<Self, SqlError> {
        let mut scope = Self::default();
        scope.push(
            catalog,
            &TableRef {
                name: table.to_string(),
                alias: None,
            },
        )?;
        Ok(scope)
    }

    /// Adds a table to the scope and returns its index.
    ///
    /// # Errors
    ///
    /// - [`SqlError::UnknownTable`] when the schema has no such table.
    /// - [`SqlError::Unsupported`] when the table is already in the scope, or
    ///   its alias is.
    pub(super) fn push(
        &mut self,
        catalog: &Catalog<'_>,
        table: &TableRef,
    ) -> Result<usize, SqlError> {
        let columns =
            catalog(&table.name).ok_or_else(|| SqlError::UnknownTable(table.name.clone()))?;
        let qualifier = table.alias.as_ref().unwrap_or(&table.name);
        if self.tables.iter().any(|known| known.name == table.name) {
            return Err(SqlError::Unsupported(format!(
                "table `{name}` appears more than once; self-joins are not supported",
                name = table.name
            )));
        }
        if self
            .tables
            .iter()
            .any(|known| known.qualifier == *qualifier)
        {
            return Err(SqlError::Unsupported(format!(
                "table alias `{qualifier}` is used more than once"
            )));
        }
        self.tables.push(ScopeTable {
            name: table.name.clone(),
            qualifier: qualifier.clone(),
            columns,
        });
        Ok(self.tables.len() - 1)
    }

    /// Returns the schema name of the table at `index`.
    pub(super) fn table_name(&self, index: usize) -> &str {
        &self.tables[index].name
    }

    /// Returns the columns of the table at `index`.
    pub(super) fn columns(&self, index: usize) -> &'static [ColumnDef] {
        self.tables[index].columns
    }

    /// Returns `table.column`, with the schema name of the table.
    pub(super) fn qualified_name(&self, resolved: &Resolved) -> String {
        format!(
            "{table}.{column}",
            table = self.table_name(resolved.table),
            column = resolved.def.name
        )
    }

    /// Finds the table and definition of a column reference.
    ///
    /// # Errors
    ///
    /// - [`SqlError::UnknownTable`] when the qualifier names no table of the scope.
    /// - [`SqlError::UnknownColumn`] when no table has the column.
    /// - [`SqlError::AmbiguousColumn`] when an unqualified name exists on
    ///   several tables.
    pub(super) fn resolve(&self, column: &ColumnRef) -> Result<Resolved, SqlError> {
        let find = |table: &ScopeTable| {
            table
                .columns
                .iter()
                .find(|def| def.name == column.column)
                .copied()
        };
        let unknown = |table: &str| SqlError::UnknownColumn {
            table: table.to_string(),
            column: column.column.clone(),
        };

        if let Some(qualifier) = &column.table {
            let table = self
                .tables
                .iter()
                .position(|table| table.qualifier == *qualifier)
                .ok_or_else(|| SqlError::UnknownTable(qualifier.clone()))?;
            let def = find(&self.tables[table]).ok_or_else(|| unknown(self.table_name(table)))?;
            return Ok(Resolved { table, def });
        }

        let mut matches = self
            .tables
            .iter()
            .enumerate()
            .filter_map(|(table, candidate)| find(candidate).map(|def| Resolved { table, def }));
        match (matches.next(), matches.next()) {
            (Some(resolved), None) => Ok(resolved),
            (Some(_), Some(_)) => Err(SqlError::AmbiguousColumn(column.column.clone())),
            (None, _) => {
                let tables = self
                    .tables
                    .iter()
                    .map(|table| table.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" or ");
                Err(unknown(&tables))
            }
        }
    }
}

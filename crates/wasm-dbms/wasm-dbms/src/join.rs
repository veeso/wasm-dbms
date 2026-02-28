// Rust guideline compliant 2026-02-28

//! Join execution engine for cross-table queries.

use wasm_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, DbmsResult, JoinType, OrderDirection, Query, Value,
};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::database::WasmDbmsDatabase;
use crate::schema::DatabaseSchema;

/// A row in the joined result, organized by source table.
type JoinedRow = Vec<(String, Vec<(ColumnDef, Value)>)>;

/// Engine that executes join queries using nested-loop join.
pub struct JoinEngine<'a, Schema: ?Sized, M: MemoryProvider>
where
    Schema: DatabaseSchema<M>,
{
    schema: &'a Schema,
    _marker: std::marker::PhantomData<M>,
}

impl<'a, Schema: ?Sized, M: MemoryProvider> JoinEngine<'a, Schema, M>
where
    Schema: DatabaseSchema<M>,
{
    pub fn new(schema: &'a Schema) -> Self {
        Self {
            schema,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Schema: ?Sized, M: MemoryProvider> JoinEngine<'_, Schema, M>
where
    Schema: DatabaseSchema<M>,
{
    /// Executes a join query using nested-loop join.
    pub fn join(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        from_table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
        let from_rows = self
            .schema
            .select(dbms, from_table, Query::builder().all().build())?;

        let mut joined_rows: Vec<JoinedRow> = from_rows
            .into_iter()
            .map(|row| vec![(from_table.to_string(), row)])
            .collect();

        for join in &query.joins {
            let right_rows =
                self.schema
                    .select(dbms, &join.table, Query::builder().all().build())?;

            let (left_table, left_col) = self.resolve_column_ref(&join.left_column, from_table);
            let (_right_table_ref, right_col) =
                self.resolve_column_ref(&join.right_column, &join.table);

            let (keep_unmatched_left, keep_unmatched_right) = match join.join_type {
                JoinType::Inner => (false, false),
                JoinType::Left => (true, false),
                JoinType::Right => (false, true),
                JoinType::Full => (true, true),
            };

            joined_rows = self.nested_loop_join(
                joined_rows,
                &right_rows,
                &join.table,
                &left_table,
                left_col,
                right_col,
                keep_unmatched_left,
                keep_unmatched_right,
            );
        }

        if let Some(filter) = &query.filter {
            joined_rows.retain(|row| {
                let groups: Vec<(&str, Vec<(ColumnDef, Value)>)> = row
                    .iter()
                    .map(|(t, cols)| (t.as_str(), cols.clone()))
                    .collect();
                filter.matches_joined_row(&groups).unwrap_or(false)
            });
        }

        for (column, direction) in query.order_by.iter().rev() {
            self.sort_joined_rows(&mut joined_rows, column, *direction);
        }

        let offset = query.offset.unwrap_or_default();
        if offset > 0 {
            if offset >= joined_rows.len() {
                joined_rows.clear();
            } else {
                joined_rows = joined_rows.into_iter().skip(offset).collect();
            }
        }

        if let Some(limit) = query.limit {
            joined_rows.truncate(limit);
        }

        let results = joined_rows
            .into_iter()
            .map(|row| self.flatten_joined_row(row, &query))
            .collect::<DbmsResult<Vec<_>>>()?;

        Ok(results)
    }

    /// Unified nested-loop join.
    #[allow(clippy::too_many_arguments)]
    fn nested_loop_join(
        &self,
        left_rows: Vec<JoinedRow>,
        right_rows: &[Vec<(ColumnDef, Value)>],
        right_table: &str,
        left_table: &str,
        left_col: &str,
        right_col: &str,
        keep_unmatched_left: bool,
        keep_unmatched_right: bool,
    ) -> Vec<JoinedRow> {
        let mut results = Vec::new();
        let mut right_matched = vec![false; right_rows.len()];

        for left_row in &left_rows {
            let left_value = self.get_column_value(left_row, left_table, left_col);
            let mut matched = false;

            for (i, right_row) in right_rows.iter().enumerate() {
                let right_value = right_row
                    .iter()
                    .find(|(c, _)| c.name == right_col)
                    .map(|(_, v)| v);

                if left_value == right_value && left_value.is_some() {
                    let mut new_row = left_row.clone();
                    new_row.push((right_table.to_string(), right_row.clone()));
                    results.push(new_row);
                    right_matched[i] = true;
                    matched = true;
                }
            }

            if keep_unmatched_left && !matched {
                let mut new_row = left_row.clone();
                let null_cols = right_rows
                    .first()
                    .map(|sample| self.null_pad_columns(sample))
                    .unwrap_or_default();
                new_row.push((right_table.to_string(), null_cols));
                results.push(new_row);
            }
        }

        if keep_unmatched_right {
            for (i, right_row) in right_rows.iter().enumerate() {
                if !right_matched[i] {
                    let mut new_row: JoinedRow = Vec::new();
                    if let Some(sample_left) = left_rows.first() {
                        for (table_name, cols) in sample_left {
                            new_row.push((table_name.clone(), self.null_pad_columns(cols)));
                        }
                    }
                    new_row.push((right_table.to_string(), right_row.clone()));
                    results.push(new_row);
                }
            }
        }

        results
    }

    /// Resolves a column reference to (table_name, column_name).
    fn resolve_column_ref<'a>(&self, field: &'a str, default_table: &'a str) -> (String, &'a str) {
        if let Some((table, column)) = field.split_once('.') {
            (table.to_string(), column)
        } else {
            (default_table.to_string(), field)
        }
    }

    /// Finds a column value in a joined row.
    fn get_column_value<'a>(
        &self,
        row: &'a JoinedRow,
        table: &str,
        column: &str,
    ) -> Option<&'a Value> {
        row.iter()
            .find(|(t, _)| t == table)
            .and_then(|(_, cols)| cols.iter().find(|(c, _)| c.name == column).map(|(_, v)| v))
    }

    /// Creates a NULL-padded row.
    fn null_pad_columns(&self, sample_row: &[(ColumnDef, Value)]) -> Vec<(ColumnDef, Value)> {
        sample_row
            .iter()
            .map(|(col, _)| (*col, Value::Null))
            .collect()
    }

    /// Sorts joined rows by a column.
    fn sort_joined_rows(&self, rows: &mut [JoinedRow], column: &str, direction: OrderDirection) {
        let (table, col) = if let Some((t, c)) = column.split_once('.') {
            (Some(t), c)
        } else {
            (None, column)
        };

        rows.sort_by(|a, b| {
            let a_val = self.find_value_in_joined_row(a, table, col);
            let b_val = self.find_value_in_joined_row(b, table, col);

            WasmDbmsDatabase::<M>::sort_values_with_direction(a_val, b_val, direction)
        });
    }

    /// Finds a column value in a joined row, optionally scoped to a table.
    fn find_value_in_joined_row<'a>(
        &self,
        row: &'a JoinedRow,
        table: Option<&str>,
        column: &str,
    ) -> Option<&'a Value> {
        if let Some(table) = table {
            return self.get_column_value(row, table, column);
        }
        row.iter()
            .flat_map(|(_, cols)| cols)
            .find_map(|(col, value)| if col.name == column { Some(value) } else { None })
    }

    /// Flattens a joined row into the output format.
    fn flatten_joined_row(
        &self,
        row: JoinedRow,
        query: &Query,
    ) -> DbmsResult<Vec<(CandidColumnDef, Value)>> {
        let mut result = Vec::new();

        for (table_name, cols) in row {
            for (col, val) in cols {
                let mut candid_col = CandidColumnDef::from(col);
                candid_col.table = Some(table_name.clone());

                if !query.all_selected() {
                    let selected = query.raw_columns();
                    let qualified_name = format!("{table_name}.{col}", col = candid_col.name);
                    if !selected.contains(&candid_col.name) && !selected.contains(&qualified_name) {
                        continue;
                    }
                }

                result.push((candid_col, val));
            }
        }

        Ok(result)
    }
}

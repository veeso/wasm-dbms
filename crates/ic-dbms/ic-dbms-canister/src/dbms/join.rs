//! Join execution engine for cross-table queries.

use ic_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, IcDbmsResult, JoinType, OrderDirection, Query, Value,
};

use crate::dbms::IcDbmsDatabase;
use crate::dbms::schema::DatabaseSchema;

/// A row in the joined result, organized by source table.
/// Each element is (table_name, columns_from_that_table).
type JoinedRow = Vec<(String, Vec<(ColumnDef, Value)>)>;

/// Engine that executes join queries using nested-loop join.
pub struct JoinEngine<'a, Schema: ?Sized>
where
    Schema: DatabaseSchema,
{
    schema: &'a Schema,
}

impl<'a, Schema: ?Sized> JoinEngine<'a, Schema>
where
    Schema: DatabaseSchema,
{
    pub fn new(schema: &'a Schema) -> Self {
        Self { schema }
    }
}

impl<Schema: ?Sized> JoinEngine<'_, Schema>
where
    Schema: DatabaseSchema,
{
    /// Executes a join query using nested-loop join.
    ///
    /// Reads all rows from the FROM table and each joined table,
    /// performs the join, applies filters, column selection,
    /// ordering, offset, and limit.
    pub fn join(
        &self,
        dbms: &IcDbmsDatabase,
        from_table: &str,
        query: Query,
    ) -> IcDbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
        // 1. Read all rows from the FROM table (no filter/limit/joins).
        let from_rows = self
            .schema
            .select(dbms, from_table, Query::builder().all().build())?;

        let mut joined_rows: Vec<JoinedRow> = from_rows
            .into_iter()
            .map(|row| vec![(from_table.to_string(), row)])
            .collect();

        // 2. Process each join left-to-right.
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

        // 3. Apply filter on the combined rows.
        if let Some(filter) = &query.filter {
            joined_rows.retain(|row| {
                let groups: Vec<(&str, Vec<(ColumnDef, Value)>)> = row
                    .iter()
                    .map(|(t, cols)| (t.as_str(), cols.clone()))
                    .collect();
                filter.matches_joined_row(&groups).unwrap_or(false)
            });
        }

        // 4. Apply ordering (in reverse so primary sort key is applied last via stable sort).
        for (column, direction) in query.order_by.iter().rev() {
            self.sort_joined_rows(&mut joined_rows, column, *direction);
        }

        // 5. Apply offset.
        let offset = query.offset.unwrap_or_default();
        if offset > 0 {
            if offset >= joined_rows.len() {
                joined_rows.clear();
            } else {
                joined_rows = joined_rows.into_iter().skip(offset).collect();
            }
        }

        // 6. Apply limit.
        if let Some(limit) = query.limit {
            joined_rows.truncate(limit);
        }

        // 7. Convert to output format.
        let results = joined_rows
            .into_iter()
            .map(|row| self.flatten_joined_row(row, &query))
            .collect::<IcDbmsResult<Vec<_>>>()?;

        Ok(results)
    }

    /// Unified nested-loop join that handles all join types via two flags.
    ///
    /// - `keep_unmatched_left`: if true, left rows with no match produce a row with NULL-padded right columns.
    /// - `keep_unmatched_right`: if true, right rows with no match produce a row with NULL-padded left columns.
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
    ///
    /// Unqualified names default to the given default table.
    fn resolve_column_ref<'a>(&self, field: &'a str, default_table: &'a str) -> (String, &'a str) {
        if let Some((table, column)) = field.split_once('.') {
            (table.to_string(), column)
        } else {
            (default_table.to_string(), field)
        }
    }

    /// Finds the value of a column in a [`JoinedRow`] by table name and column name.
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

    /// Creates a NULL-padded row for a table's columns.
    fn null_pad_columns(&self, sample_row: &[(ColumnDef, Value)]) -> Vec<(ColumnDef, Value)> {
        sample_row
            .iter()
            .map(|(col, _)| (*col, Value::Null))
            .collect()
    }

    /// Sorts joined rows by a column (supports qualified "table.column" names).
    fn sort_joined_rows(&self, rows: &mut [JoinedRow], column: &str, direction: OrderDirection) {
        let (table, col) = if let Some((t, c)) = column.split_once('.') {
            (Some(t), c)
        } else {
            (None, column)
        };

        rows.sort_by(|a, b| {
            let a_val = self.find_value_in_joined_row(a, table, col);
            let b_val = self.find_value_in_joined_row(b, table, col);

            IcDbmsDatabase::sort_values_with_direction(a_val, b_val, direction)
        });
    }

    /// Finds a column value in a joined row, optionally scoped to a specific table.
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
            .find_map(|(col, value)| {
                if col.name == column {
                    Some(value)
                } else {
                    None
                }
            })
    }

    /// Flattens a joined row into the output format, applying column selection.
    fn flatten_joined_row(
        &self,
        row: JoinedRow,
        query: &Query,
    ) -> IcDbmsResult<Vec<(CandidColumnDef, Value)>> {
        let mut result = Vec::new();

        for (table_name, cols) in row {
            for (col, val) in cols {
                let mut candid_col = CandidColumnDef::from(col);
                candid_col.table = Some(table_name.clone());

                // Apply column selection if not Select::All.
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

#[cfg(test)]
mod tests {
    use ic_dbms_api::prelude::{DataTypeKind, OrderDirection};

    use super::*;
    use crate::tests::TestDatabaseSchema;

    /// Creates a simple [`ColumnDef`] with the given name and [`DataTypeKind::Uint32`].
    fn col(name: &'static str) -> ColumnDef {
        ColumnDef {
            name,
            data_type: DataTypeKind::Uint32,
            nullable: false,
            primary_key: false,
            foreign_key: None,
        }
    }

    /// Creates a [`JoinedRow`] with a single table group.
    fn joined_row(table: &str, cols: Vec<(ColumnDef, Value)>) -> JoinedRow {
        vec![(table.to_string(), cols)]
    }

    /// Builds the [`JoinEngine`] backed by [`TestDatabaseSchema`].
    fn engine() -> JoinEngine<'static, TestDatabaseSchema> {
        JoinEngine::new(&TestDatabaseSchema)
    }

    // -- resolve_column_ref --------------------------------------------------

    #[test]
    fn test_resolve_column_ref_qualified() {
        let engine = engine();
        let (table, column) = engine.resolve_column_ref("users.name", "default_table");
        assert_eq!(table, "users");
        assert_eq!(column, "name");
    }

    #[test]
    fn test_resolve_column_ref_unqualified() {
        let engine = engine();
        let (table, column) = engine.resolve_column_ref("name", "default_table");
        assert_eq!(table, "default_table");
        assert_eq!(column, "name");
    }

    // -- get_column_value ----------------------------------------------------

    #[test]
    fn test_get_column_value_found() {
        let engine = engine();
        let row = joined_row("t", vec![(col("id"), Value::Uint32(42.into()))]);
        let val = engine.get_column_value(&row, "t", "id");
        assert_eq!(val, Some(&Value::Uint32(42.into())));
    }

    #[test]
    fn test_get_column_value_wrong_table() {
        let engine = engine();
        let row = joined_row("t", vec![(col("id"), Value::Uint32(42.into()))]);
        assert!(engine.get_column_value(&row, "other", "id").is_none());
    }

    #[test]
    fn test_get_column_value_wrong_column() {
        let engine = engine();
        let row = joined_row("t", vec![(col("id"), Value::Uint32(42.into()))]);
        assert!(engine.get_column_value(&row, "t", "name").is_none());
    }

    // -- null_pad_columns ----------------------------------------------------

    #[test]
    fn test_null_pad_columns() {
        let engine = engine();
        let sample = vec![
            (col("id"), Value::Uint32(1.into())),
            (col("name"), Value::Text("Alice".into())),
        ];
        let padded = engine.null_pad_columns(&sample);
        assert_eq!(padded.len(), 2);
        assert_eq!(padded[0].0.name, "id");
        assert_eq!(padded[0].1, Value::Null);
        assert_eq!(padded[1].0.name, "name");
        assert_eq!(padded[1].1, Value::Null);
    }

    // -- find_value_in_joined_row --------------------------------------------

    #[test]
    fn test_find_value_in_joined_row_with_table() {
        let engine = engine();
        let mut row = joined_row("a", vec![(col("id"), Value::Uint32(1.into()))]);
        row.push(("b".to_string(), vec![(col("id"), Value::Uint32(2.into()))]));

        // Scoped to table "b".
        let val = engine.find_value_in_joined_row(&row, Some("b"), "id");
        assert_eq!(val, Some(&Value::Uint32(2.into())));
    }

    #[test]
    fn test_find_value_in_joined_row_without_table() {
        let engine = engine();
        let mut row = joined_row("a", vec![(col("x"), Value::Uint32(1.into()))]);
        row.push(("b".to_string(), vec![(col("y"), Value::Uint32(2.into()))]));

        // Unscoped: finds the first match across all tables.
        let val = engine.find_value_in_joined_row(&row, None, "y");
        assert_eq!(val, Some(&Value::Uint32(2.into())));
    }

    #[test]
    fn test_find_value_in_joined_row_missing() {
        let engine = engine();
        let row = joined_row("a", vec![(col("id"), Value::Uint32(1.into()))]);
        assert!(
            engine
                .find_value_in_joined_row(&row, None, "missing")
                .is_none()
        );
    }

    // -- nested_loop_join ----------------------------------------------------

    /// Builds left/right fixture data for nested-loop join tests.
    ///
    /// Left: two rows in table "L" with ids 1, 2.
    /// Right: two rows in table "R" with fk values 1, 3 (only id=1 matches).
    fn build_join_fixtures() -> (Vec<JoinedRow>, Vec<Vec<(ColumnDef, Value)>>) {
        let left = vec![
            joined_row("L", vec![(col("id"), Value::Uint32(1.into()))]),
            joined_row("L", vec![(col("id"), Value::Uint32(2.into()))]),
        ];
        let right = vec![
            vec![
                (col("fk"), Value::Uint32(1.into())),
                (col("val"), Value::Text("r1".into())),
            ],
            vec![
                (col("fk"), Value::Uint32(3.into())),
                (col("val"), Value::Text("r2".into())),
            ],
        ];
        (left, right)
    }

    #[test]
    fn test_nested_loop_inner_join() {
        let engine = engine();
        let (left, right) = build_join_fixtures();

        // INNER: only matching rows (L.id=1 matches R.fk=1).
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, false);
        assert_eq!(result.len(), 1);

        // Matched row must contain both L and R groups.
        assert_eq!(result[0].len(), 2);
        assert_eq!(result[0][0].0, "L");
        assert_eq!(result[0][1].0, "R");
    }

    #[test]
    fn test_nested_loop_left_join() {
        let engine = engine();
        let (left, right) = build_join_fixtures();

        // LEFT: unmatched left rows are kept with NULL-padded right.
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", true, false);
        assert_eq!(result.len(), 2);

        // Second row (L.id=2, no match) should have NULL right columns.
        let unmatched = &result[1];
        let right_group = &unmatched[1];
        assert_eq!(right_group.0, "R");
        assert!(right_group.1.iter().all(|(_, v)| *v == Value::Null));
    }

    #[test]
    fn test_nested_loop_right_join() {
        let engine = engine();
        let (left, right) = build_join_fixtures();

        // RIGHT: unmatched right rows are kept with NULL-padded left.
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, true);
        assert_eq!(result.len(), 2);

        // Second row (R.fk=3, no match) should have NULL left columns.
        let unmatched = &result[1];
        let left_group = &unmatched[0];
        assert_eq!(left_group.0, "L");
        assert!(left_group.1.iter().all(|(_, v)| *v == Value::Null));
    }

    #[test]
    fn test_nested_loop_full_join() {
        let engine = engine();
        let (left, right) = build_join_fixtures();

        // FULL: both unmatched sides are preserved.
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", true, true);
        // 1 match + 1 unmatched left + 1 unmatched right = 3.
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_nested_loop_inner_join_empty_right() {
        let engine = engine();
        let left = vec![joined_row("L", vec![(col("id"), Value::Uint32(1.into()))])];
        let right: Vec<Vec<(ColumnDef, Value)>> = vec![];

        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_nested_loop_inner_join_empty_left() {
        let engine = engine();
        let left: Vec<JoinedRow> = vec![];
        let right = vec![vec![(col("fk"), Value::Uint32(1.into()))]];

        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_nested_loop_left_join_empty_right() {
        let engine = engine();
        let left = vec![joined_row("L", vec![(col("id"), Value::Uint32(1.into()))])];
        let right: Vec<Vec<(ColumnDef, Value)>> = vec![];

        // LEFT with empty right: left row is kept, right group has no columns.
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", true, false);
        assert_eq!(result.len(), 1);
        let right_group = &result[0][1];
        assert_eq!(right_group.0, "R");
        assert!(right_group.1.is_empty());
    }

    #[test]
    fn test_nested_loop_right_join_empty_left() {
        let engine = engine();
        let left: Vec<JoinedRow> = vec![];
        let right = vec![vec![
            (col("fk"), Value::Uint32(1.into())),
            (col("val"), Value::Text("r1".into())),
        ]];

        // RIGHT with empty left: right row is kept with no left table groups.
        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, true);
        assert_eq!(result.len(), 1);
        // Only the right group is present (no left sample to null-pad).
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].0, "R");
    }

    #[test]
    fn test_nested_loop_join_both_values_none() {
        let engine = engine();
        // Left row has column "id" but left_col is "missing" so left_value = None.
        // Right row has column "fk" matching right_col but values differ.
        // Both None: should NOT match because left_value.is_some() is false.
        let left = vec![joined_row("L", vec![(col("id"), Value::Uint32(1.into()))])];
        let right = vec![vec![(col("fk"), Value::Uint32(1.into()))]];

        let result = engine.nested_loop_join(left, &right, "R", "L", "missing", "fk", false, false);
        assert!(result.is_empty());
    }

    #[test]
    fn test_nested_loop_one_to_many() {
        let engine = engine();
        // One left row matches two right rows.
        let left = vec![joined_row("L", vec![(col("id"), Value::Uint32(1.into()))])];
        let right = vec![
            vec![(col("fk"), Value::Uint32(1.into()))],
            vec![(col("fk"), Value::Uint32(1.into()))],
        ];

        let result = engine.nested_loop_join(left, &right, "R", "L", "id", "fk", false, false);
        assert_eq!(result.len(), 2);
    }

    // -- sort_joined_rows ----------------------------------------------------

    #[test]
    fn test_sort_joined_rows_ascending() {
        let engine = engine();
        let mut rows = vec![
            joined_row("t", vec![(col("id"), Value::Uint32(3.into()))]),
            joined_row("t", vec![(col("id"), Value::Uint32(1.into()))]),
            joined_row("t", vec![(col("id"), Value::Uint32(2.into()))]),
        ];

        engine.sort_joined_rows(&mut rows, "id", OrderDirection::Ascending);

        let ids: Vec<_> = rows
            .iter()
            .map(|r| engine.get_column_value(r, "t", "id").cloned())
            .collect();
        assert_eq!(
            ids,
            vec![
                Some(Value::Uint32(1.into())),
                Some(Value::Uint32(2.into())),
                Some(Value::Uint32(3.into())),
            ]
        );
    }

    #[test]
    fn test_sort_joined_rows_descending() {
        let engine = engine();
        let mut rows = vec![
            joined_row("t", vec![(col("id"), Value::Uint32(1.into()))]),
            joined_row("t", vec![(col("id"), Value::Uint32(3.into()))]),
            joined_row("t", vec![(col("id"), Value::Uint32(2.into()))]),
        ];

        engine.sort_joined_rows(&mut rows, "id", OrderDirection::Descending);

        let ids: Vec<_> = rows
            .iter()
            .map(|r| engine.get_column_value(r, "t", "id").cloned())
            .collect();
        assert_eq!(
            ids,
            vec![
                Some(Value::Uint32(3.into())),
                Some(Value::Uint32(2.into())),
                Some(Value::Uint32(1.into())),
            ]
        );
    }

    #[test]
    fn test_sort_joined_rows_qualified_column() {
        let engine = engine();
        let mut rows = vec![
            joined_row("t", vec![(col("id"), Value::Uint32(2.into()))]),
            joined_row("t", vec![(col("id"), Value::Uint32(1.into()))]),
        ];

        engine.sort_joined_rows(&mut rows, "t.id", OrderDirection::Ascending);

        let first = engine.get_column_value(&rows[0], "t", "id");
        assert_eq!(first, Some(&Value::Uint32(1.into())));
    }

    #[test]
    fn test_sort_joined_rows_unqualified_across_tables() {
        let engine = engine();
        // Each row has two table groups; sort by unqualified "val" which lives in "b".
        let mut rows = vec![
            {
                let mut r = joined_row("a", vec![(col("id"), Value::Uint32(1.into()))]);
                r.push(("b".to_string(), vec![(col("val"), Value::Uint32(3.into()))]));
                r
            },
            {
                let mut r = joined_row("a", vec![(col("id"), Value::Uint32(2.into()))]);
                r.push(("b".to_string(), vec![(col("val"), Value::Uint32(1.into()))]));
                r
            },
        ];

        engine.sort_joined_rows(&mut rows, "val", OrderDirection::Ascending);

        // Row with val=1 should come first.
        let first_val = engine.find_value_in_joined_row(&rows[0], None, "val");
        assert_eq!(first_val, Some(&Value::Uint32(1.into())));
    }

    // -- flatten_joined_row --------------------------------------------------

    #[test]
    fn test_flatten_joined_row_select_all() {
        let engine = engine();
        let row: JoinedRow = vec![
            ("a".to_string(), vec![(col("id"), Value::Uint32(1.into()))]),
            (
                "b".to_string(),
                vec![(col("name"), Value::Text("x".into()))],
            ),
        ];
        let query = Query::builder().all().build();

        let result = engine.flatten_joined_row(row, &query).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.table.as_deref(), Some("a"));
        assert_eq!(result[0].0.name, "id");
        assert_eq!(result[1].0.table.as_deref(), Some("b"));
        assert_eq!(result[1].0.name, "name");
    }

    #[test]
    fn test_flatten_joined_row_select_specific_columns() {
        let engine = engine();
        let row: JoinedRow = vec![
            (
                "a".to_string(),
                vec![
                    (col("id"), Value::Uint32(1.into())),
                    (col("name"), Value::Text("Alice".into())),
                ],
            ),
            (
                "b".to_string(),
                vec![(col("title"), Value::Text("Post".into()))],
            ),
        ];

        // Only select "a.name" and "title" (unqualified).
        let query = Query::builder().field("a.name").field("title").build();

        let result = engine.flatten_joined_row(row, &query).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.name, "name");
        assert_eq!(result[1].0.name, "title");
    }

    #[test]
    fn test_flatten_joined_row_filters_out_unselected() {
        let engine = engine();
        let row: JoinedRow = vec![(
            "a".to_string(),
            vec![
                (col("id"), Value::Uint32(1.into())),
                (col("name"), Value::Text("Alice".into())),
            ],
        )];

        // Only select "id"; "name" must be excluded.
        let query = Query::builder().field("id").build();

        let result = engine.flatten_joined_row(row, &query).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0.name, "id");
    }

    #[test]
    fn test_flatten_joined_row_empty() {
        let engine = engine();
        let row: JoinedRow = vec![];
        let query = Query::builder().all().build();

        let result = engine.flatten_joined_row(row, &query).unwrap();
        assert!(result.is_empty());
    }
}

//! Join execution engine for cross-table queries.

use std::collections::{HashMap, HashSet};

use wasm_dbms_api::prelude::{
    ColumnDef, DbmsResult, Filter, JoinColumnDef, JoinResultSet, JoinType, OrderDirection, Query,
    Value,
};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::database::{MAX_ALTERNATIVES, WasmDbmsDatabase};
use crate::schema::DatabaseSchema;

/// A materialized row of one table: its column definitions and values.
type TableRow = Vec<(ColumnDef, Value)>;

/// A joined row before materialization.
///
/// One entry per [`JoinSide`], in side order, holding the index of that
/// side's row or `None` when the side is NULL-padded by an outer join.
type RowRef = Vec<Option<usize>>;

/// Position of a join key: `(side index, column index)`.
type KeyPosition = (usize, usize);

/// The rows of one table taking part in a join.
struct JoinSide {
    /// Table name as referenced by the query.
    table: String,
    /// Compile-time column definitions, in row order.
    columns: &'static [ColumnDef],
    /// Rows loaded from the table.
    rows: Vec<TableRow>,
    /// A row of `NULL` values with the shape of `columns`, used to pad the
    /// missing side of an outer join even when the table has no rows.
    null_row: TableRow,
}

impl JoinSide {
    fn new(table: &str, columns: &'static [ColumnDef], rows: Vec<TableRow>) -> Self {
        Self {
            table: table.to_string(),
            columns,
            rows,
            null_row: columns
                .iter()
                .map(|column| (*column, Value::Null))
                .collect(),
        }
    }

    /// Returns the row at `index`, or the `NULL` row when the side is padded.
    fn row(&self, index: Option<usize>) -> &[(ColumnDef, Value)] {
        match index {
            Some(index) => &self.rows[index],
            None => &self.null_row,
        }
    }

    /// Returns the position of `column` in this side's rows.
    fn column_position(&self, column: &str) -> Option<usize> {
        self.columns.iter().position(|def| def.name == column)
    }
}

/// Engine that executes join queries using hash joins.
pub struct JoinEngine<'a, Schema: ?Sized, M>
where
    Schema: DatabaseSchema<M>,
    M: MemoryProvider,
{
    schema: &'a Schema,
    _marker: std::marker::PhantomData<M>,
}

impl<'a, Schema: ?Sized, M> JoinEngine<'a, Schema, M>
where
    Schema: DatabaseSchema<M>,
    M: MemoryProvider,
{
    pub fn new(schema: &'a Schema) -> Self {
        Self {
            schema,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Schema: ?Sized, M> JoinEngine<'_, Schema, M>
where
    Schema: DatabaseSchema<M>,
    M: MemoryProvider,
{
    /// Executes a join query.
    ///
    /// Every join clause is processed left to right with a hash join: a hash
    /// table is built over the right rows keyed by the join column and probed
    /// once per accumulated left row. Rows stay as index tuples until filter,
    /// ordering, offset, and limit have run; the column list is built once
    /// and values are cloned once, into [`JoinResultSet::rows`].
    pub fn join(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        from_table: &str,
        query: Query,
    ) -> DbmsResult<JoinResultSet> {
        let mut table_columns: Vec<(String, &'static [ColumnDef])> = vec![(
            from_table.to_string(),
            self.schema.table_columns(from_table)?,
        )];
        for join in &query.joins {
            table_columns.push((join.table.clone(), self.schema.table_columns(&join.table)?));
        }

        if let Some(filter) = &query.filter {
            let groups: Vec<(&str, &[ColumnDef])> = table_columns
                .iter()
                .map(|(table, columns)| (table.as_str(), *columns))
                .collect();
            filter.validate_joined(&groups)?;
        }

        let from_rows = self
            .schema
            .select(dbms, from_table, Query::builder().all().build())?;
        let mut sides: Vec<JoinSide> =
            vec![JoinSide::new(from_table, table_columns[0].1, from_rows)];
        let mut rows: Vec<RowRef> = (0..sides[0].rows.len())
            .map(|index| vec![Some(index)])
            .collect();

        for (join_index, join) in query.joins.iter().enumerate() {
            let (left_table, left_col) = self.resolve_column_ref(&join.left_column, from_table);
            let (_right_table_ref, right_col) =
                self.resolve_column_ref(&join.right_column, &join.table);

            let (keep_unmatched_left, keep_unmatched_right) = match join.join_type {
                JoinType::Inner => (false, false),
                JoinType::Left => (true, false),
                JoinType::Right => (false, true),
                JoinType::Full => (true, true),
            };

            // A left table or column outside the query scope never matches,
            // which keeps the pre-existing semantics instead of erroring.
            let left_key: Option<KeyPosition> = sides
                .iter()
                .position(|side| side.table == left_table)
                .and_then(|side_index| {
                    sides[side_index]
                        .column_position(left_col)
                        .map(|column_index| (side_index, column_index))
                });

            let right_columns = table_columns[join_index + 1].1;
            let right_rows = self.load_join_right_rows(
                dbms,
                &sides,
                &rows,
                left_key,
                &join.table,
                right_col,
                keep_unmatched_right,
            )?;
            let right_side = JoinSide::new(&join.table, right_columns, right_rows);

            rows = self.hash_join(
                &sides,
                &rows,
                left_key,
                &right_side,
                right_col,
                keep_unmatched_left,
                keep_unmatched_right,
            );
            sides.push(right_side);
        }

        if let Some(filter) = &query.filter {
            let mut filtered_rows = Vec::with_capacity(rows.len());
            for row in rows {
                let groups = self.row_groups(&sides, &row);
                // Evaluation errors (ambiguous or out-of-scope columns, invalid
                // operands) are query errors, not non-matching rows.
                if filter.matches_joined_row_ref(&groups)? {
                    filtered_rows.push(row);
                }
            }
            rows = filtered_rows;
        }

        for (column, direction) in query.order_by.iter().rev() {
            self.sort_joined_rows(&sides, &mut rows, column, *direction);
        }

        let offset = query.offset.unwrap_or_default();
        if offset > 0 {
            if offset >= rows.len() {
                rows.clear();
            } else {
                rows.drain(..offset);
            }
        }

        if let Some(limit) = query.limit {
            rows.truncate(limit);
        }

        let selected: Vec<Vec<bool>> = sides
            .iter()
            .map(|side| self.selected_columns(side, &query))
            .collect();
        let columns: Vec<JoinColumnDef> = sides
            .iter()
            .zip(&selected)
            .flat_map(|(side, mask)| {
                side.columns
                    .iter()
                    .zip(mask)
                    .filter(|(_, is_selected)| **is_selected)
                    .map(|(column, _)| {
                        let mut def = JoinColumnDef::from(*column);
                        def.table = Some(side.table.clone());
                        def
                    })
            })
            .collect();
        let width = columns.len();
        let rows = rows
            .into_iter()
            .map(|row| self.materialize_row(&sides, &selected, width, &row))
            .collect();

        Ok(JoinResultSet { columns, rows })
    }

    /// Loads the rows of `right_table` that can take part in the join.
    ///
    /// Right and full joins need every right row. For inner and left joins
    /// the distinct left keys are pushed down as an `IN` filter when the join
    /// column leads an index of the right table and the planner can keep the
    /// list within its alternative limit. Other rows are loaded by physical
    /// scan, omitting NULL keys because they cannot match.
    #[expect(
        clippy::too_many_arguments,
        reason = "arguments are necessary for loading right table rows based on join conditions"
    )]
    fn load_join_right_rows(
        &self,
        dbms: &WasmDbmsDatabase<'_, M>,
        sides: &[JoinSide],
        rows: &[RowRef],
        left_key: Option<KeyPosition>,
        right_table: &str,
        right_col: &str,
        keep_unmatched_right: bool,
    ) -> DbmsResult<Vec<TableRow>> {
        if keep_unmatched_right {
            return self
                .schema
                .select(dbms, right_table, Query::builder().all().build());
        }

        let Some((side_index, column_index)) = left_key else {
            return Ok(Vec::new());
        };
        let side = &sides[side_index];
        let mut seen: HashSet<&Value> = HashSet::new();
        let unique_join_values: Vec<Value> = rows
            .iter()
            .filter_map(|row| row[side_index])
            .map(|row_index| &side.rows[row_index][column_index].1)
            .filter(|value| !value.is_null())
            .filter(|value| seen.insert(*value))
            .cloned()
            .collect();

        if unique_join_values.is_empty() {
            return Ok(Vec::new());
        }

        if !self.should_push_down_join_keys(right_table, right_col, unique_join_values.len()) {
            return self.schema.select(
                dbms,
                right_table,
                Query::builder()
                    .all()
                    .filter(Some(Filter::ne(right_col, Value::Null)))
                    .build(),
            );
        }

        self.schema.select(
            dbms,
            right_table,
            Query::builder()
                .all()
                .filter(Some(Filter::in_list(right_col, unique_join_values)))
                .build(),
        )
    }

    /// Returns whether an `IN` filter can use a leading index without
    /// exceeding the access planner's maximum number of alternatives.
    fn should_push_down_join_keys(&self, table: &str, column: &str, key_count: usize) -> bool {
        key_count <= MAX_ALTERNATIVES && self.is_indexed_column(table, column)
    }

    /// Returns whether `column` leads an index of `table`.
    fn is_indexed_column(&self, table: &str, column: &str) -> bool {
        self.schema
            .table_indexes(table)
            .map(|indexes| {
                indexes
                    .iter()
                    .any(|index| index.columns().first() == Some(&column))
            })
            .unwrap_or(false)
    }

    /// Joins `rows` with `right_side` on `left_key = right_col` using a hash
    /// table built over the right rows.
    ///
    /// Matching rows are emitted in left-row order and, within one left row,
    /// in right-row order. `NULL` keys never match. Unmatched rows kept by an
    /// outer join get `None` for the missing side, which [`JoinSide::row`]
    /// turns into `NULL` values.
    #[expect(
        clippy::too_many_arguments,
        reason = "arguments describe both join sides and the outer-join flags"
    )]
    fn hash_join(
        &self,
        sides: &[JoinSide],
        rows: &[RowRef],
        left_key: Option<KeyPosition>,
        right_side: &JoinSide,
        right_col: &str,
        keep_unmatched_left: bool,
        keep_unmatched_right: bool,
    ) -> Vec<RowRef> {
        let mut build: HashMap<&Value, Vec<usize>> = HashMap::new();
        if let Some(column_index) = right_side.column_position(right_col) {
            for (row_index, row) in right_side.rows.iter().enumerate() {
                let value = &row[column_index].1;
                if !value.is_null() {
                    build.entry(value).or_default().push(row_index);
                }
            }
        }

        let mut results = Vec::with_capacity(rows.len());
        let mut right_matched = vec![false; right_side.rows.len()];

        for row in rows {
            let left_value = left_key.and_then(|(side_index, column_index)| {
                row[side_index].map(|row_index| &sides[side_index].rows[row_index][column_index].1)
            });
            let matches = left_value
                .filter(|value| !value.is_null())
                .and_then(|value| build.get(value));

            match matches {
                Some(right_indexes) => {
                    for &right_index in right_indexes {
                        let mut new_row = row.clone();
                        new_row.push(Some(right_index));
                        results.push(new_row);
                        right_matched[right_index] = true;
                    }
                }
                None if keep_unmatched_left => {
                    let mut new_row = row.clone();
                    new_row.push(None);
                    results.push(new_row);
                }
                None => {}
            }
        }

        if keep_unmatched_right {
            for (right_index, matched) in right_matched.iter().enumerate() {
                if !matched {
                    let mut new_row: RowRef = vec![None; sides.len()];
                    new_row.push(Some(right_index));
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

    /// Returns the `(table, columns)` groups of a joined row, borrowing the
    /// loaded rows (or the `NULL` row for a padded side).
    fn row_groups<'a>(
        &self,
        sides: &'a [JoinSide],
        row: &RowRef,
    ) -> Vec<(&'a str, &'a [(ColumnDef, Value)])> {
        sides
            .iter()
            .zip(row)
            .map(|(side, index)| (side.table.as_str(), side.row(*index)))
            .collect()
    }

    /// Sorts joined rows by a column.
    fn sort_joined_rows(
        &self,
        sides: &[JoinSide],
        rows: &mut [RowRef],
        column: &str,
        direction: OrderDirection,
    ) {
        let (table, col) = match column.split_once('.') {
            Some((table, col)) => (Some(table), col),
            None => (None, column),
        };

        rows.sort_by(|a, b| {
            let a_val = self.find_value(sides, a, table, col);
            let b_val = self.find_value(sides, b, table, col);

            crate::database::sort_values_with_direction(a_val, b_val, direction)
        });
    }

    /// Finds a column value in a joined row, optionally scoped to a table.
    fn find_value<'a>(
        &self,
        sides: &'a [JoinSide],
        row: &RowRef,
        table: Option<&str>,
        column: &str,
    ) -> Option<&'a Value> {
        sides
            .iter()
            .zip(row)
            .filter(|(side, _)| table.is_none_or(|table| side.table == table))
            .find_map(|(side, index)| {
                side.row(*index)
                    .iter()
                    .find(|(def, _)| def.name == column)
                    .map(|(_, value)| value)
            })
    }

    /// Returns, for every column of `side`, whether the query selects it.
    ///
    /// A column is selected when the query selects every column, its bare
    /// name, or its `table.column` qualified name.
    fn selected_columns(&self, side: &JoinSide, query: &Query) -> Vec<bool> {
        let selected = query.raw_columns();
        side.columns
            .iter()
            .map(|column| {
                query.all_selected()
                    || selected.iter().any(|field| {
                        field.as_str() == column.name
                            || field
                                .strip_prefix(side.table.as_str())
                                .and_then(|rest| rest.strip_prefix('.'))
                                == Some(column.name)
                    })
            })
            .collect()
    }

    /// Materializes one joined row, cloning each selected value exactly once.
    fn materialize_row(
        &self,
        sides: &[JoinSide],
        selected: &[Vec<bool>],
        width: usize,
        row: &RowRef,
    ) -> Vec<Value> {
        let mut values = Vec::with_capacity(width);
        for ((side, mask), index) in sides.iter().zip(selected).zip(row) {
            for ((_, value), is_selected) in side.row(*index).iter().zip(mask) {
                if *is_selected {
                    values.push(value.clone());
                }
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, DbmsError, Filter, InsertRecord as _, JoinRow, Nullable, Query, QueryError,
        TableSchema as _, Text, Uint32, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::JoinEngine;
    use crate::database::MAX_ALTERNATIVES;
    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    // Use tables WITHOUT foreign key constraints so we can test all join
    // types including unmatched rows without FK validation failures.

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "departments"]
    pub struct Department {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
        pub code: Nullable<Uint32>,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "employees"]
    pub struct Employee {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
        pub dept_id: Uint32,
        pub dept_code: Nullable<Uint32>,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "assignments"]
    pub struct Assignment {
        #[primary_key]
        pub id: Uint32,
        pub employee_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(
        Department = "departments",
        Employee = "employees",
        Assignment = "assignments"
    )]
    pub struct TestSchema;

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "indexed_departments"]
    pub struct IndexedDepartment {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "indexed_employees"]
    pub struct IndexedEmployee {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
        #[index]
        pub dept_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(
        IndexedDepartment = "indexed_departments",
        IndexedEmployee = "indexed_employees"
    )]
    pub struct IndexedJoinSchema;

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "covering_employees"]
    pub struct CoveringEmployee {
        #[primary_key]
        #[index(group = "all_columns")]
        pub id: Uint32,
        #[index(group = "all_columns")]
        pub name: Text,
        #[index(group = "all_columns")]
        pub dept_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(Department = "departments", CoveringEmployee = "covering_employees")]
    pub struct CoveringJoinSchema;

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "composite_indexed_employees"]
    pub struct CompositeIndexedEmployee {
        #[primary_key]
        pub id: Uint32,
        #[index(group = "department_name")]
        pub dept_id: Uint32,
        #[index(group = "department_name")]
        pub name: Text,
    }

    #[derive(DatabaseSchema)]
    #[tables(
        Department = "departments",
        CompositeIndexedEmployee = "composite_indexed_employees"
    )]
    pub struct CompositeIndexedJoinSchema;

    fn setup() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
        ctx
    }

    fn setup_indexed() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        IndexedJoinSchema::register_tables(&ctx).unwrap();
        ctx
    }

    fn insert_dept(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, name: &str) {
        let insert = DepartmentInsertRequest::from_values(&[
            (Department::columns()[0], Value::Uint32(Uint32(id))),
            (
                Department::columns()[1],
                Value::Text(Text(name.to_string())),
            ),
        ])
        .unwrap();
        db.insert::<Department>(insert).unwrap();
    }

    fn insert_emp(
        db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
        id: u32,
        name: &str,
        dept_id: u32,
    ) {
        let insert = EmployeeInsertRequest::from_values(&[
            (Employee::columns()[0], Value::Uint32(Uint32(id))),
            (Employee::columns()[1], Value::Text(Text(name.to_string()))),
            (Employee::columns()[2], Value::Uint32(Uint32(dept_id))),
        ])
        .unwrap();
        db.insert::<Employee>(insert).unwrap();
    }

    fn insert_assignment(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, employee_id: u32) {
        let insert = AssignmentInsertRequest::from_values(&[
            (Assignment::columns()[0], Value::Uint32(Uint32(id))),
            (Assignment::columns()[1], Value::Uint32(Uint32(employee_id))),
        ])
        .unwrap();
        db.insert::<Assignment>(insert).unwrap();
    }

    fn insert_indexed_dept(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, name: &str) {
        let insert = IndexedDepartmentInsertRequest::from_values(&[
            (IndexedDepartment::columns()[0], Value::Uint32(Uint32(id))),
            (
                IndexedDepartment::columns()[1],
                Value::Text(Text(name.to_string())),
            ),
        ])
        .unwrap();
        db.insert::<IndexedDepartment>(insert).unwrap();
    }

    fn insert_indexed_emp(
        db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
        id: u32,
        name: &str,
        dept_id: u32,
    ) {
        let insert = IndexedEmployeeInsertRequest::from_values(&[
            (IndexedEmployee::columns()[0], Value::Uint32(Uint32(id))),
            (
                IndexedEmployee::columns()[1],
                Value::Text(Text(name.to_string())),
            ),
            (
                IndexedEmployee::columns()[2],
                Value::Uint32(Uint32(dept_id)),
            ),
        ])
        .unwrap();
        db.insert::<IndexedEmployee>(insert).unwrap();
    }

    fn insert_covering_emp(
        db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
        id: u32,
        name: &str,
        dept_id: u32,
    ) {
        let insert = CoveringEmployeeInsertRequest::from_values(&[
            (CoveringEmployee::columns()[0], Value::Uint32(Uint32(id))),
            (
                CoveringEmployee::columns()[1],
                Value::Text(Text(name.to_string())),
            ),
            (
                CoveringEmployee::columns()[2],
                Value::Uint32(Uint32(dept_id)),
            ),
        ])
        .unwrap();
        db.insert::<CoveringEmployee>(insert).unwrap();
    }

    fn insert_composite_indexed_emp(
        db: &WasmDbmsDatabase<'_, HeapMemoryProvider>,
        id: u32,
        dept_id: u32,
        name: &str,
    ) {
        let insert = CompositeIndexedEmployeeInsertRequest::from_values(&[
            (
                CompositeIndexedEmployee::columns()[0],
                Value::Uint32(Uint32(id)),
            ),
            (
                CompositeIndexedEmployee::columns()[1],
                Value::Uint32(Uint32(dept_id)),
            ),
            (
                CompositeIndexedEmployee::columns()[2],
                Value::Text(Text(name.to_string())),
            ),
        ])
        .unwrap();
        db.insert::<CompositeIndexedEmployee>(insert).unwrap();
    }

    #[test]
    fn test_inner_join_does_not_match_null_keys() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "code", "dept_code")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_outer_join_keeps_rows_with_null_keys_unmatched() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .full_join("employees", "code", "dept_code")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 2);
        let non_null_ids: Vec<Vec<(&str, Value)>> = results
            .iter()
            .map(|row| {
                row.iter()
                    .filter(|(column, value)| column.name == "id" && !value.is_null())
                    .map(|(column, value)| {
                        (
                            column
                                .table
                                .as_deref()
                                .expect("join column must name its table"),
                            value.clone(),
                        )
                    })
                    .collect()
            })
            .collect();
        assert_eq!(
            non_null_ids,
            vec![
                vec![("departments", Value::from(1u32))],
                vec![("employees", Value::from(10u32))],
            ]
        );
    }

    #[test]
    fn test_inner_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        // eng has 2 employees, hr has 0 → 2 rows
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_left_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .left_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        // eng has 1 employee, hr has 0 but LEFT keeps unmatched left → 2 rows
        assert_eq!(results.len(), 2);

        // Find hr's row: employee columns should be Null
        let hr_row = results
            .iter()
            .find(|row| {
                row.iter().any(|(col, val)| {
                    col.name == "name"
                        && col.table.as_deref() == Some("departments")
                        && *val == Value::Text(Text("hr".to_string()))
                })
            })
            .expect("hr should be in results");

        // hr's employee name should be Null
        let emp_name = hr_row
            .iter()
            .find(|(col, _)| col.name == "name" && col.table.as_deref() == Some("employees"))
            .expect("employee name column should exist for hr");
        assert_eq!(*emp_name.1, Value::Null);
    }

    #[test]
    fn test_right_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);
        // charlie references dept 999 which doesn't exist (no FK constraint)
        insert_emp(&db, 11, "charlie", 999);

        let query = Query::builder()
            .all()
            .right_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        // alice matches eng, charlie (dept_id=999) is unmatched right → 2 rows
        assert_eq!(results.len(), 2);

        // charlie should have null department columns
        let charlie_row = results
            .iter()
            .find(|row| {
                row.iter().any(|(col, val)| {
                    col.name == "name"
                        && col.table.as_deref() == Some("employees")
                        && *val == Value::Text(Text("charlie".to_string()))
                })
            })
            .expect("charlie should be in results");

        let dept_name = charlie_row
            .iter()
            .find(|(col, _)| col.name == "name" && col.table.as_deref() == Some("departments"))
            .expect("department name column should exist for charlie");
        assert_eq!(*dept_name.1, Value::Null);
    }

    #[test]
    fn test_full_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        // charlie references dept 999 which doesn't exist
        insert_emp(&db, 11, "charlie", 999);

        let query = Query::builder()
            .all()
            .full_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        // eng-alice matched (1), hr unmatched left (1), charlie unmatched right (1) = 3
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_join_with_filter() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 2);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .and_where(Filter::eq(
                "departments.name",
                Value::Text(Text("eng".to_string())),
            ))
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_join_with_order_by() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "zzz", 1);
        insert_emp(&db, 11, "aaa", 2);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .order_by_asc("employees.name")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 2);
        let first_name = results
            .row(0)
            .unwrap()
            .iter()
            .find(|(col, _)| col.name == "name" && col.table.as_deref() == Some("employees"))
            .unwrap();
        assert_eq!(*first_name.1, Value::Text(Text("aaa".to_string())));
    }

    #[test]
    fn test_join_with_limit() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 2);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .limit(1)
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_join_with_offset() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 2);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .offset(1)
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_join_with_column_selection() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .field("departments.name")
            .field("employees.name")
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results.columns.len(), 2);
        assert_eq!(results.row(0).unwrap().len(), 2);
    }

    #[test]
    fn test_inner_join_empty_result() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        // No employees

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_join_offset_exceeding_results_returns_empty() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .offset(100)
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_join_on_indexed_column() {
        let ctx = setup_indexed();
        let db = WasmDbmsDatabase::oneshot(&ctx, IndexedJoinSchema);
        insert_indexed_dept(&db, 1, "eng");
        insert_indexed_dept(&db, 2, "hr");
        insert_indexed_emp(&db, 10, "alice", 1);
        insert_indexed_emp(&db, 11, "bob", 2);

        let query = Query::builder()
            .all()
            .inner_join(
                "indexed_employees",
                "indexed_departments.id",
                "indexed_employees.dept_id",
            )
            .build();
        let results = db.select_join("indexed_departments", query).unwrap();

        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|row| {
            row.iter().any(|(column, value)| {
                column.name == "name"
                    && column.table.as_deref() == Some("indexed_employees")
                    && *value == Value::Text(Text("alice".to_string()))
            })
        }));
    }

    /// Returns the value of `table.column` in a joined row, if the column is present.
    fn joined_value<'a>(row: &JoinRow<'a>, table: &str, column: &str) -> Option<&'a Value> {
        row.get(&format!("{table}.{column}"))
    }

    /// Asserts that `row` has every department and employee column, in schema
    /// order, and that the columns of `null_table` are all `NULL`.
    fn assert_complete_row_with_null_side(row: &JoinRow<'_>, null_table: &str) {
        let shape: Vec<(Option<&str>, &str)> = row
            .iter()
            .map(|(col, _)| (col.table.as_deref(), col.name.as_str()))
            .collect();
        let expected_shape: Vec<(Option<&str>, &str)> = Department::columns()
            .iter()
            .map(|col| (Some("departments"), col.name))
            .chain(
                Employee::columns()
                    .iter()
                    .map(|col| (Some("employees"), col.name)),
            )
            .collect();
        assert_eq!(shape, expected_shape);

        for (col, value) in row.iter() {
            if col.table.as_deref() == Some(null_table) {
                assert_eq!(
                    *value,
                    Value::Null,
                    "{null_table}.{} must be NULL",
                    col.name
                );
            }
        }
    }

    #[test]
    fn test_left_join_with_empty_right_table_keeps_null_right_columns() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");

        let query = Query::builder()
            .all()
            .left_join("employees", "departments.id", "employees.dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "employees");
        assert_eq!(
            joined_value(&results.row(0).unwrap(), "departments", "name"),
            Some(&Value::Text(Text("eng".to_string())))
        );
    }

    #[test]
    fn test_right_join_with_empty_left_table_keeps_null_left_columns() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .right_join("employees", "departments.id", "employees.dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "departments");
        assert_eq!(
            joined_value(&results.row(0).unwrap(), "employees", "name"),
            Some(&Value::Text(Text("alice".to_string())))
        );
    }

    #[test]
    fn test_full_join_with_empty_right_table_keeps_null_right_columns() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");

        let query = Query::builder()
            .all()
            .full_join("employees", "departments.id", "employees.dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "employees");
    }

    #[test]
    fn test_full_join_with_empty_left_table_keeps_null_left_columns() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .full_join("employees", "departments.id", "employees.dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "departments");
    }

    #[test]
    fn test_full_join_with_unmatched_rows_on_both_sides_has_stable_shape() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 999);

        let query = Query::builder()
            .all()
            .full_join("employees", "departments.id", "employees.dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 2);
        let eng_row = results
            .iter()
            .find(|row| joined_value(row, "departments", "id") == Some(&Value::Uint32(Uint32(1))))
            .expect("eng should be in results");
        assert_complete_row_with_null_side(&eng_row, "employees");
        let alice_row = results
            .iter()
            .find(|row| joined_value(row, "employees", "id") == Some(&Value::Uint32(Uint32(10))))
            .expect("alice should be in results");
        assert_complete_row_with_null_side(&alice_row, "departments");
    }

    #[test]
    fn test_join_filter_with_ambiguous_column_returns_error() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::eq("id", Value::Uint32(Uint32(1))))
            .build();
        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("Ambiguous column 'id'")
            ),
            "expected ambiguous column error, got {result:?}"
        );
    }

    #[test]
    fn test_join_filter_with_out_of_scope_table_returns_error() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::eq("projects.id", Value::Uint32(Uint32(1))))
            .build();
        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("Table 'projects' not in query scope")
            ),
            "expected out-of-scope table error, got {result:?}"
        );
    }

    #[test]
    fn test_join_filter_with_invalid_like_operand_returns_error() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::like("employees.dept_id", "1%"))
            .build();
        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("LIKE operator can only be applied to Text values")
            ),
            "expected LIKE type error, got {result:?}"
        );
    }

    #[test]
    fn test_join_filter_with_ambiguous_column_returns_error_for_empty_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::eq("id", Value::Uint32(Uint32(1))))
            .build();

        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("Ambiguous column 'id'")
            ),
            "expected ambiguous column error, got {result:?}"
        );
    }

    #[test]
    fn test_join_filter_with_out_of_scope_table_returns_error_for_empty_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::eq("projects.id", Value::Uint32(Uint32(1))))
            .build();

        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("Table 'projects' not in query scope")
            ),
            "expected out-of-scope table error, got {result:?}"
        );
    }

    #[test]
    fn test_join_filter_with_invalid_like_operand_returns_error_for_empty_join() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::like("employees.dept_id", "1%"))
            .build();

        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("LIKE operator can only be applied to Text values")
            ),
            "expected LIKE type error, got {result:?}"
        );
    }

    #[test]
    fn test_left_join_like_filter_skips_null_padded_rows() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .left_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::like("employees.name", "a%"))
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            joined_value(&results.row(0).unwrap(), "employees", "name"),
            Some(&Value::Text(Text("alice".to_string())))
        );

        let query = Query::builder()
            .all()
            .left_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::like("employees.name", "a%").not())
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            joined_value(&results.row(0).unwrap(), "departments", "name"),
            Some(&Value::Text(Text("hr".to_string())))
        );
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "employees");
    }

    #[test]
    fn test_join_filter_with_dangling_like_escape_returns_error() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "abc", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "departments.id", "employees.dept_id")
            .and_where(Filter::like("employees.name", "abc\\"))
            .build();
        let result = db.select_join("departments", query);

        assert!(
            matches!(
                &result,
                Err(DbmsError::Query(QueryError::InvalidQuery(message)))
                    if message.contains("Invalid LIKE pattern")
            ),
            "expected invalid LIKE pattern error, got {result:?}"
        );
    }

    #[test]
    fn test_inner_join_with_duplicate_keys_on_both_sides_emits_every_pair() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 1);
        insert_emp(&db, 12, "carol", 2);
        insert_assignment(&db, 100, 1);
        insert_assignment(&db, 101, 1);
        insert_assignment(&db, 102, 3);

        let query = Query::builder()
            .all()
            .inner_join(
                "assignments",
                "employees.dept_id",
                "assignments.employee_id",
            )
            .build();
        let results = db.select_join("employees", query).unwrap();

        assert_eq!(results.len(), 4);
        let emp = results.column_index("employees.id").unwrap();
        let assignment = results.column_index("assignments.id").unwrap();
        let pairs: Vec<(Value, Value)> = results
            .rows
            .iter()
            .map(|row| (row[emp].clone(), row[assignment].clone()))
            .collect();
        let pair = |e: u32, a: u32| (Value::Uint32(Uint32(e)), Value::Uint32(Uint32(a)));
        assert_eq!(
            pairs,
            vec![pair(10, 100), pair(10, 101), pair(11, 100), pair(11, 101)]
        );
    }

    #[test]
    fn test_join_chain_resolves_key_on_previously_joined_table() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);
        insert_emp(&db, 11, "bob", 1);
        insert_assignment(&db, 100, 10);
        insert_assignment(&db, 101, 10);
        insert_assignment(&db, 102, 11);
        insert_assignment(&db, 103, 999);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .inner_join("assignments", "employees.id", "employee_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 3);
        for row in &results {
            assert_eq!(row.get("employees.id"), row.get("assignments.employee_id"));
            assert_eq!(row.get("departments.id"), Some(&Value::Uint32(Uint32(1))));
        }
    }

    #[test]
    fn test_inner_join_on_missing_right_column_returns_no_rows() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "no_such_column")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert!(results.is_empty());

        let query = Query::builder()
            .all()
            .left_join("employees", "id", "no_such_column")
            .build();
        let results = db.select_join("departments", query).unwrap();
        assert_eq!(results.len(), 1);
        assert_complete_row_with_null_side(&results.row(0).unwrap(), "employees");
    }

    #[test]
    fn test_inner_join_empty_result_keeps_columns() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);

        let query = Query::builder()
            .field("departments.name")
            .field("employees.name")
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert!(results.is_empty());
        let shape: Vec<(Option<&str>, &str)> = results
            .columns
            .iter()
            .map(|col| (col.table.as_deref(), col.name.as_str()))
            .collect();
        assert_eq!(
            shape,
            vec![(Some("departments"), "name"), (Some("employees"), "name")]
        );
    }

    #[test]
    fn test_inner_join_with_many_rows_matches_every_employee_to_its_department() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        for dept in 1..=300u32 {
            insert_dept(&db, dept, &format!("dept_{dept}"));
        }
        for emp in 1..=3000u32 {
            insert_emp(&db, emp, &format!("emp_{emp}"), ((emp - 1) % 300) + 1);
        }

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 3000);
        let dept_id = results.column_index("departments.id").unwrap();
        let emp_dept = results.column_index("employees.dept_id").unwrap();
        assert!(results.rows.iter().all(|row| row[dept_id] == row[emp_dept]));
    }

    #[test]
    fn test_join_with_qualified_and_unqualified_column_selection() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_emp(&db, 10, "alice", 1);

        let query = Query::builder()
            .field("name")
            .field("employees.dept_id")
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        let selected: Vec<(Option<&str>, &str)> = results
            .columns
            .iter()
            .map(|column| (column.table.as_deref(), column.name.as_str()))
            .collect();
        assert_eq!(
            selected,
            vec![
                (Some("departments"), "name"),
                (Some("employees"), "name"),
                (Some("employees"), "dept_id"),
            ]
        );
        let row = results.row(0).unwrap();
        assert_eq!(
            row.get("employees.name"),
            Some(&Value::Text(Text("alice".to_string())))
        );
        assert_eq!(row.get("name"), Some(&Value::Text(Text("eng".to_string()))));
    }

    #[test]
    fn test_inner_join_on_unindexed_column_with_more_keys_than_right_rows() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        for dept in 1..=50u32 {
            insert_dept(&db, dept, &format!("dept_{dept}"));
        }
        insert_emp(&db, 10, "alice", 7);
        insert_emp(&db, 11, "bob", 7);
        insert_emp(&db, 12, "carol", 999);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "dept_id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 2);
        for row in &results {
            assert_eq!(row.get("departments.id"), Some(&Value::Uint32(Uint32(7))));
        }
    }

    #[test]
    fn test_inner_join_preserves_physical_right_order_with_covering_index_before_limit() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        CoveringJoinSchema::register_tables(&ctx).unwrap();
        let db = WasmDbmsDatabase::oneshot(&ctx, CoveringJoinSchema);
        insert_dept(&db, 1, "eng");
        insert_covering_emp(&db, 20, "inserted first", 1);
        insert_covering_emp(&db, 10, "inserted second", 1);

        let query = Query::builder()
            .all()
            .inner_join("covering_employees", "id", "dept_id")
            .limit(1)
            .build();
        let results = db.select_join("departments", query).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(
            results.row(0).unwrap().get("covering_employees.id"),
            Some(&Value::Uint32(Uint32(20)))
        );
    }

    #[test]
    fn test_indexed_join_key_push_down_respects_access_planner_limit() {
        let schema = IndexedJoinSchema;
        let engine = JoinEngine::<_, HeapMemoryProvider>::new(&schema);

        assert!(engine.should_push_down_join_keys(
            "indexed_employees",
            "dept_id",
            MAX_ALTERNATIVES
        ));
        assert!(!engine.should_push_down_join_keys(
            "indexed_employees",
            "dept_id",
            MAX_ALTERNATIVES + 1
        ));
        assert!(!engine.should_push_down_join_keys(
            "indexed_employees",
            "id",
            MAX_ALTERNATIVES + 1
        ));

        let composite_schema = CompositeIndexedJoinSchema;
        let composite_engine = JoinEngine::<_, HeapMemoryProvider>::new(&composite_schema);
        assert!(composite_engine.should_push_down_join_keys(
            "composite_indexed_employees",
            "dept_id",
            MAX_ALTERNATIVES
        ));
        assert!(!composite_engine.should_push_down_join_keys(
            "composite_indexed_employees",
            "dept_id",
            MAX_ALTERNATIVES + 1
        ));
    }

    #[test]
    fn test_oversized_indexed_join_keys_still_return_matches() {
        let key_count = MAX_ALTERNATIVES as u32 + 1;

        let ctx = setup_indexed();
        let db = WasmDbmsDatabase::oneshot(&ctx, IndexedJoinSchema);
        for id in 1..=key_count {
            insert_indexed_dept(&db, id, "department");
            insert_indexed_emp(&db, id, "employee", id);
        }
        ctx.reset_access_stats();
        let results = db
            .select_join(
                "indexed_departments",
                Query::builder()
                    .all()
                    .inner_join("indexed_employees", "id", "id")
                    .build(),
            )
            .unwrap();
        assert_eq!(results.len(), key_count as usize);
        assert_eq!(ctx.access_stats().scanned_rows, u64::from(key_count) * 2);

        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        CompositeIndexedJoinSchema::register_tables(&ctx).unwrap();
        let db = WasmDbmsDatabase::oneshot(&ctx, CompositeIndexedJoinSchema);
        for id in 1..=key_count {
            insert_dept(&db, id, "department");
            insert_composite_indexed_emp(&db, id, id, "employee");
        }
        ctx.reset_access_stats();
        let results = db
            .select_join(
                "departments",
                Query::builder()
                    .all()
                    .inner_join("composite_indexed_employees", "id", "dept_id")
                    .build(),
            )
            .unwrap();
        assert_eq!(results.len(), key_count as usize);
        assert_eq!(ctx.access_stats().scanned_rows, u64::from(key_count) * 2);
    }

    #[test]
    fn test_inner_join_on_primary_key_column_pushes_keys_down() {
        let ctx = setup();
        let db = WasmDbmsDatabase::oneshot(&ctx, TestSchema);
        insert_dept(&db, 1, "eng");
        insert_dept(&db, 2, "hr");
        insert_dept(&db, 3, "ops");
        insert_emp(&db, 1, "alice", 1);
        insert_emp(&db, 3, "bob", 1);
        insert_emp(&db, 9, "carol", 1);

        let query = Query::builder()
            .all()
            .inner_join("employees", "id", "id")
            .order_by_asc("employees.id")
            .build();
        let results = db.select_join("departments", query).unwrap();

        let ids: Vec<Option<&Value>> = results.iter().map(|row| row.get("employees.id")).collect();
        assert_eq!(
            ids,
            vec![
                Some(&Value::Uint32(Uint32(1))),
                Some(&Value::Uint32(Uint32(3)))
            ]
        );
    }
}

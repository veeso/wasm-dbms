// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Join execution engine for cross-table queries.

use wasm_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, DbmsResult, JoinType, OrderDirection, Query, Value,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryProvider};

use crate::database::WasmDbmsDatabase;
use crate::schema::DatabaseSchema;

/// A row in the joined result, organized by source table.
type JoinedRow = Vec<(String, Vec<(ColumnDef, Value)>)>;

/// Engine that executes join queries using nested-loop join.
pub struct JoinEngine<'a, Schema: ?Sized, M, A = AccessControlList>
where
    Schema: DatabaseSchema<M, A>,
    M: MemoryProvider,
    A: AccessControl,
{
    schema: &'a Schema,
    _marker: std::marker::PhantomData<(M, A)>,
}

impl<'a, Schema: ?Sized, M, A> JoinEngine<'a, Schema, M, A>
where
    Schema: DatabaseSchema<M, A>,
    M: MemoryProvider,
    A: AccessControl,
{
    pub fn new(schema: &'a Schema) -> Self {
        Self {
            schema,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Schema: ?Sized, M, A> JoinEngine<'_, Schema, M, A>
where
    Schema: DatabaseSchema<M, A>,
    M: MemoryProvider,
    A: AccessControl,
{
    /// Executes a join query using nested-loop join.
    pub fn join(
        &self,
        dbms: &WasmDbmsDatabase<'_, M, A>,
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

            crate::database::sort_values_with_direction(a_val, b_val, direction)
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
            .find_map(|(col, value)| {
                if col.name == column {
                    Some(value)
                } else {
                    None
                }
            })
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

#[cfg(test)]
mod tests {

    use wasm_dbms_api::prelude::{
        Database as _, Filter, InsertRecord as _, Query, TableSchema as _, Text, Uint32, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use crate::prelude::{DbmsContext, WasmDbmsDatabase};

    // Use tables WITHOUT foreign key constraints so we can test all join
    // types including unmatched rows without FK validation failures.

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "departments"]
    pub struct Department {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "employees"]
    pub struct Employee {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
        pub dept_id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(Department = "departments", Employee = "employees")]
    pub struct TestSchema;

    fn setup() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        TestSchema::register_tables(&ctx).unwrap();
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
        assert_eq!(emp_name.1, Value::Null);
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
        assert_eq!(dept_name.1, Value::Null);
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
        let first_name = results[0]
            .iter()
            .find(|(col, _)| col.name == "name" && col.table.as_deref() == Some("employees"))
            .unwrap();
        assert_eq!(first_name.1, Value::Text(Text("aaa".to_string())));
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
        assert_eq!(results[0].len(), 2);
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
}

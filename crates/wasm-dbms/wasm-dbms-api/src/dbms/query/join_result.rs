//! Result of a join query: column descriptions once, then rows of values.

use serde::{Deserialize, Serialize};

use crate::dbms::table::JoinColumnDef;
use crate::dbms::value::Value;

/// Result of [`Database::select_join`](crate::prelude::Database::select_join).
///
/// The column descriptions are stored once in `columns`; every entry of
/// `rows` holds one [`Value`] per column, in the same order. Use
/// [`Self::column_index`] to resolve a column name once and index each row,
/// or [`Self::iter`] to walk the rows as [`JoinRow`] views.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct JoinResultSet {
    /// One description per output column, in row order. The `table` field
    /// names the table each column comes from.
    pub columns: Vec<JoinColumnDef>,
    /// One entry per row; `rows[r][c]` belongs to `columns[c]`.
    pub rows: Vec<Vec<Value>>,
}

impl JoinResultSet {
    /// Returns the number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns `true` when the result has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns the index of `column` in [`Self::columns`].
    ///
    /// `"table.column"` matches the column of that table. A bare name matches
    /// the first column with that name in row order.
    pub fn column_index(&self, column: &str) -> Option<usize> {
        column_index(&self.columns, column)
    }

    /// Returns a view over the row at `index`, if it exists.
    pub fn row(&self, index: usize) -> Option<JoinRow<'_>> {
        self.rows.get(index).map(|values| JoinRow {
            columns: &self.columns,
            values,
        })
    }

    /// Iterates over the rows as [`JoinRow`] views.
    pub fn iter(&self) -> impl Iterator<Item = JoinRow<'_>> {
        self.rows.iter().map(|values| JoinRow {
            columns: &self.columns,
            values,
        })
    }
}

impl<'a> IntoIterator for &'a JoinResultSet {
    type Item = JoinRow<'a>;
    type IntoIter = JoinRows<'a>;

    fn into_iter(self) -> Self::IntoIter {
        JoinRows {
            columns: &self.columns,
            rows: self.rows.iter(),
        }
    }
}

/// Iterator over the rows of a [`JoinResultSet`], yielding [`JoinRow`] views.
#[derive(Debug)]
pub struct JoinRows<'a> {
    columns: &'a [JoinColumnDef],
    rows: std::slice::Iter<'a, Vec<Value>>,
}

impl<'a> Iterator for JoinRows<'a> {
    type Item = JoinRow<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rows.next().map(|values| JoinRow {
            columns: self.columns,
            values,
        })
    }
}

/// A borrowed row of a [`JoinResultSet`], paired with its column descriptions.
#[derive(Debug, Clone, Copy)]
pub struct JoinRow<'a> {
    columns: &'a [JoinColumnDef],
    values: &'a [Value],
}

impl<'a> JoinRow<'a> {
    /// Returns the column descriptions of the result set.
    pub fn columns(&self) -> &'a [JoinColumnDef] {
        self.columns
    }

    /// Returns the values of this row, in column order.
    pub fn values(&self) -> &'a [Value] {
        self.values
    }

    /// Returns the number of values in the row.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns `true` when the row has no values.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the value of `column`, resolved like
    /// [`JoinResultSet::column_index`].
    pub fn get(&self, column: &str) -> Option<&'a Value> {
        column_index(self.columns, column).and_then(|index| self.values.get(index))
    }

    /// Iterates over `(column, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&'a JoinColumnDef, &'a Value)> + use<'a> {
        self.columns.iter().zip(self.values)
    }
}

/// Resolves a qualified or bare column name against `columns`.
fn column_index(columns: &[JoinColumnDef], column: &str) -> Option<usize> {
    match column.split_once('.') {
        Some((table, name)) => columns
            .iter()
            .position(|def| def.table.as_deref() == Some(table) && def.name == name),
        None => columns.iter().position(|def| def.name == column),
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::dbms::table::ColumnDef;
    use crate::dbms::types::{DataTypeKind, Text};

    fn column(table: &str, name: &'static str) -> JoinColumnDef {
        let mut def = JoinColumnDef::from(ColumnDef {
            name,
            data_type: DataTypeKind::Text,
            auto_increment: false,
            nullable: false,
            primary_key: false,
            unique: false,
            foreign_key: None,
            default: None,
            renamed_from: &[],
        });
        def.table = Some(table.to_string());
        def
    }

    fn text(value: &str) -> Value {
        Value::Text(Text(value.to_string()))
    }

    fn result_set() -> JoinResultSet {
        JoinResultSet {
            columns: vec![
                column("users", "id"),
                column("users", "name"),
                column("posts", "id"),
                column("posts", "title"),
            ],
            rows: vec![
                vec![text("1"), text("alice"), text("10"), text("hello")],
                vec![text("1"), text("alice"), text("11"), text("world")],
            ],
        }
    }

    #[test]
    fn test_should_report_length_and_emptiness() {
        let set = result_set();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert!(JoinResultSet::default().is_empty());
    }

    #[test]
    fn test_should_resolve_qualified_and_unqualified_column_index() {
        let set = result_set();
        assert_eq!(set.column_index("posts.id"), Some(2));
        assert_eq!(set.column_index("users.id"), Some(0));
        assert_eq!(set.column_index("id"), Some(0));
        assert_eq!(set.column_index("title"), Some(3));
        assert_eq!(set.column_index("comments.id"), None);
        assert_eq!(set.column_index("missing"), None);
    }

    #[test]
    fn test_should_read_row_values_by_name() {
        let set = result_set();
        let row = set.row(1).expect("second row exists");
        assert_eq!(row.len(), 4);
        assert_eq!(row.get("posts.title"), Some(&text("world")));
        assert_eq!(row.get("name"), Some(&text("alice")));
        assert_eq!(row.get("posts.missing"), None);
        assert!(set.row(2).is_none());
    }

    #[test]
    fn test_should_iterate_rows_zipped_with_columns() {
        let set = result_set();
        let pairs: Vec<(Option<&str>, &str, &Value)> = set
            .iter()
            .flat_map(|row| row.iter())
            .map(|(def, value)| (def.table.as_deref(), def.name.as_str(), value))
            .collect();
        assert_eq!(pairs.len(), 8);
        assert_eq!(pairs[2], (Some("posts"), "id", &text("10")));
        assert_eq!(pairs[7], (Some("posts"), "title", &text("world")));

        let mut count = 0;
        for row in &set {
            assert_eq!(row.columns().len(), 4);
            assert_eq!(row.values().len(), 4);
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[test]
    fn test_should_serialize_as_columns_and_rows() {
        let set = result_set();
        let json = serde_json::to_value(&set).expect("serialize");
        assert_eq!(json["columns"].as_array().map(Vec::len), Some(4));
        assert_eq!(json["rows"].as_array().map(Vec::len), Some(2));
        let back: JoinResultSet = serde_json::from_value(json).expect("deserialize");
        assert_eq!(back, set);
    }
}

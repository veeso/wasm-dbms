use crate::dbms::table::{ColumnDef, TableSchema};
use crate::dbms::value::Value;
use crate::error::DbmsResult;
use crate::prelude::Filter;

pub type TableColumns = Vec<(ValuesSource, Vec<(ColumnDef, Value)>)>;

/// Flattens [`TableColumns`] rows into flat column-value pairs.
///
/// Only includes columns whose source is [`ValuesSource::This`],
/// discarding any foreign or eager-loaded columns.
pub fn flatten_table_columns(rows: Vec<TableColumns>) -> Vec<Vec<(ColumnDef, Value)>> {
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .filter(|(source, _)| *source == ValuesSource::This)
                .flat_map(|(_, cols)| cols)
                .collect()
        })
        .collect()
}

/// Indicates the source of the column values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValuesSource {
    /// Column values belong to the current table.
    This,
    /// Column values belong to a foreign table.
    Foreign { table: String, column: String },
}

/// This trait represents a record returned by a [`crate::dbms::query::Query`] for a table.
pub trait TableRecord: Clone {
    /// The table schema associated with this record.
    type Schema: TableSchema<Record = Self>;

    /// Constructs [`TableRecord`] from a list of column values grouped by table.
    fn from_values(values: TableColumns) -> Self;

    /// Converts the record into a list of column [`Value`]s.
    fn to_values(&self) -> Vec<(ColumnDef, Value)>;
}

/// This trait represents a record for inserting into a table.
pub trait InsertRecord: Sized + Clone {
    /// The [`TableRecord`] type associated with this table schema.
    type Record: TableRecord;
    /// The table schema associated with this record.
    type Schema: TableSchema<Record = Self::Record>;

    /// Creates an insert record from a list of column [`Value`]s.
    fn from_values(values: &[(ColumnDef, Value)]) -> DbmsResult<Self>;

    /// Converts the record into a list of column [`Value`]s for insertion.
    fn into_values(self) -> Vec<(ColumnDef, Value)>;

    /// Converts the insert record into the corresponding table record.
    fn into_record(self) -> Self::Schema;
}

/// This trait represents a record for updating a table.
pub trait UpdateRecord: Sized + Clone {
    /// The [`TableRecord`] type associated with this table schema.
    type Record: TableRecord;
    /// The table schema associated with this record.
    type Schema: TableSchema<Record = Self::Record>;

    /// Creates an update record from a list of column [`Value`]s and an optional [`Filter`] for the where clause.
    fn from_values(values: &[(ColumnDef, Value)], where_clause: Option<Filter>) -> Self;

    /// Get the list of column [`Value`]s to be updated.
    fn update_values(&self) -> Vec<(ColumnDef, Value)>;

    /// Get the [`Filter`] condition for the update operation.
    fn where_clause(&self) -> Option<Filter>;
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_should_create_values_source_this() {
        let source = ValuesSource::This;
        assert_eq!(source, ValuesSource::This);
    }

    #[test]
    fn test_should_create_values_source_foreign() {
        let source = ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        };

        if let ValuesSource::Foreign { table, column } = source {
            assert_eq!(table, "users");
            assert_eq!(column, "id");
        } else {
            panic!("expected ValuesSource::Foreign");
        }
    }

    #[test]
    fn test_should_clone_values_source() {
        let source = ValuesSource::Foreign {
            table: "posts".to_string(),
            column: "author_id".to_string(),
        };

        let cloned = source.clone();
        assert_eq!(source, cloned);
    }

    #[test]
    fn test_should_compare_values_sources() {
        let source1 = ValuesSource::This;
        let source2 = ValuesSource::This;
        let source3 = ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        };
        let source4 = ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        };
        let source5 = ValuesSource::Foreign {
            table: "posts".to_string(),
            column: "id".to_string(),
        };

        assert_eq!(source1, source2);
        assert_eq!(source3, source4);
        assert_ne!(source1, source3);
        assert_ne!(source3, source5);
    }

    #[test]
    fn test_should_hash_values_source() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ValuesSource::This);
        set.insert(ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        });

        assert!(set.contains(&ValuesSource::This));
        assert!(set.contains(&ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        }));
        assert!(!set.contains(&ValuesSource::Foreign {
            table: "posts".to_string(),
            column: "id".to_string(),
        }));
    }

    #[test]
    fn test_should_debug_values_source() {
        let source = ValuesSource::This;
        let debug_str = format!("{:?}", source);
        assert_eq!(debug_str, "This");

        let foreign = ValuesSource::Foreign {
            table: "users".to_string(),
            column: "id".to_string(),
        };
        let foreign_debug = format!("{:?}", foreign);
        assert!(foreign_debug.contains("Foreign"));
        assert!(foreign_debug.contains("users"));
        assert!(foreign_debug.contains("id"));
    }
}

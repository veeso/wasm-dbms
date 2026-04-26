//! This module exposes all the types related to queries that can be performed on the DBMS.

mod aggregate;
mod builder;
mod delete;
mod filter;
mod join;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use self::aggregate::{AggregateFunction, AggregatedRow, AggregatedValue};
pub use self::builder::QueryBuilder;
pub use self::delete::DeleteBehavior;
pub use self::filter::{Filter, JsonCmp, JsonFilter};
pub use self::join::{Join, JoinType};
use crate::dbms::table::TableSchema;
use crate::dbms::value::Value;
use crate::memory::MemoryError;

/// The result type for query operations.
pub type QueryResult<T> = Result<T, QueryError>;

/// An enum representing possible errors that can occur during query operations.
#[derive(Debug, Error, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum QueryError {
    /// The specified primary key value already exists in the table.
    #[error("Primary key conflict: record with the same primary key already exists")]
    PrimaryKeyConflict,

    /// A unique constraint was violated (e.g., UNIQUE index, CHECK constraint, etc.)
    #[error("Unique constraint violation on field '{field}'")]
    UniqueConstraintViolation { field: String },

    /// A foreign key references a non-existent record in another table.
    #[error("Broken foreign key reference to table '{table}' with key '{key:?}'")]
    BrokenForeignKeyReference { table: String, key: Value },

    /// Tried to delete or update a record that is referenced by another table's foreign key.
    #[error("Foreign key constraint violation on table '{referencing_table}' for field '{field}'")]
    ForeignKeyConstraintViolation {
        referencing_table: String,
        field: String,
    },

    /// Tried to reference a column that does not exist in the table schema.
    #[error("Unknown column: {0}")]
    UnknownColumn(String),

    /// Tried to insert a record missing non-nullable fields.
    #[error("Missing non-nullable field: {0}")]
    MissingNonNullableField(String),

    /// The specified transaction was not found or has expired.
    #[error("transaction not found")]
    TransactionNotFound,

    /// Query contains syntactically or semantically invalid conditions.
    #[error("Invalid query: {0}")]
    InvalidQuery(String),

    /// Join inside a typed select operation
    #[error("Join cannot be used on type select")]
    JoinInsideTypedSelect,

    /// `GROUP BY` or `HAVING` was set on a non-aggregate query.
    /// Use `Database::aggregate` instead.
    #[error("GROUP BY / HAVING require aggregate(); use Database::aggregate")]
    AggregateClauseInSelect,

    /// Generic constraint violation (e.g., UNIQUE, CHECK, etc.)
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),

    /// The memory allocator or memory manager failed to allocate or access stable memory.
    #[error("Memory error: {0}")]
    MemoryError(MemoryError),

    /// The table or schema was not found.
    #[error("Table not found: {0}")]
    TableNotFound(String),

    /// The record identified by the given key or filter does not exist.
    #[error("Record not found")]
    RecordNotFound,

    /// Any low-level IO or serialization/deserialization issue.
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Generic catch-all error (for internal, unexpected conditions).
    #[error("Internal error: {0}")]
    Internal(String),
}

/// An enum representing the fields to select in a query.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum Select {
    #[default]
    All,
    Columns(Vec<String>),
}

/// An enum representing the direction of ordering in a query.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum OrderDirection {
    Ascending,
    Descending,
}

/// A struct representing a query in the DBMS.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    /// Fields to select in the query.
    columns: Select,
    /// Distinct records by the specified fields.
    pub distinct_by: Vec<String>,
    /// Relations to eagerly load with the main records.
    pub eager_relations: Vec<String>,
    /// [`Filter`] to apply to the query.
    pub filter: Option<Filter>,
    /// Group by fields for aggregate queries.
    pub group_by: Vec<String>,
    /// Having [`Filter`] for aggregate queries.
    pub having: Option<Filter>,
    /// Join operations
    pub joins: Vec<Join>,
    /// Limit on the number of records to return.
    pub limit: Option<usize>,
    /// Offset for pagination.
    pub offset: Option<usize>,
    /// Order by clauses for sorting the results.
    pub order_by: Vec<(String, OrderDirection)>,
}

#[cfg(feature = "candid")]
impl candid::CandidType for Query {
    fn _ty() -> candid::types::Type {
        use candid::types::TypeInner;
        let mut fields = vec![
            candid::field! { columns: Select::_ty() },
            candid::field! { distinct_by: <Vec<String>>::_ty() },
            candid::field! { eager_relations: <Vec<String>>::_ty() },
            candid::field! { filter: <Option<Filter>>::_ty() },
            candid::field! { group_by: <Vec<String>>::_ty() },
            candid::field! { having: <Option<Filter>>::_ty() },
            candid::field! { joins: <Vec<Join>>::_ty() },
            candid::field! { limit: <Option<usize>>::_ty() },
            candid::field! { offset: <Option<usize>>::_ty() },
            candid::field! { order_by: <Vec<(String, OrderDirection)>>::_ty() },
        ];

        fields.sort_by_key(|f| f.id.clone());
        TypeInner::Record(fields).into()
    }

    fn idl_serialize<S>(&self, serializer: S) -> Result<(), S::Error>
    where
        S: candid::types::Serializer,
    {
        use candid::types::Compound;
        // Fields must be serialized in Candid field hash order. The order
        // below matches the ascending hash of each field name (idl_hash).
        let mut record_serializer = serializer.serialize_struct()?;
        record_serializer.serialize_element(&self.eager_relations)?;
        record_serializer.serialize_element(&self.distinct_by)?;
        record_serializer.serialize_element(&self.joins)?;
        record_serializer.serialize_element(&self.offset)?;
        record_serializer.serialize_element(&self.limit)?;
        record_serializer.serialize_element(&self.filter)?;
        record_serializer.serialize_element(&self.group_by)?;
        record_serializer.serialize_element(&self.having)?;
        record_serializer.serialize_element(&self.order_by)?;
        record_serializer.serialize_element(&self.columns)?;

        Ok(())
    }
}

impl Query {
    /// Creates a new [`QueryBuilder`] for building a query.
    pub fn builder() -> QueryBuilder {
        QueryBuilder::default()
    }

    /// Returns whether all columns are selected in the query.
    pub fn all_selected(&self) -> bool {
        matches!(self.columns, Select::All)
    }
    /// Returns the list of columns to be selected in the query.
    pub fn columns<T>(&self) -> Vec<String>
    where
        T: TableSchema,
    {
        match &self.columns {
            Select::All => T::columns()
                .iter()
                .map(|col| col.name.to_string())
                .collect(),
            Select::Columns(cols) => cols.clone(),
        }
    }

    /// Returns whether the query has any joins.
    pub fn has_joins(&self) -> bool {
        !self.joins.is_empty()
    }

    /// Returns the raw column names from the Select clause.
    ///
    /// Unlike `columns::<T>()`, this does not expand `Select::All`
    /// using the table schema.
    pub fn raw_columns(&self) -> &[String] {
        match &self.columns {
            Select::All => &[],
            Select::Columns(cols) => cols,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::tests::User;

    #[test]
    fn test_should_build_default_query() {
        let query = Query::default();
        assert!(matches!(query.columns, Select::All));
        assert!(query.eager_relations.is_empty());
        assert!(query.filter.is_none());
        assert!(query.order_by.is_empty());
        assert!(query.limit.is_none());
        assert!(query.offset.is_none());
    }

    #[test]
    fn test_should_get_columns() {
        let query = Query::default();
        let columns = query.columns::<User>();
        assert_eq!(columns, vec!["id", "name",]);

        let query = Query {
            columns: Select::Columns(vec!["id".to_string()]),
            ..Default::default()
        };

        let columns = query.columns::<User>();
        assert_eq!(columns, vec!["id"]);
    }

    #[test]
    fn test_should_check_all_selected() {
        let query = Query::default();
        assert!(query.all_selected());
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_encode_decode_query_candid() {
        let query = Query::builder()
            .field("id")
            .with("posts")
            .and_where(Filter::eq("name", Value::Text("Alice".into())))
            .order_by_asc("id")
            .limit(10)
            .offset(5)
            .build();
        let encoded = candid::encode_one(&query).unwrap();
        let decoded: Query = candid::decode_one(&encoded).unwrap();
        assert_eq!(query, decoded);
    }

    #[test]
    fn test_should_build_query_with_joins() {
        let query = Query::builder()
            .all()
            .inner_join("posts", "id", "user")
            .build();
        assert_eq!(query.joins.len(), 1);
        assert_eq!(query.joins[0].table, "posts");
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_encode_decode_query_with_joins_candid() {
        let query = Query::builder()
            .all()
            .inner_join("posts", "id", "user")
            .left_join("comments", "posts.id", "post_id")
            .and_where(Filter::eq("users.name", Value::Text("Alice".into())))
            .build();
        let encoded = candid::encode_one(&query).unwrap();
        let decoded: Query = candid::decode_one(&encoded).unwrap();
        assert_eq!(query, decoded);
    }

    #[test]
    fn test_default_query_has_empty_joins() {
        let query = Query::default();
        assert!(query.joins.is_empty());
        assert!(!query.has_joins());
    }

    #[test]
    fn test_has_joins() {
        let query = Query::builder()
            .all()
            .inner_join("posts", "id", "user")
            .build();
        assert!(query.has_joins());
    }

    #[test]
    fn test_raw_columns_returns_empty_for_all() {
        let query = Query::builder().all().build();
        assert!(query.raw_columns().is_empty());
    }

    #[test]
    fn test_raw_columns_returns_specified_columns() {
        let query = Query::builder().field("id").field("name").build();
        assert_eq!(query.raw_columns(), &["id", "name"]);
    }
}

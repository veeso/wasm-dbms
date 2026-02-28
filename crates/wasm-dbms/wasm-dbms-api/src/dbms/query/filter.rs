mod json_filter;
mod like;

use serde::{Deserialize, Serialize};

pub use self::json_filter::{JsonCmp, JsonFilter};
use crate::dbms::query::QueryResult;
use crate::dbms::table::ColumnDef;
use crate::dbms::types::Text;
use crate::dbms::value::Value;
use crate::prelude::QueryError;

/// [`super::Query`] filters.
///
/// The first value refers to the column name, and the second to the value to compare against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum Filter {
    Eq(String, Value),
    Ne(String, Value),
    Gt(String, Value),
    Lt(String, Value),
    Ge(String, Value),
    In(String, Vec<Value>),
    /// JSON filter applied to a column.
    Json(String, JsonFilter),
    Le(String, Value),
    Like(String, String),
    NotNull(String),
    IsNull(String),
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
    Not(Box<Filter>),
}

impl Filter {
    /// Creates an equality filter.
    pub fn eq(field: &str, value: Value) -> Self {
        Filter::Eq(field.to_string(), value)
    }

    /// Creates a not-equal filter.
    pub fn ne(field: &str, value: Value) -> Self {
        Filter::Ne(field.to_string(), value)
    }

    /// Creates a greater-than filter.
    pub fn gt(field: &str, value: Value) -> Self {
        Filter::Gt(field.to_string(), value)
    }

    /// Creates a less-than filter.
    pub fn lt(field: &str, value: Value) -> Self {
        Filter::Lt(field.to_string(), value)
    }

    /// Creates a greater-than-or-equal filter.
    pub fn ge(field: &str, value: Value) -> Self {
        Filter::Ge(field.to_string(), value)
    }

    /// Creates a less-than-or-equal filter.
    pub fn le(field: &str, value: Value) -> Self {
        Filter::Le(field.to_string(), value)
    }

    /// Creates an IN filter.
    pub fn in_list(field: &str, values: Vec<Value>) -> Self {
        Filter::In(field.to_string(), values)
    }

    /// Creates a LIKE filter.
    pub fn like(field: &str, pattern: &str) -> Self {
        Filter::Like(field.to_string(), pattern.to_string())
    }

    /// Creates a NOT NULL filter.
    pub fn not_null(field: &str) -> Self {
        Filter::NotNull(field.to_string())
    }

    /// Creates an IS NULL filter.
    pub fn is_null(field: &str) -> Self {
        Filter::IsNull(field.to_string())
    }

    /// Creates a JSON filter.
    pub fn json(field: &str, json_filter: JsonFilter) -> Self {
        Filter::Json(field.to_string(), json_filter)
    }

    /// Chain two filters with AND.
    pub fn and(self, other: Filter) -> Self {
        Filter::And(Box::new(self), Box::new(other))
    }

    /// Chain two filters with OR.
    pub fn or(self, other: Filter) -> Self {
        Filter::Or(Box::new(self), Box::new(other))
    }

    /// Negate a filter with NOT.
    #[allow(clippy::should_implement_trait)]
    pub fn not(self) -> Self {
        Filter::Not(Box::new(self))
    }

    /// Checks if the given joined row values match the filter.
    ///
    /// Each element in `table_groups` is a `(table_name, columns)` pair.
    /// Column references can be qualified ("table.column") or unqualified ("column").
    /// Unqualified names that exist in multiple tables produce an error.
    pub fn matches_joined_row(
        &self,
        table_groups: &[(&str, Vec<(ColumnDef, Value)>)],
    ) -> QueryResult<bool> {
        let res = match self {
            Filter::Eq(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v == value)
            }
            Filter::Ne(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v != value)
            }
            Filter::Gt(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v > value)
            }
            Filter::Lt(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v < value)
            }
            Filter::Ge(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v >= value)
            }
            Filter::Le(field, value) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v <= value)
            }
            Filter::In(field, list) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| list.iter().any(|item| item == v))
            }
            Filter::Json(field, json_filter) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                let json = col_value.and_then(|v| v.as_json()).ok_or_else(|| {
                    QueryError::InvalidQuery(format!("Column '{field}' is not a Json type"))
                })?;
                return json_filter.matches(json);
            }
            Filter::Like(field, pattern) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                if let Some(Value::Text(Text(text))) = col_value {
                    return Ok(like::Like::parse(pattern)
                        .map_err(|e| {
                            QueryError::InvalidQuery(format!("Invalid LIKE pattern {pattern}: {e}"))
                        })?
                        .matches(text));
                }
                if col_value.is_some() {
                    return Err(QueryError::InvalidQuery(
                        "LIKE operator can only be applied to Text values".to_string(),
                    ));
                }
                false
            }
            Filter::NotNull(field) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| !v.is_null())
            }
            Filter::IsNull(field) => {
                let col_value = Self::resolve_joined_column(field, table_groups)?;
                col_value.is_some_and(|v| v.is_null())
            }
            Filter::And(left, right) => {
                left.matches_joined_row(table_groups)? && right.matches_joined_row(table_groups)?
            }
            Filter::Or(left, right) => {
                left.matches_joined_row(table_groups)? || right.matches_joined_row(table_groups)?
            }
            Filter::Not(inner) => !inner.matches_joined_row(table_groups)?,
        };

        Ok(res)
    }

    /// Resolves a column reference against joined table groups.
    ///
    /// Qualified names ("table.column") are resolved directly.
    /// Unqualified names are searched across all groups; if found in
    /// more than one table, an ambiguity error is returned.
    fn resolve_joined_column<'a>(
        field: &str,
        table_groups: &'a [(&str, Vec<(ColumnDef, Value)>)],
    ) -> QueryResult<Option<&'a Value>> {
        if let Some((table, column)) = field.split_once('.') {
            // Qualified name: "table.column"
            let group = table_groups
                .iter()
                .find(|(t, _)| *t == table)
                .ok_or_else(|| {
                    QueryError::InvalidQuery(format!("Table '{table}' not in query scope"))
                })?;
            Ok(group
                .1
                .iter()
                .find(|(c, _)| c.name == column)
                .map(|(_, v)| v))
        } else {
            // Unqualified name: search all groups
            let mut found: Vec<&Value> = Vec::new();
            for (_, cols) in table_groups {
                for (col, val) in cols {
                    if col.name == field {
                        found.push(val);
                    }
                }
            }
            match found.len() {
                0 => Ok(None),
                1 => Ok(Some(found[0])),
                _ => Err(QueryError::InvalidQuery(format!(
                    "Ambiguous column '{field}': exists in multiple joined tables, qualify with table name"
                ))),
            }
        }
    }

    /// Checks if the given values match the filter.
    pub fn matches(&self, values: &[(ColumnDef, Value)]) -> QueryResult<bool> {
        let res = match self {
            Filter::Eq(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val == value),
            Filter::Ne(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val != value),
            Filter::Gt(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val > value),
            Filter::Lt(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val < value),
            Filter::Ge(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val >= value),
            Filter::Le(field, value) => values
                .iter()
                .any(|(col, val)| col.name == *field && val <= value),
            Filter::In(field, list) => values
                .iter()
                .any(|(col, val)| col.name == *field && list.iter().any(|v| v == val)),
            Filter::Json(field, json_filter) => {
                let json = values
                    .iter()
                    .find(|(col, _)| col.name == *field)
                    .and_then(|(_, val)| val.as_json())
                    .ok_or_else(|| {
                        QueryError::InvalidQuery(format!("Column '{field}' is not a Json type"))
                    })?;
                return json_filter.matches(json);
            }
            Filter::Like(field, pattern) => {
                for (col, val) in values {
                    if col.name == *field {
                        if let Value::Text(Text(text)) = val {
                            return Ok(like::Like::parse(pattern)
                                .map_err(|e| {
                                    QueryError::InvalidQuery(format!(
                                        "Invalid LIKE pattern {pattern}: {e}"
                                    ))
                                })?
                                .matches(text));
                        }
                        return Err(QueryError::InvalidQuery(
                            "LIKE operator can only be applied to Text values".to_string(),
                        ));
                    }
                }
                false
            }
            Filter::NotNull(field) => values
                .iter()
                .any(|(col, val)| col.name == *field && !val.is_null()),
            Filter::IsNull(field) => values
                .iter()
                .any(|(col, val)| col.name == *field && val.is_null()),
            Filter::And(left, right) => left.matches(values)? && right.matches(values)?,
            Filter::Or(left, right) => left.matches(values)? || right.matches(values)?,
            Filter::Not(inner) => !inner.matches(values)?,
        };

        Ok(res)
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::dbms::types::DataTypeKind;

    #[test]
    fn test_should_check_eq() {
        let filter = Filter::eq("id", Value::Int32(30.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(30.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(35.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_ne() {
        let filter = Filter::ne("id", Value::Int32(30.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(25.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(30.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_gt() {
        let filter = Filter::gt("id", Value::Int32(20.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(25.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(10.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_lt() {
        let filter = Filter::lt("id", Value::Int32(30.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(25.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(40.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_ge() {
        let filter = Filter::ge("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(25.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let filter = Filter::ge("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(30.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let filter = Filter::ge("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(20.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_le() {
        let filter = Filter::le("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(25.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let filter = Filter::le("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(20.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let filter = Filter::le("id", Value::Int32(25.into()));
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(35.into()),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_is_null() {
        let filter = Filter::is_null("name");
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Null,
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Alice".to_string())),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_not_null() {
        let filter = Filter::not_null("name");
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Alice".to_string())),
        )];
        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Null,
        )];
        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_like() {
        let filter = Filter::like("name", "%ohn%");
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Johnathan".to_string())),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Alice".to_string())),
        )];

        let result = filter.matches(&values).expect("LIKE match failed");
        assert!(!result);
    }

    #[test]
    fn test_should_raise_error_or_like_on_non_text() {
        let filter = Filter::like("age", "%30%");
        let values = vec![(
            ColumnDef {
                name: "age",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Int32(30.into()),
        )];

        let result = filter.matches(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_escape_like() {
        let filter = Filter::like("name", "100%% match");
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("100% match".to_string())),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);
    }

    #[test]
    fn test_should_check_and_or_not() {
        let filter = Filter::eq("id", Value::Int32(30.into()))
            .and(Filter::gt("age", Value::Int32(18.into())))
            .or(Filter::is_null("name").not());

        let values = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(30.into()),
            ),
            (
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(20.into()),
            ),
            (
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Alice".to_string())),
            ),
        ];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        // check false
        let values = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(25.into()),
            ),
            (
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(16.into()),
            ),
            (
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Null,
            ),
        ];
        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_not() {
        let filter = Filter::not_null("name").not();

        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Null,
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: true,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Bob".to_string())),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_in_list() {
        let filter = Filter::in_list(
            "id",
            vec![
                Value::Int32(10.into()),
                Value::Int32(20.into()),
                Value::Int32(30.into()),
            ],
        );
        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(20.into()),
        )];
        let result = filter.matches(&values).unwrap();
        assert!(result);

        let values = vec![(
            ColumnDef {
                name: "id",
                data_type: DataTypeKind::Int32,
                nullable: false,
                primary_key: true,
                foreign_key: None,
            },
            Value::Int32(40.into()),
        )];
        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_json_extract() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"user": {"name": "Alice", "age": 30}}"#).unwrap();
        let filter = Filter::json(
            "data",
            JsonFilter::extract_eq("user.name", Value::Text("Alice".into())),
        );
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);
    }

    #[test]
    fn test_should_check_json_extract_no_match() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"user": {"name": "Bob"}}"#).unwrap();
        let filter = Filter::json(
            "data",
            JsonFilter::extract_eq("user.name", Value::Text("Alice".into())),
        );
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_check_json_contains() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"active": true, "role": "admin"}"#).unwrap();
        let pattern = Json::from_str(r#"{"active": true}"#).unwrap();
        let filter = Filter::json("data", JsonFilter::contains(pattern));
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);
    }

    #[test]
    fn test_should_check_json_has_key() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"email": "alice@example.com"}"#).unwrap();
        let filter = Filter::json("data", JsonFilter::has_key("email"));
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        // Test missing key
        let json_value = Json::from_str(r#"{"name": "Alice"}"#).unwrap();
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    #[test]
    fn test_should_error_json_filter_on_non_json_column() {
        let filter = Filter::json("name", JsonFilter::has_key("email"));
        let values = vec![(
            ColumnDef {
                name: "name",
                data_type: DataTypeKind::Text,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Text(Text("Alice".to_string())),
        )];

        let result = filter.matches(&values);
        assert!(result.is_err());
    }

    #[test]
    fn test_should_check_json_combined_with_and() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"user": {"name": "Alice", "age": 25}}"#).unwrap();

        // has email key AND user.age > 18
        let filter = Filter::json("data", JsonFilter::has_key("user.name")).and(Filter::json(
            "data",
            JsonFilter::extract_gt("user.age", Value::Int64(18.into())),
        ));

        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];

        let result = filter.matches(&values).unwrap();
        assert!(result);
    }

    #[test]
    fn test_should_check_json_combined_with_or() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        // user.role = "admin" OR user.role = "moderator"
        let filter = Filter::json(
            "data",
            JsonFilter::extract_eq("role", Value::Text("admin".into())),
        )
        .or(Filter::json(
            "data",
            JsonFilter::extract_eq("role", Value::Text("moderator".into())),
        ));

        // Test admin
        let json_value = Json::from_str(r#"{"role": "admin"}"#).unwrap();
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];
        assert!(filter.matches(&values).unwrap());

        // Test moderator
        let json_value = Json::from_str(r#"{"role": "moderator"}"#).unwrap();
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];
        assert!(filter.matches(&values).unwrap());

        // Test user (should fail)
        let json_value = Json::from_str(r#"{"role": "user"}"#).unwrap();
        let values = vec![(
            ColumnDef {
                name: "data",
                data_type: DataTypeKind::Json,
                nullable: false,
                primary_key: false,
                foreign_key: None,
            },
            Value::Json(json_value),
        )];
        assert!(!filter.matches(&values).unwrap());
    }

    #[test]
    fn test_should_check_json_with_other_filters() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        // id = 1 AND data contains {"active": true}
        let pattern = Json::from_str(r#"{"active": true}"#).unwrap();
        let filter = Filter::eq("id", Value::Int32(1.into()))
            .and(Filter::json("data", JsonFilter::contains(pattern)));

        let json_value = Json::from_str(r#"{"active": true, "name": "Test"}"#).unwrap();
        let values = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            ),
            (
                ColumnDef {
                    name: "data",
                    data_type: DataTypeKind::Json,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Json(json_value),
            ),
        ];

        let result = filter.matches(&values).unwrap();
        assert!(result);

        // Test with wrong id
        let json_value = Json::from_str(r#"{"active": true}"#).unwrap();
        let values = vec![
            (
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(2.into()),
            ),
            (
                ColumnDef {
                    name: "data",
                    data_type: DataTypeKind::Json,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Json(json_value),
            ),
        ];

        let result = filter.matches(&values).unwrap();
        assert!(!result);
    }

    // === matches_joined_row tests ===

    #[test]
    fn test_should_match_qualified_column_name() {
        let filter = Filter::eq("users.id", Value::Int32(1.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_unqualified_column_in_joined_row() {
        let filter = Filter::eq("title", Value::Text(Text("Hello".to_string())));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![
            (
                "users",
                vec![(
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Int32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    Value::Int32(1.into()),
                )],
            ),
            (
                "posts",
                vec![(
                    ColumnDef {
                        name: "title",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text(Text("Hello".to_string())),
                )],
            ),
        ];
        assert!(filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_error_on_ambiguous_column_in_joined_row() {
        let filter = Filter::eq("id", Value::Int32(1.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![
            (
                "users",
                vec![(
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Int32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    Value::Int32(1.into()),
                )],
            ),
            (
                "posts",
                vec![(
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Int32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    Value::Int32(2.into()),
                )],
            ),
        ];
        assert!(filter.matches_joined_row(&values).is_err());
    }

    #[test]
    fn test_should_match_and_filter_on_joined_row() {
        let filter = Filter::eq("users.id", Value::Int32(1.into())).and(Filter::eq(
            "posts.title",
            Value::Text(Text("Hello".to_string())),
        ));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![
            (
                "users",
                vec![(
                    ColumnDef {
                        name: "id",
                        data_type: DataTypeKind::Int32,
                        nullable: false,
                        primary_key: true,
                        foreign_key: None,
                    },
                    Value::Int32(1.into()),
                )],
            ),
            (
                "posts",
                vec![(
                    ColumnDef {
                        name: "title",
                        data_type: DataTypeKind::Text,
                        nullable: false,
                        primary_key: false,
                        foreign_key: None,
                    },
                    Value::Text(Text("Hello".to_string())),
                )],
            ),
        ];
        assert!(filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_error_on_unknown_table_in_joined_row() {
        let filter = Filter::eq("unknown.id", Value::Int32(1.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).is_err());
    }

    #[test]
    fn test_should_match_ne_filter_on_joined_row() {
        let filter = Filter::ne("users.id", Value::Int32(99.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_gt_filter_on_joined_row() {
        let filter = Filter::gt("users.age", Value::Int32(18.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(25.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::gt("users.age", Value::Int32(30.into()));
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_lt_filter_on_joined_row() {
        let filter = Filter::lt("users.age", Value::Int32(30.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(25.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::lt("users.age", Value::Int32(20.into()));
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_ge_filter_on_joined_row() {
        let filter = Filter::ge("users.age", Value::Int32(25.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(25.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::ge("users.age", Value::Int32(26.into()));
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_le_filter_on_joined_row() {
        let filter = Filter::le("users.age", Value::Int32(25.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "age",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Int32(25.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::le("users.age", Value::Int32(24.into()));
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_in_filter_on_joined_row() {
        let filter = Filter::in_list(
            "users.id",
            vec![
                Value::Int32(1.into()),
                Value::Int32(2.into()),
                Value::Int32(3.into()),
            ],
        );
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(2.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::in_list(
            "users.id",
            vec![Value::Int32(10.into()), Value::Int32(20.into())],
        );
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_like_filter_on_joined_row() {
        let filter = Filter::like("posts.title", "%ello%");
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "posts",
            vec![(
                ColumnDef {
                    name: "title",
                    data_type: DataTypeKind::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Hello World".to_string())),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let filter = Filter::like("posts.title", "%xyz%");
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_error_like_on_non_text_in_joined_row() {
        let filter = Filter::like("users.id", "%1%");
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).is_err());
    }

    #[test]
    fn test_should_match_not_null_filter_on_joined_row() {
        let filter = Filter::not_null("users.name");
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Alice".to_string())),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Null,
            )],
        )];
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_is_null_filter_on_joined_row() {
        let filter = Filter::is_null("users.name");
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Null,
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Alice".to_string())),
            )],
        )];
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_or_filter_on_joined_row() {
        let filter = Filter::eq("users.id", Value::Int32(1.into()))
            .or(Filter::eq("users.id", Value::Int32(2.into())));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(2.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(99.into()),
            )],
        )];
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_not_filter_on_joined_row() {
        let filter = Filter::eq("users.id", Value::Int32(1.into())).not();
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(99.into()),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());

        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_match_json_filter_on_joined_row() {
        use std::str::FromStr;

        use crate::dbms::types::Json;

        let json_value = Json::from_str(r#"{"user": {"name": "Alice", "age": 30}}"#).unwrap();
        let filter = Filter::json(
            "posts.data",
            JsonFilter::extract_eq("user.name", Value::Text("Alice".into())),
        );
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "posts",
            vec![(
                ColumnDef {
                    name: "data",
                    data_type: DataTypeKind::Json,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Json(json_value),
            )],
        )];
        assert!(filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_error_json_filter_on_non_json_in_joined_row() {
        let filter = Filter::json("users.name", JsonFilter::has_key("email"));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: false,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("Alice".to_string())),
            )],
        )];
        assert!(filter.matches_joined_row(&values).is_err());
    }

    #[test]
    fn test_should_return_false_for_missing_column_in_joined_row() {
        let filter = Filter::eq("users.nonexistent", Value::Int32(1.into()));
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "id",
                    data_type: DataTypeKind::Int32,
                    nullable: false,
                    primary_key: true,
                    foreign_key: None,
                },
                Value::Int32(1.into()),
            )],
        )];
        assert!(!filter.matches_joined_row(&values).unwrap());
    }

    #[test]
    fn test_should_return_false_like_on_missing_column_in_joined_row() {
        let filter = Filter::like("users.nonexistent", "%test%");
        let values: Vec<(&str, Vec<(ColumnDef, Value)>)> = vec![(
            "users",
            vec![(
                ColumnDef {
                    name: "name",
                    data_type: DataTypeKind::Text,
                    nullable: true,
                    primary_key: false,
                    foreign_key: None,
                },
                Value::Text(Text("hello".to_string())),
            )],
        )];
        // LIKE on a missing column returns false
        assert!(!filter.matches_joined_row(&values).unwrap());
    }
}

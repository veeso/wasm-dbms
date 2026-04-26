//! Types for aggregated queries

use serde::{Deserialize, Serialize};

use crate::prelude::Value;

/// An aggregate function applied to a column in a query.
///
/// Each variant maps to a SQL aggregate (`COUNT`, `SUM`, `AVG`, `MIN`, `MAX`).
/// All variants except [`AggregateFunction::Count`] take the name of the
/// column to aggregate over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum AggregateFunction {
    /// Count rows in the group.
    ///
    /// `None` is equivalent to `COUNT(*)` and counts every row.
    /// `Some(column)` is equivalent to `COUNT(column)` and counts only rows
    /// where `column` is non-null.
    Count(Option<String>),
    /// Sum the values of the given column (`SUM(column)`).
    Sum(String),
    /// Arithmetic mean of the given column (`AVG(column)`).
    Avg(String),
    /// Minimum value of the given column (`MIN(column)`).
    Min(String),
    /// Maximum value of the given column (`MAX(column)`).
    Max(String),
}

/// Result of a single aggregate function applied to a group.
///
/// Each variant corresponds to a variant of [`AggregateFunction`]. `Count` is
/// always a `u64`; the remaining variants wrap a [`Value`] whose concrete
/// kind matches the aggregated column's data type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum AggregatedValue {
    /// Number of rows counted by [`AggregateFunction::Count`].
    Count(u64),
    /// Sum produced by [`AggregateFunction::Sum`].
    Sum(Value),
    /// Average produced by [`AggregateFunction::Avg`].
    Avg(Value),
    /// Minimum produced by [`AggregateFunction::Min`].
    Min(Value),
    /// Maximum produced by [`AggregateFunction::Max`].
    Max(Value),
}

/// A single row of aggregated query results.
///
/// One `AggregatedRow` is produced per distinct grouping tuple. `values`
/// contains one [`AggregatedValue`] per requested aggregate, in the same
/// order as the [`AggregateFunction`] list passed to the query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct AggregatedRow {
    /// Values of the `GROUP BY` columns that identify this group, in the
    /// order the columns were declared.
    pub group_keys: Vec<Value>,
    /// Aggregate results, one per [`AggregateFunction`] in the query.
    pub values: Vec<AggregatedValue>,
}

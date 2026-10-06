//! Binds a parsed statement to a schema.
//!
//! The planner resolves table and column names, converts literals and
//! parameters to column types, and produces the [`Query`], [`Filter`], and
//! column values that the DBMS API takes.

mod aggregate;
mod coerce;
mod condition;
mod dml;
mod scope;
mod select;
#[cfg(test)]
mod tests;

use wasm_dbms_api::prelude::{
    AggregateFunction, ColumnDef, DeleteBehavior, Filter, JoinColumnDef, Query,
};

pub(crate) use self::dml::{delete, insert, update};
pub(crate) use self::select::select;

/// Looks up the column definitions of a table by name.
///
/// Returns `None` when the schema has no such table.
pub(crate) type Catalog<'a> = dyn Fn(&str) -> Option<&'static [ColumnDef]> + 'a;

/// How to run a `SELECT`.
#[derive(Debug)]
pub(crate) enum SelectPlan {
    /// Read one table.
    Table(TablePlan),
    /// Read several joined tables.
    Join(TablePlan),
    /// Compute aggregates over one table.
    Aggregate(AggregatePlan),
}

/// A row query on one table or on a join.
#[derive(Debug)]
pub(crate) struct TablePlan {
    /// The `FROM` table.
    pub(crate) table: String,
    /// The query to run; it always reads every column.
    pub(crate) query: Query,
    /// The columns to return, in order; `None` returns every column.
    pub(crate) projection: Option<Vec<OutputColumn>>,
}

/// One column of the result of a [`TablePlan`].
#[derive(Debug)]
pub(crate) struct OutputColumn {
    /// The table the column is read from; `Some` only for a join.
    pub(crate) table: Option<String>,
    /// The column to read.
    pub(crate) column: String,
    /// The name of the column in the result.
    pub(crate) name: String,
}

/// An aggregate query on one table.
#[derive(Debug)]
pub(crate) struct AggregatePlan {
    /// The `FROM` table.
    pub(crate) table: String,
    /// The query carrying `WHERE`, `GROUP BY`, `HAVING`, `ORDER BY`, and pagination.
    pub(crate) query: Query,
    /// The aggregates to compute; `agg{N}` in `query` refers to entry `N`.
    pub(crate) aggregates: Vec<AggregateFunction>,
    /// The columns to return, in order.
    pub(crate) outputs: Vec<AggregateOutput>,
}

/// One column of the result of an [`AggregatePlan`].
#[derive(Debug)]
pub(crate) struct AggregateOutput {
    /// Where the value comes from in an aggregated row.
    pub(crate) source: AggregateSource,
    /// The definition of the column in the result.
    pub(crate) def: JoinColumnDef,
}

/// The origin of an aggregate output value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AggregateSource {
    /// The `GROUP BY` key at this index.
    GroupKey(usize),
    /// The aggregate at this index.
    Aggregate(usize),
}

/// How to run an `INSERT`.
#[derive(Debug)]
pub(crate) struct InsertPlan {
    pub(crate) table: String,
    /// The listed columns with their values.
    pub(crate) values: Vec<(ColumnDef, wasm_dbms_api::prelude::Value)>,
}

/// How to run an `UPDATE`.
#[derive(Debug)]
pub(crate) struct UpdatePlan {
    pub(crate) table: String,
    /// The assigned columns with their new values.
    pub(crate) values: Vec<(ColumnDef, wasm_dbms_api::prelude::Value)>,
    pub(crate) filter: Filter,
}

/// How to run a `DELETE`.
#[derive(Debug)]
pub(crate) struct DeletePlan {
    pub(crate) table: String,
    pub(crate) filter: Filter,
    pub(crate) behavior: DeleteBehavior,
}

//! SQL execution against a `wasm-dbms` database.

#[cfg(test)]
mod tests;

use wasm_dbms::prelude::{DatabaseSchema, DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{
    AggregatedRow, AggregatedValue, ColumnDef, Database as _, JoinColumnDef, JoinResultSet,
    SqlError, SqlResult, SqlRow, TransactionId, Value,
};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::ast::Statement;
use crate::parser::parse;
use crate::planner::{self, AggregateOutput, AggregateSource, OutputColumn, SelectPlan};

/// Runs SQL statements against a `wasm-dbms` database.
///
/// The engine holds the database schema and nothing else. It does not borrow
/// the [`DbmsContext`]: the context is passed to every
/// [`execute`](Self::execute) call, so one engine can serve several contexts
/// and can be stored next to its context, for example in a `thread_local!`.
///
/// # Transactions
///
/// Transactions are addressed by their [`TransactionId`], the same ids that
/// [`DbmsContext::begin_transaction`] returns. `BEGIN` opens a transaction and
/// returns its id in [`SqlResult::TxBegin`]; passing that id to later calls
/// runs them inside the transaction, where they see its uncommitted writes.
/// `COMMIT` and `ROLLBACK` close the transaction whose id they receive. A
/// statement that fails leaves the transaction open. Calls without an id are
/// applied immediately and do not see open transactions.
///
/// The engine does not check who holds an id. An application layer that
/// serves several identities keeps its own ledger from id to identity and
/// checks it before calling `execute` with that id.
///
/// # Examples
///
/// ```rust,ignore
/// use wasm_dbms::prelude::*;
/// use wasm_dbms_api::prelude::*;
/// use wasm_dbms_memory::prelude::HeapMemoryProvider;
/// use wasm_dbms_sql::SqlEngine;
///
/// #[derive(Clone, DatabaseSchema)]
/// #[tables(User = "users")]
/// pub struct MySchema;
///
/// let ctx = DbmsContext::new(HeapMemoryProvider::default());
/// MySchema::register_tables(&ctx)?;
/// let engine = SqlEngine::new(MySchema);
///
/// engine.execute(
///     &ctx,
///     None,
///     "INSERT INTO users (id, name) VALUES (?, ?)",
///     &[Value::from(1u32), Value::from("Alice")],
/// )?;
///
/// let SqlResult::TxBegin(tx) = engine.execute(&ctx, None, "BEGIN", &[])? else {
///     unreachable!("BEGIN returns TxBegin");
/// };
/// engine.execute(&ctx, Some(tx), "UPDATE users SET name = 'Alicia' WHERE id = 1", &[])?;
/// engine.execute(&ctx, Some(tx), "COMMIT", &[])?;
/// ```
pub struct SqlEngine<S> {
    schema: S,
}

impl<S> SqlEngine<S> {
    /// Creates an engine for the tables of `schema`.
    pub fn new(schema: S) -> Self {
        Self { schema }
    }

    /// Parses and runs one SQL statement, outside or inside a transaction.
    ///
    /// `tx` selects where the statement runs: `None` applies it immediately
    /// through [`WasmDbmsDatabase::oneshot`], `Some(id)` runs it through
    /// [`WasmDbmsDatabase::from_transaction`]. `BEGIN` requires `None` and
    /// returns the new id in [`SqlResult::TxBegin`]; `COMMIT` and `ROLLBACK`
    /// require `Some(id)`. `params` supplies one value per `?` placeholder, in
    /// order.
    ///
    /// # Errors
    ///
    /// - [`SqlError::Parse`] and [`SqlError::MissingWhereClause`] when `sql`
    ///   is not a valid statement.
    /// - [`SqlError::ParameterCountMismatch`] when `params` does not have one
    ///   value per placeholder.
    /// - [`SqlError::UnknownTable`], [`SqlError::UnknownColumn`], and
    ///   [`SqlError::AmbiguousColumn`] when a name does not resolve.
    /// - [`SqlError::TypeMismatch`] and [`SqlError::InvalidLiteral`] when a
    ///   value cannot be converted to the type of its column.
    /// - [`SqlError::Unsupported`] when the statement combines features the
    ///   DBMS cannot run together.
    /// - [`SqlError::TransactionAlreadyActive`] for `BEGIN` with `Some(id)`.
    /// - [`SqlError::NoActiveTransaction`] for `COMMIT` or `ROLLBACK` with
    ///   `None`.
    /// - [`SqlError::Runtime`] when the DBMS rejects the operation, for
    ///   example on a constraint violation, or when `id` does not name an
    ///   open transaction.
    pub fn execute<'ctx, M>(
        &self,
        ctx: &'ctx DbmsContext<M>,
        tx: Option<TransactionId>,
        sql: &str,
        params: &[Value],
    ) -> Result<SqlResult, SqlError>
    where
        M: MemoryProvider,
        S: DatabaseSchema<M> + Clone + 'ctx,
    {
        let parsed = parse(sql)?;
        if parsed.parameter_count != params.len() {
            return Err(SqlError::ParameterCountMismatch {
                expected: parsed.parameter_count,
                got: params.len(),
            });
        }
        match (&parsed.statement, tx) {
            (Statement::Begin, None) => Ok(SqlResult::TxBegin(ctx.begin_transaction())),
            (Statement::Begin, Some(_)) => Err(SqlError::TransactionAlreadyActive),
            (Statement::Commit | Statement::Rollback, None) => Err(SqlError::NoActiveTransaction),
            (Statement::Commit, Some(id)) => {
                WasmDbmsDatabase::from_transaction(ctx, self.schema.clone(), id).commit()?;
                Ok(SqlResult::TxCommit)
            }
            (Statement::Rollback, Some(id)) => {
                WasmDbmsDatabase::from_transaction(ctx, self.schema.clone(), id).rollback()?;
                Ok(SqlResult::TxRollback)
            }
            (statement, Some(id)) => self.run(
                &WasmDbmsDatabase::from_transaction(ctx, self.schema.clone(), id),
                statement,
                params,
            ),
            (statement, None) => self.run(
                &WasmDbmsDatabase::oneshot(ctx, self.schema.clone()),
                statement,
                params,
            ),
        }
    }

    /// Plans and runs a data statement on `db`.
    fn run<M>(
        &self,
        db: &WasmDbmsDatabase<'_, M>,
        statement: &Statement,
        params: &[Value],
    ) -> Result<SqlResult, SqlError>
    where
        M: MemoryProvider,
        S: DatabaseSchema<M>,
    {
        let catalog = |table: &str| self.schema.table_columns(table).ok();
        match statement {
            Statement::Select(select) => {
                let rows = match planner::select(select, &catalog, params)? {
                    SelectPlan::Table(plan) => db
                        .select_raw(&plan.table, plan.query)?
                        .into_iter()
                        .map(|row| table_row(row, plan.projection.as_deref()))
                        .collect(),
                    SelectPlan::Join(plan) => join_rows(
                        db.select_join(&plan.table, plan.query)?,
                        plan.projection.as_deref(),
                    ),
                    SelectPlan::Aggregate(plan) => self
                        .schema
                        .aggregate(db, &plan.table, plan.query, &plan.aggregates)?
                        .into_iter()
                        .map(|row| aggregate_row(&row, &plan.outputs))
                        .collect(),
                };
                Ok(SqlResult::Rows(rows))
            }
            Statement::Insert(insert) => {
                let plan = planner::insert(insert, &catalog, params)?;
                let table = self.schema.static_table_name(&plan.table)?;
                self.schema.insert(db, table, &plan.values)?;
                Ok(SqlResult::RowsAffected(1))
            }
            Statement::Update(update) => {
                let plan = planner::update(update, &catalog, params)?;
                let table = self.schema.static_table_name(&plan.table)?;
                let count = self
                    .schema
                    .update(db, table, &plan.values, Some(plan.filter))?;
                Ok(SqlResult::RowsAffected(count))
            }
            Statement::Delete(delete) => {
                let plan = planner::delete(delete, &catalog, params)?;
                let table = self.schema.static_table_name(&plan.table)?;
                let count = self
                    .schema
                    .delete(db, table, plan.behavior, Some(plan.filter))?;
                Ok(SqlResult::RowsAffected(count))
            }
            Statement::Begin | Statement::Commit | Statement::Rollback => {
                unreachable!("transaction statements are handled by `execute`")
            }
        }
    }
}

impl<S> std::fmt::Debug for SqlEngine<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlEngine").finish_non_exhaustive()
    }
}

/// Shapes a row of a single-table query.
fn table_row(row: Vec<(ColumnDef, Value)>, projection: Option<&[OutputColumn]>) -> SqlRow {
    let Some(projection) = projection else {
        return row
            .into_iter()
            .map(|(def, value)| (JoinColumnDef::from(def), value))
            .collect();
    };
    projection
        .iter()
        .map(|output| {
            let (def, value) = row
                .iter()
                .find(|(def, _)| def.name == output.column)
                .expect("planned column must exist in the selected table row");
            let mut def = JoinColumnDef::from(*def);
            def.name = output.name.clone();
            (def, value.clone())
        })
        .collect()
}

/// Shapes a join result into SQL rows, applying the select-list projection.
///
/// The projection is resolved once against the column list; every row is
/// then built by index.
fn join_rows(result: JoinResultSet, projection: Option<&[OutputColumn]>) -> Vec<SqlRow> {
    let JoinResultSet { columns, rows } = result;
    let outputs: Vec<(usize, JoinColumnDef)> = match projection {
        None => columns.into_iter().enumerate().collect(),
        Some(projection) => projection
            .iter()
            .map(|output| {
                let index = columns
                    .iter()
                    .position(|def| def.table == output.table && def.name == output.column)
                    .expect("planned column must exist in the selected join row");
                let mut def = columns[index].clone();
                def.name = output.name.clone();
                (index, def)
            })
            .collect(),
    };

    rows.into_iter()
        .map(|row| {
            outputs
                .iter()
                .map(|(index, def)| (def.clone(), row[*index].clone()))
                .collect()
        })
        .collect()
}

/// Shapes an aggregated row into the columns of the select list.
fn aggregate_row(row: &AggregatedRow, outputs: &[AggregateOutput]) -> SqlRow {
    outputs
        .iter()
        .map(|output| {
            let value = match output.source {
                AggregateSource::GroupKey(index) => row
                    .group_keys
                    .get(index)
                    .cloned()
                    .expect("planned group key must exist in the aggregated row"),
                AggregateSource::Aggregate(index) => row
                    .values
                    .get(index)
                    .map(|value| match value {
                        AggregatedValue::Count(count) => Value::from(*count),
                        AggregatedValue::Sum(value)
                        | AggregatedValue::Avg(value)
                        | AggregatedValue::Min(value)
                        | AggregatedValue::Max(value) => value.clone(),
                    })
                    .expect("planned aggregate must exist in the aggregated row"),
            };
            (output.def.clone(), value)
        })
        .collect()
}

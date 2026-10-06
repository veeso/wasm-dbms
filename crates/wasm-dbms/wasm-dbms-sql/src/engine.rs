//! SQL execution against a `wasm-dbms` database.

#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;

use wasm_dbms::prelude::{ContextId, DatabaseSchema, DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{
    AggregatedRow, AggregatedValue, ColumnDef, Database as _, DbmsError, JoinColumnDef, SqlError,
    SqlResult, SqlRow, TransactionError, TransactionId, Value,
};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::ast::Statement;
use crate::parser::parse;
use crate::planner::{self, AggregateOutput, AggregateSource, OutputColumn, SelectPlan};

/// Runs SQL statements against a `wasm-dbms` database.
///
/// The engine owns the database schema and the SQL transaction of each
/// caller in each context. It does not borrow the [`DbmsContext`]: the context is passed to
/// every [`execute`](Self::execute) call, so an engine can be stored next to
/// its context, for example in a `thread_local!`, and keep transactions open
/// across calls.
///
/// # Transactions
///
/// `BEGIN` opens a transaction for the caller that issued it in the supplied
/// context. Until that
/// caller issues `COMMIT` or `ROLLBACK`, its statements run inside the
/// transaction and see its uncommitted writes; other callers do not. A
/// statement that fails leaves the transaction open. Statements of a caller
/// without a transaction are applied immediately.
///
/// SQL transactions are separate from the transactions started with
/// [`DbmsContext::begin_transaction`]: a unit of work should use one or the
/// other.
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
///     b"alice",
///     "INSERT INTO users (id, name) VALUES (?, ?)",
///     &[Value::from(1u32), Value::from("Alice")],
/// )?;
/// let result = engine.execute(&ctx, b"alice", "SELECT name FROM users WHERE id = 1", &[])?;
/// ```
pub struct SqlEngine<S> {
    schema: S,
    /// The open SQL transaction of each caller in each DBMS context.
    transactions: RefCell<HashMap<(ContextId, Vec<u8>), TransactionId>>,
}

impl<S> SqlEngine<S> {
    /// Creates an engine for the tables of `schema`.
    pub fn new(schema: S) -> Self {
        Self {
            schema,
            transactions: RefCell::new(HashMap::new()),
        }
    }

    /// Returns whether `caller` has a SQL transaction open in `ctx`.
    pub fn in_transaction<M>(&self, ctx: &DbmsContext<M>, caller: &[u8]) -> bool
    where
        M: MemoryProvider,
    {
        self.evict_closed_transaction(ctx, caller);
        self.transactions
            .borrow()
            .contains_key(&(ctx.id(), caller.to_vec()))
    }

    /// Parses and runs one SQL statement on behalf of `caller`.
    ///
    /// `caller` identifies who issued the statement; together with `ctx`, it
    /// selects the SQL transaction the statement runs in. `params` supplies
    /// one value per `?` placeholder, in order.
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
    /// - [`SqlError::TransactionAlreadyActive`] for `BEGIN` inside a
    ///   transaction.
    /// - [`SqlError::Runtime`] when the DBMS rejects the operation, for
    ///   example on a constraint violation, or on `COMMIT` or `ROLLBACK`
    ///   without a transaction.
    pub fn execute<'ctx, M>(
        &self,
        ctx: &'ctx DbmsContext<M>,
        caller: &[u8],
        sql: &str,
        params: &[Value],
    ) -> Result<SqlResult, SqlError>
    where
        M: MemoryProvider,
        S: DatabaseSchema<M> + Clone + 'ctx,
    {
        self.evict_closed_transaction(ctx, caller);
        let parsed = parse(sql)?;
        if parsed.parameter_count != params.len() {
            return Err(SqlError::ParameterCountMismatch {
                expected: parsed.parameter_count,
                got: params.len(),
            });
        }
        match &parsed.statement {
            Statement::Begin => self.begin(ctx, caller),
            Statement::Commit => {
                self.end_transaction(ctx, caller, |db| db.commit())?;
                Ok(SqlResult::TxCommit)
            }
            Statement::Rollback => {
                self.end_transaction(ctx, caller, |db| db.rollback())?;
                Ok(SqlResult::TxRollback)
            }
            statement => {
                let transaction = self
                    .transactions
                    .borrow()
                    .get(&(ctx.id(), caller.to_vec()))
                    .copied();
                let db = match transaction {
                    Some(id) => WasmDbmsDatabase::from_transaction(ctx, self.schema.clone(), id),
                    None => WasmDbmsDatabase::oneshot(ctx, self.schema.clone()),
                };
                self.run(&db, statement, params)
            }
        }
    }

    fn begin<M>(&self, ctx: &DbmsContext<M>, caller: &[u8]) -> Result<SqlResult, SqlError>
    where
        M: MemoryProvider,
    {
        let key = (ctx.id(), caller.to_vec());
        let mut transactions = self.transactions.borrow_mut();
        if transactions.contains_key(&key) {
            return Err(SqlError::TransactionAlreadyActive);
        }
        let id = ctx.begin_transaction(caller.to_vec());
        transactions.insert(key, id);
        Ok(SqlResult::TxBegin)
    }

    /// Forgets transactions whose context or transaction session no longer exists.
    fn evict_closed_transaction<M>(&self, ctx: &DbmsContext<M>, caller: &[u8])
    where
        M: MemoryProvider,
    {
        let key = (ctx.id(), caller.to_vec());
        let mut transactions = self.transactions.borrow_mut();
        transactions.retain(|(context_id, _), _| context_id.is_alive());
        let id = transactions.get(&key).copied();
        if id.is_some_and(|id| !ctx.has_transaction(&id, caller)) {
            transactions.remove(&key);
        }
    }

    /// Commits or rolls back the transaction of `caller` with `finish`.
    ///
    /// The caller's transaction is forgotten once the DBMS has consumed it,
    /// which it also does when a commit fails while applying the changes.
    fn end_transaction<'ctx, M, F>(
        &self,
        ctx: &'ctx DbmsContext<M>,
        caller: &[u8],
        finish: F,
    ) -> Result<(), SqlError>
    where
        M: MemoryProvider,
        S: DatabaseSchema<M> + Clone + 'ctx,
        F: FnOnce(&mut WasmDbmsDatabase<'ctx, M>) -> Result<(), DbmsError>,
    {
        let key = (ctx.id(), caller.to_vec());
        let id = self
            .transactions
            .borrow()
            .get(&key)
            .copied()
            .ok_or(DbmsError::Transaction(
                TransactionError::NoActiveTransaction,
            ))?;
        let mut db = WasmDbmsDatabase::from_transaction(ctx, self.schema.clone(), id);
        let result = finish(&mut db);
        if !ctx.has_transaction(&id, caller) {
            self.transactions.borrow_mut().remove(&key);
        }
        result.map_err(SqlError::from)
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
                    SelectPlan::Join(plan) => db
                        .select_join(&plan.table, plan.query)?
                        .into_iter()
                        .map(|row| join_row(row, plan.projection.as_deref()))
                        .collect(),
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
        f.debug_struct("SqlEngine")
            .field("active_transactions", &self.transactions.borrow().len())
            .finish_non_exhaustive()
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

/// Shapes a row of a join query, whose columns carry their table name.
fn join_row(row: SqlRow, projection: Option<&[OutputColumn]>) -> SqlRow {
    let Some(projection) = projection else {
        return row;
    };
    projection
        .iter()
        .map(|output| {
            let (def, value) = row
                .iter()
                .find(|(def, _)| def.table == output.table && def.name == output.column)
                .expect("planned column must exist in the selected join row");
            let mut def = def.clone();
            def.name = output.name.clone();
            (def, value.clone())
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

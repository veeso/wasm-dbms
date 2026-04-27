use crate::error::DbmsResult;
use crate::prelude::{
    AggregateFunction, AggregatedRow, ColumnDef, DeleteBehavior, Filter, InsertRecord,
    JoinColumnDef, MigrationOp, MigrationPolicy, Query, TableSchema, UpdateRecord, Value,
};

/// CRUD, aggregate, and transaction operations exposed by a wasm-dbms session.
///
/// One implementation lives in `wasm-dbms` (`WasmDbmsDatabase`); IC adapters
/// wrap that implementation behind canister endpoints. Methods that take a
/// [`Query`] honour its `WHERE`, `DISTINCT`, `ORDER BY`, `OFFSET`, `LIMIT`,
/// and (for [`aggregate`](Self::aggregate)) `GROUP BY` / `HAVING` clauses.
///
/// All methods return [`DbmsResult`]; the error variants worth handling at
/// each call site are listed under each method's `# Errors` section.
pub trait Database {
    /// Runs a typed `SELECT` for table `T` and decodes each row into `T::Record`.
    ///
    /// The query's primary key column is always included in the returned
    /// records even when the caller restricts `Select::Columns` to other
    /// fields, so the `Record` shape is always reconstructible.
    ///
    /// # Arguments
    ///
    /// - `query` - The [`Query`] to execute. Must not contain joins; use
    ///   [`Database::select_join`] for joined queries.
    ///
    /// # Returns
    ///
    /// A `Vec<T::Record>` containing one entry per matching row, in the order
    /// produced by the query pipeline (see the
    /// [Query API reference](crate::prelude::Query)).
    ///
    /// # Errors
    ///
    /// - [`QueryError::JoinInsideTypedSelect`] — the query contains joins.
    /// - [`QueryError::AggregateClauseInSelect`] — `group_by` or `having` is
    ///   set; call [`aggregate`](Self::aggregate) instead.
    /// - [`QueryError::UnknownColumn`] — a referenced column does not exist
    ///   on `T`.
    /// - [`QueryError::TableNotFound`] — `T::table_name()` was never registered.
    ///
    /// [`QueryError::JoinInsideTypedSelect`]: crate::prelude::QueryError::JoinInsideTypedSelect
    /// [`QueryError::AggregateClauseInSelect`]: crate::prelude::QueryError::AggregateClauseInSelect
    /// [`QueryError::UnknownColumn`]: crate::prelude::QueryError::UnknownColumn
    /// [`QueryError::TableNotFound`]: crate::prelude::QueryError::TableNotFound
    fn select<T>(&self, query: Query) -> DbmsResult<Vec<T::Record>>
    where
        T: TableSchema;

    /// Runs a `SELECT` against a table identified by name, returning raw
    /// column-value pairs instead of typed records.
    ///
    /// Useful when the caller does not know the table type at compile time
    /// (for example, on the IC canister boundary). Same execution pipeline as
    /// [`select`](Self::select); same restrictions apply.
    ///
    /// # Arguments
    ///
    /// - `table` - Table name as registered with the schema.
    /// - `query` - The [`Query`] to execute. Must not contain joins.
    ///
    /// # Returns
    ///
    /// A `Vec` of rows, where each row is a `Vec<(ColumnDef, Value)>` with
    /// one entry per selected column.
    ///
    /// # Errors
    ///
    /// - [`QueryError::TableNotFound`] — `table` is not registered.
    /// - [`QueryError::AggregateClauseInSelect`] — `group_by` or `having` is set.
    /// - [`QueryError::UnknownColumn`] — a referenced column does not exist.
    ///
    /// [`QueryError::TableNotFound`]: crate::prelude::QueryError::TableNotFound
    /// [`QueryError::AggregateClauseInSelect`]: crate::prelude::QueryError::AggregateClauseInSelect
    /// [`QueryError::UnknownColumn`]: crate::prelude::QueryError::UnknownColumn
    fn select_raw(&self, table: &str, query: Query) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>>;

    /// Runs a join query starting from `table`, returning rows with
    /// [`JoinColumnDef`] entries that carry the source table name.
    ///
    /// Use `table.column` syntax in [`field`](crate::prelude::QueryBuilder::field),
    /// [`and_where`](crate::prelude::QueryBuilder::and_where),
    /// [`or_where`](crate::prelude::QueryBuilder::or_where), and `order_by_*`
    /// to disambiguate columns that share names across joined tables.
    /// Unqualified names default to the `table` argument.
    ///
    /// # Arguments
    ///
    /// - `table` - The driving (`FROM`) table for the join.
    /// - `query` - The [`Query`] to execute. Must include at least one join via
    ///   [`QueryBuilder::inner_join`](crate::prelude::QueryBuilder::inner_join)
    ///   or its variants.
    ///
    /// # Returns
    ///
    /// A `Vec` of rows, where each row is a `Vec<(JoinColumnDef, Value)>`
    /// containing columns from every joined table in the query.
    ///
    /// # Errors
    ///
    /// - [`QueryError::TableNotFound`] — `table` or any joined table is not
    ///   registered.
    /// - [`QueryError::AggregateClauseInSelect`] — `group_by` or `having` is set.
    /// - [`QueryError::InvalidQuery`] — an ambiguous unqualified column appears
    ///   in multiple joined tables.
    ///
    /// [`QueryError::TableNotFound`]: crate::prelude::QueryError::TableNotFound
    /// [`QueryError::AggregateClauseInSelect`]: crate::prelude::QueryError::AggregateClauseInSelect
    /// [`QueryError::InvalidQuery`]: crate::prelude::QueryError::InvalidQuery
    fn select_join(
        &self,
        table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(JoinColumnDef, Value)>>>;

    /// Runs an aggregate query for table `T`, computing the requested
    /// aggregate functions per group.
    ///
    /// Pipeline: `WHERE` -> `DISTINCT` -> bucket rows by [`Query::group_by`]
    /// -> compute each [`AggregateFunction`] per bucket -> apply
    /// [`Query::having`] -> apply `ORDER BY` -> apply `OFFSET` / `LIMIT`. When
    /// `group_by` is empty all matching rows form one group, producing at most
    /// one [`AggregatedRow`].
    ///
    /// `HAVING` and `ORDER BY` may reference any column listed in `group_by`
    /// or any aggregate output by its synthetic name `agg{N}` (`agg0` is the
    /// first entry of `aggregates`, `agg1` the second, ...).
    ///
    /// # Arguments
    ///
    /// - `query` - The [`Query`] providing `WHERE`, `DISTINCT`, `GROUP BY`,
    ///   `HAVING`, `ORDER BY`, `LIMIT`, and `OFFSET`. Joins and eager
    ///   relations are rejected.
    /// - `aggregates` - The aggregate functions to compute per group, in the
    ///   order they should appear in [`AggregatedRow::values`].
    ///
    /// # Returns
    ///
    /// One [`AggregatedRow`] per distinct grouping tuple. Empty when every
    /// group is filtered out by `HAVING` or when no rows survive `WHERE`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::UnknownColumn`] — `group_by` or an aggregate references
    ///   a column not on `T`.
    /// - [`QueryError::InvalidQuery`] — `SUM` or `AVG` on a non-numeric column,
    ///   `HAVING` / `ORDER BY` references an unknown `agg{N}` or column,
    ///   `LIKE` or JSON filter inside `HAVING`, or query carries joins or
    ///   eager relations.
    ///
    /// [`QueryError::UnknownColumn`]: crate::prelude::QueryError::UnknownColumn
    /// [`QueryError::InvalidQuery`]: crate::prelude::QueryError::InvalidQuery
    fn aggregate<T>(
        &self,
        query: Query,
        aggregates: &[AggregateFunction],
    ) -> DbmsResult<Vec<AggregatedRow>>
    where
        T: TableSchema;

    /// Inserts a single record into table `T`.
    ///
    /// Auto-increment columns left unset are filled before insertion.
    /// Sanitizers run on each column before validators; insert-time integrity
    /// checks (primary key uniqueness, `#[unique]` constraints, foreign-key
    /// existence) are evaluated before the row is written.
    ///
    /// Outside a transaction the write is journaled and applied atomically
    /// against stable storage; inside a transaction the write goes to the
    /// transaction overlay and becomes visible to subsequent reads on the
    /// same transaction.
    ///
    /// # Arguments
    ///
    /// - `record` - The insert payload, typically built from
    ///   `T::Insert::from_values(...)` or the `*InsertRequest` struct
    ///   generated by `#[derive(Table)]`.
    ///
    /// # Errors
    ///
    /// - [`QueryError::PrimaryKeyConflict`] — the row's PK already exists.
    /// - [`QueryError::UniqueConstraintViolation`] — a `#[unique]` column
    ///   collides with an existing row.
    /// - [`QueryError::BrokenForeignKeyReference`] — a foreign key points at
    ///   a row that does not exist.
    /// - [`QueryError::MissingNonNullableField`] — a required column was
    ///   omitted.
    /// - [`DbmsError::Validation`] / [`DbmsError::Sanitize`] — a column
    ///   validator or sanitizer rejected the value.
    ///
    /// [`QueryError::PrimaryKeyConflict`]: crate::prelude::QueryError::PrimaryKeyConflict
    /// [`QueryError::UniqueConstraintViolation`]: crate::prelude::QueryError::UniqueConstraintViolation
    /// [`QueryError::BrokenForeignKeyReference`]: crate::prelude::QueryError::BrokenForeignKeyReference
    /// [`QueryError::MissingNonNullableField`]: crate::prelude::QueryError::MissingNonNullableField
    /// [`DbmsError::Validation`]: crate::prelude::DbmsError
    /// [`DbmsError::Sanitize`]: crate::prelude::DbmsError
    fn insert<T>(&self, record: T::Insert) -> DbmsResult<()>
    where
        T: TableSchema,
        T::Insert: InsertRecord<Schema = T>;

    /// Updates rows of table `T` matching the patch's `where_clause`.
    ///
    /// The set of columns to write and the row predicate are both carried by
    /// `patch` (see [`UpdateRecord`]); a missing `where_clause` updates every
    /// row. Sanitizers and validators run on the patched values, and integrity
    /// checks (unique, foreign-key, etc.) are re-evaluated for each updated
    /// row.
    ///
    /// Updating a primary-key column cascades the new value to every
    /// referencing row's foreign key.
    ///
    /// Outside a transaction the update is journaled and applied atomically;
    /// inside a transaction the change is staged on the transaction overlay.
    ///
    /// # Arguments
    ///
    /// - `patch` - The update payload (typically a `*UpdateRequest` generated
    ///   by `#[derive(Table)]`) containing the new column values and the
    ///   `where_clause` filter.
    ///
    /// # Returns
    ///
    /// Number of rows updated. A return of `0` means no row matched the
    /// filter — not an error.
    ///
    /// # Errors
    ///
    /// - [`QueryError::PrimaryKeyConflict`] — updating the PK collides with
    ///   an existing row.
    /// - [`QueryError::UniqueConstraintViolation`] — the new value collides
    ///   with another row's `#[unique]` column.
    /// - [`QueryError::BrokenForeignKeyReference`] — a new FK value points at
    ///   a non-existent parent row.
    ///
    /// [`QueryError::PrimaryKeyConflict`]: crate::prelude::QueryError::PrimaryKeyConflict
    /// [`QueryError::UniqueConstraintViolation`]: crate::prelude::QueryError::UniqueConstraintViolation
    /// [`QueryError::BrokenForeignKeyReference`]: crate::prelude::QueryError::BrokenForeignKeyReference
    fn update<T>(&self, patch: T::Update) -> DbmsResult<u64>
    where
        T: TableSchema,
        T::Update: UpdateRecord<Schema = T>;

    /// Deletes rows of table `T` matching `filter`.
    ///
    /// `behaviour` controls the foreign-key handling:
    /// [`DeleteBehavior::Restrict`] aborts the delete if any other row
    /// references the target, while [`DeleteBehavior::Cascade`] also deletes
    /// the referencing rows recursively.
    ///
    /// A `None` filter targets every row in the table.
    ///
    /// Outside a transaction the delete (and any cascade) is journaled and
    /// applied atomically; inside a transaction the deletion is staged on the
    /// overlay.
    ///
    /// # Arguments
    ///
    /// - `behaviour` - Foreign-key handling: [`DeleteBehavior::Restrict`] or
    ///   [`DeleteBehavior::Cascade`].
    /// - `filter` - Predicate selecting rows to delete; `None` matches every
    ///   row.
    ///
    /// # Returns
    ///
    /// Total rows deleted, including rows removed by cascade. `0` means no
    /// row matched the filter.
    ///
    /// # Errors
    ///
    /// - [`QueryError::ForeignKeyConstraintViolation`] — a referenced row
    ///   exists and `behaviour` is [`DeleteBehavior::Restrict`].
    /// - [`QueryError::UnknownColumn`] — `filter` references a column not on
    ///   `T`.
    ///
    /// [`QueryError::ForeignKeyConstraintViolation`]: crate::prelude::QueryError::ForeignKeyConstraintViolation
    /// [`QueryError::UnknownColumn`]: crate::prelude::QueryError::UnknownColumn
    fn delete<T>(&self, behaviour: DeleteBehavior, filter: Option<Filter>) -> DbmsResult<u64>
    where
        T: TableSchema;

    /// Commits the active transaction, replaying its operations against
    /// stable storage under a single write-ahead journal.
    ///
    /// The transaction handle is consumed regardless of outcome. On success
    /// every staged insert/update/delete is durably applied; on operation
    /// failure the journal is rolled back, leaving stable storage untouched
    /// before the error propagates to the caller.
    ///
    /// # Errors
    ///
    /// - [`TransactionError::NoActiveTransaction`] — no transaction was
    ///   started on this session.
    /// - Any [`QueryError`] raised by the staged operations during replay
    ///   (constraint violations, missing FKs, etc.).
    ///
    /// # Panics
    ///
    /// Panics only if the rollback that follows a failed staged operation
    /// itself fails — at that point stable memory is in an irrecoverable
    /// state (M-PANIC-ON-BUG).
    ///
    /// [`TransactionError::NoActiveTransaction`]: crate::prelude::TransactionError::NoActiveTransaction
    /// [`QueryError`]: crate::prelude::QueryError
    fn commit(&mut self) -> DbmsResult<()>;

    /// Discards every staged operation in the active transaction without
    /// touching stable storage, and consumes the transaction handle.
    ///
    /// # Errors
    ///
    /// - [`TransactionError::NoActiveTransaction`] — no transaction was
    ///   started on this session.
    ///
    /// [`TransactionError::NoActiveTransaction`]: crate::prelude::TransactionError::NoActiveTransaction
    fn rollback(&mut self) -> DbmsResult<()>;

    /// Returns `true` iff the compiled schema differs from the snapshots
    /// persisted in stable memory.
    ///
    /// `O(1)` after the first call thanks to a per-context cache. Implementors
    /// gate every CRUD entry on this flag so callers can rely on the boolean
    /// to decide whether a migration is required before doing any work.
    ///
    /// # Errors
    ///
    /// Propagates [`DbmsError::Memory`](crate::prelude::DbmsError::Memory) when
    /// the persisted snapshots cannot be read.
    fn has_drift(&self) -> DbmsResult<bool>;

    /// Returns the migration ops needed to bring the on-disk schema in line
    /// with the compiled schema, without applying anything.
    ///
    /// Always recomputes the diff — there is no cache; the call is rare
    /// (typically once before [`Self::migrate`]) and the result depends on
    /// runtime state the cache cannot track. Safe to call while drift is
    /// active.
    ///
    /// # Errors
    ///
    /// Propagates the same [`DbmsError`](crate::prelude::DbmsError) variants
    /// the migration diff produces, plus
    /// [`DbmsError::Memory`](crate::prelude::DbmsError::Memory) when persisted
    /// snapshots cannot be read.
    fn pending_migrations(&self) -> DbmsResult<Vec<MigrationOp>>;

    /// Applies a planned migration under `policy`.
    ///
    /// Plans the diff, sorts the ops into deterministic apply order, validates
    /// against `policy`, then executes inside the implementation's journaled
    /// atomic block. On success the drift cache is cleared so subsequent CRUD
    /// calls pass; on failure the journal rolls back and the drift flag stays
    /// set.
    ///
    /// # Errors
    ///
    /// - [`MigrationError::DestructiveOpDenied`](crate::prelude::MigrationError::DestructiveOpDenied)
    ///   when the planner emits a destructive op disallowed by `policy`.
    /// - [`MigrationError::DataRewriteUnsupported`](crate::prelude::MigrationError::DataRewriteUnsupported)
    ///   for column-mutating ops not yet implemented (see issue #91).
    /// - Any other [`MigrationError`](crate::prelude::MigrationError) variant
    ///   raised by the diff or apply pipeline.
    fn migrate(&mut self, policy: MigrationPolicy) -> DbmsResult<()>;
}

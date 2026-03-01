// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS, M-PANIC-ON-BUG

//! Core DBMS database struct providing CRUD and transaction operations.

use std::cmp::Ordering;
use std::collections::HashSet;

use wasm_dbms_api::prelude::{
    CandidColumnDef, ColumnDef, DataTypeKind, Database, DbmsError, DbmsResult, DeleteBehavior,
    Filter, ForeignFetcher, ForeignKeyDef, InsertRecord, OrderDirection, Query, QueryError,
    TableColumns, TableError, TableRecord, TableSchema, TransactionError, TransactionId,
    UpdateRecord, Value, ValuesSource,
};
use wasm_dbms_memory::prelude::{
    AccessControl, AccessControlList, MemoryProvider, NextRecord, TableRegistry,
};

use crate::context::DbmsContext;
use crate::schema::DatabaseSchema;
use crate::transaction::journal::{Journal, JournaledWriter};
use crate::transaction::{DatabaseOverlay, Transaction, TransactionOp};

/// Default capacity for SELECT queries.
const DEFAULT_SELECT_CAPACITY: usize = 128;

/// The main DBMS database struct, generic over `MemoryProvider` and
/// `AccessControl`.
///
/// This struct borrows from a [`DbmsContext`] and provides all CRUD
/// operations, transaction management, and query execution.
pub struct WasmDbmsDatabase<'ctx, M, A = AccessControlList>
where
    M: MemoryProvider,
    A: AccessControl,
{
    /// Reference to the DBMS context owning all state.
    ctx: &'ctx DbmsContext<M, A>,
    /// Schema for dynamic dispatch of table operations.
    schema: Box<dyn DatabaseSchema<M, A> + 'ctx>,
    /// Active transaction ID, if any.
    transaction: Option<TransactionId>,
}

impl<'ctx, M, A> WasmDbmsDatabase<'ctx, M, A>
where
    M: MemoryProvider,
    A: AccessControl,
{
    /// Creates a one-shot (non-transactional) database instance.
    pub fn oneshot(ctx: &'ctx DbmsContext<M, A>, schema: impl DatabaseSchema<M, A> + 'ctx) -> Self {
        Self {
            ctx,
            schema: Box::new(schema),
            transaction: None,
        }
    }

    /// Creates a transactional database instance.
    pub fn from_transaction(
        ctx: &'ctx DbmsContext<M, A>,
        schema: impl DatabaseSchema<M, A> + 'ctx,
        transaction_id: TransactionId,
    ) -> Self {
        Self {
            ctx,
            schema: Box::new(schema),
            transaction: Some(transaction_id),
        }
    }

    /// Executes a closure with a mutable reference to the current transaction.
    fn with_transaction_mut<F, R>(&self, f: F) -> DbmsResult<R>
    where
        F: FnOnce(&mut Transaction) -> DbmsResult<R>,
    {
        let txid = self.transaction.as_ref().ok_or(DbmsError::Transaction(
            TransactionError::NoActiveTransaction,
        ))?;

        let mut ts = self.ctx.transaction_session.borrow_mut();
        let tx = ts.get_transaction_mut(txid)?;
        f(tx)
    }

    /// Executes a closure with a reference to the current transaction.
    fn with_transaction<F, R>(&self, f: F) -> DbmsResult<R>
    where
        F: FnOnce(&Transaction) -> DbmsResult<R>,
    {
        let txid = self.transaction.as_ref().ok_or(DbmsError::Transaction(
            TransactionError::NoActiveTransaction,
        ))?;

        let ts = self.ctx.transaction_session.borrow();
        let tx = ts.get_transaction(txid)?;
        f(tx)
    }

    /// Executes a closure atomically using a write-ahead journal.
    ///
    /// All writes performed inside `f` are recorded. On success the journal
    /// is committed (entries discarded). On error the journal is rolled back,
    /// restoring every modified byte to its pre-call state.
    ///
    /// When a journal is already active (e.g., inside [`Database::commit`]),
    /// this method delegates to the outer journal and does not manage its own.
    ///
    /// # Panics
    ///
    /// Panics if the rollback itself fails, because a failed rollback leaves
    /// memory in an irrecoverably corrupt state (M-PANIC-ON-BUG).
    fn atomic<F, R>(&self, f: F) -> DbmsResult<R>
    where
        F: FnOnce(&WasmDbmsDatabase<'ctx, M, A>) -> DbmsResult<R>,
    {
        let nested = self.ctx.journal.borrow().is_some();
        if !nested {
            *self.ctx.journal.borrow_mut() = Some(Journal::new());
        }
        match f(self) {
            Ok(res) => {
                if !nested
                    && let Some(journal) = self.ctx.journal.borrow_mut().take()
                {
                    journal.commit();
                }
                Ok(res)
            }
            Err(err) => {
                if !nested
                    && let Some(journal) = self.ctx.journal.borrow_mut().take()
                {
                    journal
                        .rollback(&mut self.ctx.mm.borrow_mut())
                        .expect("critical: failed to rollback journal");
                }
                Err(err)
            }
        }
    }

    /// Checks whether any foreign key references exist for the given record.
    ///
    /// Returns `true` if at least one referencing row exists in any table.
    fn has_foreign_key_references<T>(
        &self,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<bool>
    where
        T: TableSchema,
    {
        let pk = Self::extract_pk::<T>(record_values)?;

        for (table, columns) in self.schema.referenced_tables(T::table_name()) {
            for column in columns.iter() {
                let filter = Filter::eq(column, pk.clone());
                let query = Query::builder().field(column).filter(Some(filter)).build();
                let rows = self.schema.select(self, table, query)?;
                if !rows.is_empty() {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Deletes foreign key related records recursively for cascade deletes.
    fn delete_foreign_keys_cascade<T>(
        &self,
        record_values: &[(ColumnDef, Value)],
    ) -> DbmsResult<u64>
    where
        T: TableSchema,
    {
        let pk = Self::extract_pk::<T>(record_values)?;

        let mut count = 0;
        for (table, columns) in self.schema.referenced_tables(T::table_name()) {
            for column in columns.iter() {
                let filter = Filter::eq(column, pk.clone());
                let res = self
                    .schema
                    .delete(self, table, DeleteBehavior::Cascade, Some(filter))?;
                count += res;
            }
        }
        Ok(count)
    }

    /// Extracts the primary key value from a record's column-value pairs.
    fn extract_pk<T>(record_values: &[(ColumnDef, Value)]) -> DbmsResult<Value>
    where
        T: TableSchema,
    {
        record_values
            .iter()
            .find(|(col_def, _)| col_def.primary_key)
            .ok_or(DbmsError::Query(QueryError::UnknownColumn(
                T::primary_key().to_string(),
            )))
            .map(|(_, v)| v.clone())
    }

    /// Retrieves the current overlay from the active transaction.
    fn overlay(&self) -> DbmsResult<DatabaseOverlay> {
        self.with_transaction(|tx| Ok(tx.overlay().clone()))
    }

    /// Returns whether the record matches the provided filter.
    fn record_matches_filter(
        &self,
        record_values: &[(ColumnDef, Value)],
        filter: &Filter,
    ) -> DbmsResult<bool> {
        filter.matches(record_values).map_err(DbmsError::from)
    }

    /// Filters record columns down to only the selected fields.
    fn apply_column_selection<T>(&self, results: &mut [TableColumns], query: &Query)
    where
        T: TableSchema,
    {
        if query.all_selected() {
            return;
        }
        let selected_columns = query.columns::<T>();
        results
            .iter_mut()
            .flat_map(|record| record.iter_mut())
            .filter(|(source, _)| *source == ValuesSource::This)
            .for_each(|(_, cols)| {
                cols.retain(|(col_def, _)| selected_columns.contains(&col_def.name.to_string()));
            });
    }

    /// Batch-fetches eager relations for collected results.
    fn batch_load_eager_relations<T>(
        &self,
        results: &mut [TableColumns],
        query: &Query,
    ) -> DbmsResult<()>
    where
        T: TableSchema,
    {
        if query.eager_relations.is_empty() {
            return Ok(());
        }

        let fetcher = T::foreign_fetcher();

        for relation in &query.eager_relations {
            let fk_columns = Self::collect_fk_values::<T>(results, relation)?;

            for (local_column, pk_values) in &fk_columns {
                let batch_map = fetcher.fetch_batch(self, relation, pk_values)?;

                Self::verify_fk_batch(&batch_map, pk_values, relation)?;
                Self::attach_foreign_data(results, &batch_map, relation, local_column);
            }
        }

        Ok(())
    }

    /// Collects distinct FK values across all records for a given relation.
    fn collect_fk_values<T>(
        results: &[TableColumns],
        relation: &str,
    ) -> DbmsResult<Vec<(&'static str, Vec<Value>)>>
    where
        T: TableSchema,
    {
        let mut fk_columns: Vec<(&'static str, HashSet<Value>)> = vec![];

        for record_columns in results {
            let Some(cols) = Self::this_columns(record_columns) else {
                continue;
            };

            let mut found_fk = false;
            for (col_def, value) in cols {
                let Some(fk) = &col_def.foreign_key else {
                    continue;
                };
                if *fk.foreign_table != *relation {
                    continue;
                }

                found_fk = true;
                match fk_columns.iter_mut().find(|(lc, _)| *lc == fk.local_column) {
                    Some((_, values)) => {
                        values.insert(value.clone());
                    }
                    None => {
                        let mut set = HashSet::new();
                        set.insert(value.clone());
                        fk_columns.push((fk.local_column, set));
                    }
                }
            }

            if !found_fk {
                return Err(DbmsError::Query(QueryError::InvalidQuery(format!(
                    "Cannot load relation '{relation}' for table '{}': no foreign key found",
                    T::table_name()
                ))));
            }
        }

        Ok(fk_columns
            .into_iter()
            .map(|(col, set)| (col, set.into_iter().collect()))
            .collect())
    }

    /// Verifies all FK values were found in the batch result.
    fn verify_fk_batch(
        batch_map: &std::collections::HashMap<Value, Vec<(ColumnDef, Value)>>,
        pk_values: &[Value],
        relation: &str,
    ) -> DbmsResult<()> {
        if let Some(missing) = pk_values.iter().find(|v| !batch_map.contains_key(v)) {
            return Err(DbmsError::Query(QueryError::BrokenForeignKeyReference {
                table: relation.to_string(),
                key: missing.clone(),
            }));
        }
        Ok(())
    }

    /// Attaches batch-fetched foreign data to each record.
    fn attach_foreign_data(
        results: &mut [TableColumns],
        batch_map: &std::collections::HashMap<Value, Vec<(ColumnDef, Value)>>,
        relation: &str,
        local_column: &str,
    ) {
        for record_columns in results.iter_mut() {
            let fk_value = Self::this_columns(record_columns).and_then(|cols| {
                cols.iter().find_map(|(col_def, value)| {
                    let fk = col_def.foreign_key.as_ref()?;
                    (fk.foreign_table == relation && fk.local_column == local_column)
                        .then(|| value.clone())
                })
            });

            let Some(fk_val) = fk_value else { continue };
            let Some(foreign_values) = batch_map.get(&fk_val) else {
                continue;
            };

            record_columns.push((
                ValuesSource::Foreign {
                    table: relation.to_string(),
                    column: local_column.to_string(),
                },
                foreign_values.clone(),
            ));
        }
    }

    /// Extracts the `ValuesSource::This` columns from a record.
    fn this_columns(
        record: &[(ValuesSource, Vec<(ColumnDef, Value)>)],
    ) -> Option<&Vec<(ColumnDef, Value)>> {
        record
            .iter()
            .find(|(src, _)| *src == ValuesSource::This)
            .map(|(_, cols)| cols)
    }

    /// Retrieves existing primary keys matching a filter.
    fn existing_primary_keys_for_filter<T>(&self, filter: Option<Filter>) -> DbmsResult<Vec<Value>>
    where
        T: TableSchema,
    {
        let pk = T::primary_key();
        let query = Query::builder().field(pk).filter(filter).build();
        let fields = self.select::<T>(query)?;
        let pks = fields
            .into_iter()
            .map(|record| {
                record
                    .to_values()
                    .into_iter()
                    .find(|(col_def, _value)| col_def.name == pk)
                    .expect("primary key not found")
                    .1
            })
            .collect::<Vec<Value>>();

        Ok(pks)
    }

    /// Loads the table registry for a given table schema.
    fn load_table_registry<T>(&self) -> DbmsResult<TableRegistry>
    where
        T: TableSchema,
    {
        let sr = self.ctx.schema_registry.borrow();
        let registry_pages = sr
            .table_registry_page::<T>()
            .ok_or(DbmsError::Table(TableError::TableNotFound))?;

        let mm = self.ctx.mm.borrow();
        TableRegistry::load(registry_pages, &*mm).map_err(DbmsError::from)
    }

    /// Sorts query results by a column.
    fn sort_query_results(
        &self,
        results: &mut [TableColumns],
        column: &str,
        direction: OrderDirection,
    ) {
        results.sort_by(|a, b| {
            fn get_value<'a>(
                values: &'a [(ValuesSource, Vec<(ColumnDef, Value)>)],
                column: &str,
            ) -> Option<&'a Value> {
                values
                    .iter()
                    .find(|(source, _)| *source == ValuesSource::This)
                    .and_then(|(_, cols)| {
                        cols.iter()
                            .find(|(col_def, _)| col_def.name == column)
                            .map(|(_, value)| value)
                    })
            }

            let a_value = get_value(a, column);
            let b_value = get_value(b, column);

            sort_values_with_direction(a_value, b_value, direction)
        });
    }

    /// Core select logic returning intermediate `TableColumns`.
    #[doc(hidden)]
    pub fn select_columns<T>(&self, query: Query) -> DbmsResult<Vec<TableColumns>>
    where
        T: TableSchema,
    {
        let table_registry = self.load_table_registry::<T>()?;
        let mut table_overlay = if self.transaction.is_some() {
            self.overlay()?
        } else {
            DatabaseOverlay::default()
        };

        let mut results = Vec::with_capacity(query.limit.unwrap_or(DEFAULT_SELECT_CAPACITY));
        let mut count = 0;

        {
            let mm = self.ctx.mm.borrow();
            let table_reader = table_registry.read::<T, _>(&*mm);
            let mut table_reader = table_overlay.reader(table_reader);

            while let Some(values) = table_reader.try_next()? {
                if let Some(filter) = &query.filter
                    && !self.record_matches_filter(&values, filter)?
                {
                    continue;
                }
                count += 1;
                if query.offset.is_some_and(|offset| count <= offset) {
                    continue;
                }
                results.push(vec![(ValuesSource::This, values)]);
                if query.limit.is_some_and(|limit| results.len() >= limit) {
                    break;
                }
            }
        }

        self.batch_load_eager_relations::<T>(&mut results, &query)?;
        self.apply_column_selection::<T>(&mut results, &query);

        for (column, direction) in query.order_by.into_iter().rev() {
            self.sort_query_results(&mut results, &column, direction);
        }

        Ok(results)
    }

    /// Executes a join query.
    #[doc(hidden)]
    pub fn select_join(
        &self,
        table: &str,
        query: Query,
    ) -> DbmsResult<Vec<Vec<(CandidColumnDef, Value)>>> {
        self.schema.select_join(self, table, query)
    }

    /// Updates primary key references in tables referencing the updated table.
    fn update_pk_referencing_updated_table<T>(
        &self,
        old_pk: Value,
        new_pk: Value,
        data_type: DataTypeKind,
        pk_name: &'static str,
    ) -> DbmsResult<u64>
    where
        T: TableSchema,
    {
        let mut count = 0;
        for (ref_table, ref_col) in self
            .schema
            .referenced_tables(T::table_name())
            .into_iter()
            .flat_map(|(ref_table, ref_cols)| {
                ref_cols
                    .into_iter()
                    .map(move |ref_col| (ref_table, ref_col))
            })
        {
            let ref_patch_value = (
                ColumnDef {
                    name: ref_col,
                    data_type,
                    nullable: false,
                    primary_key: false,
                    foreign_key: Some(ForeignKeyDef {
                        foreign_table: T::table_name(),
                        foreign_column: pk_name,
                        local_column: ref_col,
                    }),
                },
                new_pk.clone(),
            );
            let filter = Filter::eq(ref_col, old_pk.clone());

            count += self
                .schema
                .update(self, ref_table, &[ref_patch_value], Some(filter))?;
        }

        Ok(count)
    }

    /// Sanitizes values using the table schema's sanitizers.
    fn sanitize_values<T>(
        &self,
        values: Vec<(ColumnDef, Value)>,
    ) -> DbmsResult<Vec<(ColumnDef, Value)>>
    where
        T: TableSchema,
    {
        let mut sanitized_values = Vec::with_capacity(values.len());
        for (col_def, value) in values.into_iter() {
            let value = match T::sanitizer(col_def.name) {
                Some(sanitizer) => sanitizer.sanitize(value)?,
                None => value,
            };
            sanitized_values.push((col_def, value));
        }
        Ok(sanitized_values)
    }

    /// Collects all records matching a filter from the table registry.
    #[allow(clippy::type_complexity)]
    fn collect_matching_records<T>(
        &self,
        table_registry: &TableRegistry,
        filter: &Option<Filter>,
    ) -> DbmsResult<Vec<(NextRecord<T>, Vec<(ColumnDef, Value)>)>>
    where
        T: TableSchema,
    {
        let mm = self.ctx.mm.borrow();
        let mut table_reader = table_registry.read::<T, _>(&*mm);
        let mut records = vec![];
        while let Some(values) = table_reader.try_next()? {
            let record_values = values.record.clone().to_values();
            if let Some(filter) = filter
                && !self.record_matches_filter(&record_values, filter)?
            {
                continue;
            }
            records.push((values, record_values));
        }
        Ok(records)
    }
}

/// Provides ordering for two optional values by direction.
pub fn sort_values_with_direction(
    a: Option<&Value>,
    b: Option<&Value>,
    direction: OrderDirection,
) -> Ordering {
    match (a, b) {
        (Some(a_val), Some(b_val)) => match direction {
            OrderDirection::Ascending => a_val.cmp(b_val),
            OrderDirection::Descending => b_val.cmp(a_val),
        },
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Converts column-value pairs to a schema entity.
fn values_to_schema_entity<T>(values: Vec<(ColumnDef, Value)>) -> DbmsResult<T>
where
    T: TableSchema,
{
    let record = T::Insert::from_values(&values)?.into_record();
    Ok(record)
}

impl<M, A> Database for WasmDbmsDatabase<'_, M, A>
where
    M: MemoryProvider,
    A: AccessControl,
{
    fn select<T>(&self, query: Query) -> DbmsResult<Vec<T::Record>>
    where
        T: TableSchema,
    {
        if !query.joins.is_empty() {
            return Err(DbmsError::Query(QueryError::JoinInsideTypedSelect));
        }
        let results = self.select_columns::<T>(query)?;
        Ok(results.into_iter().map(T::Record::from_values).collect())
    }

    fn select_raw(&self, table: &str, query: Query) -> DbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        self.schema.select(self, table, query)
    }

    fn insert<T>(&self, record: T::Insert) -> DbmsResult<()>
    where
        T: TableSchema,
        T::Insert: InsertRecord<Schema = T>,
    {
        let record_values = record.clone().into_values();
        let sanitized_values = self.sanitize_values::<T>(record_values)?;
        self.schema
            .validate_insert(self, T::table_name(), &sanitized_values)?;
        if self.transaction.is_some() {
            self.with_transaction_mut(|tx| tx.insert::<T>(sanitized_values))?;
        } else {
            self.atomic(|db| {
                let mut table_registry = db.load_table_registry::<T>()?;
                let record = T::Insert::from_values(&sanitized_values)?;
                let mut mm = db.ctx.mm.borrow_mut();
                let mut journal_ref = db.ctx.journal.borrow_mut();
                let journal = journal_ref.as_mut().expect("journal must be active inside atomic");
                let mut writer = JournaledWriter::new(&mut *mm, journal);
                table_registry
                    .insert(record.into_record(), &mut writer)
                    .map_err(DbmsError::from)?;
                Ok(())
            })?;
        }

        Ok(())
    }

    fn update<T>(&self, patch: T::Update) -> DbmsResult<u64>
    where
        T: TableSchema,
        T::Update: UpdateRecord<Schema = T>,
    {
        let filter = patch.where_clause().clone();
        if self.transaction.is_some() {
            let pks = self.existing_primary_keys_for_filter::<T>(filter.clone())?;
            let count = pks.len() as u64;
            self.with_transaction_mut(|tx| tx.update::<T>(patch, filter, pks))?;

            return Ok(count);
        }

        let patch = patch.update_values();

        let pk_in_patch = patch.iter().find_map(|(col_def, value)| {
            if col_def.primary_key {
                Some((col_def, value))
            } else {
                None
            }
        });

        self.atomic(|db| {
            let mut count = 0;

            let mut table_registry = db.load_table_registry::<T>()?;
            let records = db.collect_matching_records::<T>(&table_registry, &filter)?;

            for (record, record_values) in records {
                let current_pk_value = record_values
                    .iter()
                    .find(|(col_def, _)| col_def.primary_key)
                    .expect("primary key not found")
                    .1
                    .clone();

                let previous_record = values_to_schema_entity::<T>(record_values.clone())?;
                let mut record_values = record_values;

                for (patch_col_def, patch_value) in &patch {
                    if let Some((_, record_value)) = record_values
                        .iter_mut()
                        .find(|(record_col_def, _)| record_col_def.name == patch_col_def.name)
                    {
                        *record_value = patch_value.clone();
                    }
                }
                let record_values = db.sanitize_values::<T>(record_values)?;
                db.schema.validate_update(
                    db,
                    T::table_name(),
                    &record_values,
                    current_pk_value.clone(),
                )?;
                let updated_record = values_to_schema_entity::<T>(record_values)?;
                {
                    let mut mm = db.ctx.mm.borrow_mut();
                    let mut journal_ref = db.ctx.journal.borrow_mut();
                    let journal =
                        journal_ref.as_mut().expect("journal must be active inside atomic");
                    let mut writer = JournaledWriter::new(&mut *mm, journal);
                    table_registry
                        .update(
                            updated_record,
                            previous_record,
                            record.page,
                            record.offset,
                            &mut writer,
                        )
                        .map_err(DbmsError::from)?;
                }
                count += 1;

                if let Some((pk_column, new_pk_value)) = pk_in_patch {
                    count += db.update_pk_referencing_updated_table::<T>(
                        current_pk_value,
                        new_pk_value.clone(),
                        pk_column.data_type,
                        pk_column.name,
                    )?;
                }
            }

            Ok(count)
        })
    }

    fn delete<T>(&self, behaviour: DeleteBehavior, filter: Option<Filter>) -> DbmsResult<u64>
    where
        T: TableSchema,
    {
        if self.transaction.is_some() {
            let pks = self.existing_primary_keys_for_filter::<T>(filter.clone())?;
            let count = pks.len() as u64;

            self.with_transaction_mut(|tx| tx.delete::<T>(behaviour, filter, pks))?;

            return Ok(count);
        }

        self.atomic(|db| {
            let mut table_registry = db.load_table_registry::<T>()?;
            let records = db.collect_matching_records::<T>(&table_registry, &filter)?;
            let mut count = records.len() as u64;
            for (record, record_values) in records {
                match behaviour {
                    DeleteBehavior::Cascade => {
                        count += db.delete_foreign_keys_cascade::<T>(&record_values)?;
                    }
                    DeleteBehavior::Restrict => {
                        if db.has_foreign_key_references::<T>(&record_values)? {
                            return Err(DbmsError::Query(
                                QueryError::ForeignKeyConstraintViolation {
                                    referencing_table: T::table_name().to_string(),
                                    field: T::primary_key().to_string(),
                                },
                            ));
                        }
                    }
                }
                let mut mm = db.ctx.mm.borrow_mut();
                let mut journal_ref = db.ctx.journal.borrow_mut();
                let journal =
                    journal_ref.as_mut().expect("journal must be active inside atomic");
                let mut writer = JournaledWriter::new(&mut *mm, journal);
                table_registry
                    .delete(record.record, record.page, record.offset, &mut writer)
                    .map_err(DbmsError::from)?;
            }

            Ok(count)
        })
    }

    fn commit(&mut self) -> DbmsResult<()> {
        let Some(txid) = self.transaction.take() else {
            return Err(DbmsError::Transaction(
                TransactionError::NoActiveTransaction,
            ));
        };
        let transaction = {
            let mut ts = self.ctx.transaction_session.borrow_mut();
            ts.take_transaction(&txid)?
        };

        *self.ctx.journal.borrow_mut() = Some(Journal::new());

        for op in transaction.operations {
            let result = match op {
                TransactionOp::Insert { table, values } => self
                    .schema
                    .validate_insert(self, table, &values)
                    .and_then(|()| self.schema.insert(self, table, &values)),
                TransactionOp::Delete {
                    table,
                    behaviour,
                    filter,
                } => self
                    .schema
                    .delete(self, table, behaviour, filter)
                    .map(|_| ()),
                TransactionOp::Update {
                    table,
                    patch,
                    filter,
                } => self.schema.update(self, table, &patch, filter).map(|_| ()),
            };

            if let Err(err) = result {
                if let Some(journal) = self.ctx.journal.borrow_mut().take() {
                    journal
                        .rollback(&mut self.ctx.mm.borrow_mut())
                        .expect("critical: failed to rollback journal");
                }
                return Err(err);
            }
        }

        if let Some(journal) = self.ctx.journal.borrow_mut().take() {
            journal.commit();
        }
        Ok(())
    }

    fn rollback(&mut self) -> DbmsResult<()> {
        let Some(txid) = self.transaction.take() else {
            return Err(DbmsError::Transaction(
                TransactionError::NoActiveTransaction,
            ));
        };

        let mut ts = self.ctx.transaction_session.borrow_mut();
        ts.close_transaction(&txid);
        Ok(())
    }
}

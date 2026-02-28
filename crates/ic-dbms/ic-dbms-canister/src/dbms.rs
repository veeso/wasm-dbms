//! This module exposes all the types related to the DBMS engine.

pub mod integrity;
pub mod join;
pub mod referenced_tables;
pub mod schema;
#[cfg(test)]
mod tests;
pub mod transaction;

use std::cmp::Ordering;
use std::collections::HashSet;

use ic_dbms_api::prelude::{
    ColumnDef, DataTypeKind, Database, DeleteBehavior, Filter, ForeignFetcher, ForeignKeyDef,
    IcDbmsError, IcDbmsResult, InsertRecord, OrderDirection, Query, QueryError, TableColumns,
    TableError, TableRecord, TableSchema, TransactionError, TransactionId, UpdateRecord, Value,
    ValuesSource,
};

use crate::dbms::transaction::{DatabaseOverlay, Transaction, TransactionOp};
use crate::memory::{MEMORY_MANAGER, NextRecord, SCHEMA_REGISTRY, TableRegistry};
use crate::prelude::{DatabaseSchema, TRANSACTION_SESSION};
use crate::utils::trap;

/// Default capacity for SELECT queries.
const DEFAULT_SELECT_CAPACITY: usize = 128;

/// The main DBMS struct.
///
/// This struct serves as the entry point for interacting with the DBMS engine.
///
/// It provides methods for executing queries.
///
/// - [`Database::select`] - Execute a SELECT query.
/// - [`Database::insert`] - Execute an INSERT query.
/// - [`Database::update`] - Execute an UPDATE query.
/// - [`Database::delete`] - Execute a DELETE query.
/// - [`Database::commit`] - Commit the current transaction.
/// - [`Database::rollback`] - Rollback the current transaction.
///
/// The `transaction` field indicates whether the instance is operating within a transaction context.
/// The [`Database`] can be instantiated for one-shot, with [`Database::oneshot`] operations (no transaction),
/// or within a transaction context with [`Database::from_transaction`].
///
/// If a transaction is active, all operations will be part of that transaction until it is committed or rolled back.
pub struct IcDbmsDatabase {
    /// Database schema to perform generic operations, without knowing the concrete table schema at compile time.
    schema: Box<dyn DatabaseSchema>,
    /// Id of the loaded transaction, if any.
    transaction: Option<TransactionId>,
}

impl IcDbmsDatabase {
    /// Load an instance of the [`Database`] for one-shot operations (no transaction).
    pub fn oneshot(schema: impl DatabaseSchema + 'static) -> Self {
        Self {
            schema: Box::new(schema),
            transaction: None,
        }
    }

    /// Load an instance of the [`Database`] within a transaction context.
    pub fn from_transaction(
        schema: impl DatabaseSchema + 'static,
        transaction_id: TransactionId,
    ) -> Self {
        Self {
            schema: Box::new(schema),
            transaction: Some(transaction_id),
        }
    }

    /// Executes a closure with a mutable reference to the current [`Transaction`].
    fn with_transaction_mut<F, R>(&self, f: F) -> IcDbmsResult<R>
    where
        F: FnOnce(&mut Transaction) -> IcDbmsResult<R>,
    {
        let txid = self.transaction.as_ref().ok_or(IcDbmsError::Transaction(
            TransactionError::NoActiveTransaction,
        ))?;

        TRANSACTION_SESSION.with_borrow_mut(|ts| {
            let tx = ts.get_transaction_mut(txid)?;
            f(tx)
        })
    }

    /// Executes a closure with a reference to the current [`Transaction`].
    fn with_transaction<F, R>(&self, f: F) -> IcDbmsResult<R>
    where
        F: FnOnce(&Transaction) -> IcDbmsResult<R>,
    {
        let txid = self.transaction.as_ref().ok_or(IcDbmsError::Transaction(
            TransactionError::NoActiveTransaction,
        ))?;

        TRANSACTION_SESSION.with_borrow(|ts| {
            let tx = ts.get_transaction(txid)?;
            f(tx)
        })
    }

    /// Executes a closure atomically within the database context.
    ///
    /// If the closure returns an error, the changes are rolled back by trapping the canister.
    fn atomic<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&IcDbmsDatabase) -> IcDbmsResult<R>,
    {
        match f(self) {
            Ok(res) => res,
            Err(err) => trap(err.to_string()),
        }
    }

    /// Deletes foreign key related records recursively if the delete behavior is [`DeleteBehavior::Cascade`].
    fn delete_foreign_keys_cascade<T>(
        &self,
        record_values: &[(ColumnDef, Value)],
    ) -> IcDbmsResult<u64>
    where
        T: TableSchema,
    {
        let pk = record_values
            .iter()
            .find(|(col_def, _)| col_def.primary_key)
            .ok_or(IcDbmsError::Query(QueryError::UnknownColumn(
                T::primary_key().to_string(),
            )))?
            .1
            .clone();

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

    /// Retrieves the current [`DatabaseOverlay`].
    fn overlay(&self) -> IcDbmsResult<DatabaseOverlay> {
        self.with_transaction(|tx| Ok(tx.overlay().clone()))
    }

    /// Returns whether the read given record matches the provided filter.
    fn record_matches_filter(
        &self,
        record_values: &[(ColumnDef, Value)],
        filter: &Filter,
    ) -> IcDbmsResult<bool> {
        filter.matches(record_values).map_err(IcDbmsError::from)
    }

    /// Filter record columns down to only the selected fields from the query.
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

    /// Batch-fetches all eager relations for the collected result set.
    ///
    /// For each eager relation, collects all distinct FK values across all records,
    /// performs a single batch query, and attaches the foreign data to each record.
    /// This resolves the N+1 query problem.
    fn batch_load_eager_relations<T>(
        &self,
        results: &mut [TableColumns],
        query: &Query,
    ) -> IcDbmsResult<()>
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
    ///
    /// Returns a list of `(local_column, distinct_pk_values)` pairs.
    fn collect_fk_values<T>(
        results: &[TableColumns],
        relation: &str,
    ) -> IcDbmsResult<Vec<(&'static str, Vec<Value>)>>
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
                return Err(IcDbmsError::Query(QueryError::InvalidQuery(format!(
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

    /// Verifies that every FK value was found in the batch result.
    fn verify_fk_batch(
        batch_map: &std::collections::HashMap<Value, Vec<(ColumnDef, Value)>>,
        pk_values: &[Value],
        relation: &str,
    ) -> IcDbmsResult<()> {
        if let Some(missing) = pk_values.iter().find(|v| !batch_map.contains_key(v)) {
            return Err(IcDbmsError::Query(QueryError::BrokenForeignKeyReference {
                table: relation.to_string(),
                key: missing.clone(),
            }));
        }
        Ok(())
    }

    /// Attaches batch-fetched foreign data to each record that references the given relation.
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

    /// Retrieves existing primary keys for records matching the given filter.
    ///
    /// Only the primary key column is selected to avoid loading unnecessary data.
    fn existing_primary_keys_for_filter<T>(
        &self,
        filter: Option<Filter>,
    ) -> IcDbmsResult<Vec<Value>>
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

    /// Load the table registry for the given table schema.
    fn load_table_registry<T>(&self) -> IcDbmsResult<TableRegistry>
    where
        T: TableSchema,
    {
        // get pages of the table registry from schema registry
        let registry_pages = SCHEMA_REGISTRY
            .with_borrow(|schema| schema.table_registry_page::<T>())
            .ok_or(IcDbmsError::Table(TableError::TableNotFound))?;

        MEMORY_MANAGER
            .with_borrow(|mm| TableRegistry::load(registry_pages, mm))
            .map_err(IcDbmsError::from)
    }

    /// Sorts the query results based on the specified column and order direction.
    ///
    /// We only sort values which have [`ValuesSource::This`].
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

            Self::sort_values_with_direction(a_value, b_value, direction)
        });
    }

    /// Provide the [`Ordering`] to apply to two [`Value`]s by the given [`OrderDirection`].
    fn sort_values_with_direction(
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

    /// Core select logic shared between typed `select::<T>` and generic `select_raw`.
    ///
    /// Returns the intermediate [`TableColumns`] representation before
    /// converting to `T::Record`. Each entry is a row with columns
    /// grouped by source.
    ///
    /// This method is public for use by generated [`DatabaseSchema`] implementations.
    #[doc(hidden)]
    pub fn select_columns<T>(&self, query: Query) -> IcDbmsResult<Vec<TableColumns>>
    where
        T: TableSchema,
    {
        // load table registry
        let table_registry = self.load_table_registry::<T>()?;
        // get database overlay
        let mut table_overlay = if self.transaction.is_some() {
            self.overlay()?
        } else {
            DatabaseOverlay::default()
        };

        // prepare results vector
        let mut results = Vec::with_capacity(query.limit.unwrap_or(DEFAULT_SELECT_CAPACITY));
        // iter and select
        let mut count = 0;

        // TableReader borrows MemoryManager, so the read loop must happen within with_borrow
        MEMORY_MANAGER.with_borrow(|mm| {
            let table_reader = table_registry.read::<T, _>(mm);
            let mut table_reader = table_overlay.reader(table_reader);

            while let Some(values) = table_reader.try_next()? {
                // check whether it matches the filter
                if let Some(filter) = &query.filter {
                    if !self.record_matches_filter(&values, filter)? {
                        continue;
                    }
                }
                // filter matched, check limit and offset
                count += 1;
                // check whether is before offset
                if query.offset.is_some_and(|offset| count <= offset) {
                    continue;
                }
                // wrap raw column values as This source (all columns preserved for FK lookup)
                results.push(vec![(ValuesSource::This, values)]);
                // check whether reached limit
                if query.limit.is_some_and(|limit| results.len() >= limit) {
                    break;
                }
            }

            Ok::<(), IcDbmsError>(())
        })?;

        // batch-load eager relations for all collected records
        self.batch_load_eager_relations::<T>(&mut results, &query)?;

        // apply column selection after eager loading (FK columns must be available during batch load)
        self.apply_column_selection::<T>(&mut results, &query);

        // Sort results if needed, applying in reverse order so the primary sort key
        // (first in the list) is applied last. Since `sort_by` is a stable sort,
        // this produces correct multi-column ordering.
        for (column, direction) in query.order_by.into_iter().rev() {
            self.sort_query_results(&mut results, &column, direction);
        }

        Ok(results)
    }

    /// Executes a join query, returning results with [`CandidColumnDef`] (which includes table names).
    ///
    /// This method is public for use by the generated canister API.
    #[doc(hidden)]
    pub fn select_join(
        &self,
        table: &str,
        query: Query,
    ) -> IcDbmsResult<Vec<Vec<(ic_dbms_api::prelude::CandidColumnDef, Value)>>> {
        self.schema.select_join(self, table, query)
    }

    /// Update the primary key value in the tables referencing the updated table.
    ///
    /// # Arguments
    ///
    /// - `old_pk` - The old primary key value.
    /// - `new_pk` - The new primary key value.
    /// - `data_type` - The data type of the primary key.
    /// - `pk_name` - The name of the primary key column.
    fn update_pk_referencing_updated_table<T>(
        &self,
        old_pk: Value,
        new_pk: Value,
        data_type: DataTypeKind,
        pk_name: &'static str,
    ) -> IcDbmsResult<u64>
    where
        T: TableSchema,
    {
        let mut count = 0;
        // get referencing tables for this table
        // iterate over referencing tables and columns
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
            // make an update patch
            let filter = Filter::eq(ref_col, old_pk.clone());

            count += self
                .schema
                .update(self, ref_table, &[ref_patch_value], Some(filter))?;
        }

        Ok(count)
    }

    /// Given a Vector of [`ColumnDef`] and [`Value`] pairs, sanitize the values using the
    /// sanitizers defined in the table schema.
    fn sanitize_values<T>(
        &self,
        values: Vec<(ColumnDef, Value)>,
    ) -> IcDbmsResult<Vec<(ColumnDef, Value)>>
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

    /// Collects all records matching the given filter from the table registry.
    ///
    /// Returns each matching record along with its page, offset, and column-value pairs.
    #[allow(clippy::type_complexity)]
    fn collect_matching_records<T>(
        &self,
        table_registry: &TableRegistry,
        filter: &Option<Filter>,
    ) -> IcDbmsResult<Vec<(NextRecord<T>, Vec<(ColumnDef, Value)>)>>
    where
        T: TableSchema,
    {
        MEMORY_MANAGER.with_borrow(|mm| {
            let mut table_reader = table_registry.read::<T, _>(mm);
            let mut records = vec![];
            while let Some(values) = table_reader.try_next()? {
                let record_values = values.record.clone().to_values();
                if let Some(filter) = filter {
                    if !self.record_matches_filter(&record_values, filter)? {
                        continue;
                    }
                }
                records.push((values, record_values));
            }
            Ok(records)
        })
    }
}

/// Converts column-value pairs to a schema entity.
fn values_to_schema_entity<T>(values: Vec<(ColumnDef, Value)>) -> IcDbmsResult<T>
where
    T: TableSchema,
{
    let record = T::Insert::from_values(&values)?.into_record();
    Ok(record)
}

impl Database for IcDbmsDatabase {
    fn select<T>(&self, query: Query) -> IcDbmsResult<Vec<T::Record>>
    where
        T: TableSchema,
    {
        // do not allow joins in typed select
        if !query.joins.is_empty() {
            return Err(IcDbmsError::Query(QueryError::JoinInsideTypedSelect));
        }
        let results = self.select_columns::<T>(query)?;
        Ok(results.into_iter().map(T::Record::from_values).collect())
    }

    fn select_raw(&self, table: &str, query: Query) -> IcDbmsResult<Vec<Vec<(ColumnDef, Value)>>> {
        self.schema.select(self, table, query)
    }

    /// Executes an INSERT query.
    ///
    /// # Arguments
    ///
    /// - `record` - The INSERT record to be executed.
    fn insert<T>(&self, record: T::Insert) -> IcDbmsResult<()>
    where
        T: TableSchema,
        T::Insert: InsertRecord<Schema = T>,
    {
        // check whether the insert is valid
        let record_values = record.clone().into_values();
        let sanitized_values = self.sanitize_values::<T>(record_values)?;
        // validate insert
        self.schema
            .validate_insert(self, T::table_name(), &sanitized_values)?;
        if self.transaction.is_some() {
            // insert a new `insert` into the transaction
            self.with_transaction_mut(|tx| tx.insert::<T>(sanitized_values))?;
        } else {
            // insert directly into the database; wrap in atomic for consistency
            // with update/delete paths
            self.atomic(|db| {
                let mut table_registry = db.load_table_registry::<T>()?;
                let record = T::Insert::from_values(&sanitized_values)?;
                MEMORY_MANAGER
                    .with_borrow_mut(|mm| table_registry.insert(record.into_record(), mm))
                    .map_err(IcDbmsError::from)?;
                Ok(())
            });
        }

        Ok(())
    }

    /// Executes an UPDATE query.
    ///
    /// # Arguments
    ///
    /// - `patch` - The UPDATE patch to be applied.
    /// - `filter` - An optional [`Filter`] to specify which records to update.
    ///
    /// # Returns
    ///
    /// The number of rows updated.
    fn update<T>(&self, patch: T::Update) -> IcDbmsResult<u64>
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

        // get whether PK is in the patch. If so, store the value to update referencing tables.
        let pk_in_patch = patch.iter().find_map(|(col_def, value)| {
            if col_def.primary_key {
                Some((col_def, value))
            } else {
                None
            }
        });

        let res = self.atomic(|db| {
            let mut count = 0;

            let mut table_registry = db.load_table_registry::<T>()?;
            let records = db.collect_matching_records::<T>(&table_registry, &filter)?;

            for (record, record_values) in records {
                let current_pk_value = record_values
                    .iter()
                    .find(|(col_def, _)| col_def.primary_key)
                    .expect("primary key not found") // this can't fail.
                    .1
                    .clone();

                let previous_record = values_to_schema_entity::<T>(record_values.clone())?;
                let mut record_values = record_values;

                // apply patch to record values
                for (patch_col_def, patch_value) in &patch {
                    if let Some((_, record_value)) = record_values
                        .iter_mut()
                        .find(|(record_col_def, _)| record_col_def.name == patch_col_def.name)
                    {
                        *record_value = patch_value.clone();
                    }
                }
                // sanitize updated values
                let record_values = db.sanitize_values::<T>(record_values)?;
                // validate updated values
                db.schema.validate_update(
                    db,
                    T::table_name(),
                    &record_values,
                    current_pk_value.clone(),
                )?;
                // build T from values
                let updated_record = values_to_schema_entity::<T>(record_values)?;
                // perform the update in the table registry
                MEMORY_MANAGER
                    .with_borrow_mut(|mm| {
                        table_registry.update(
                            updated_record,
                            previous_record,
                            record.page,
                            record.offset,
                            mm,
                        )
                    })
                    .map_err(IcDbmsError::from)?;
                count += 1;

                // update records in tables referencing this table if PK is updated
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
        });

        Ok(res)
    }

    /// Executes a DELETE query.
    ///
    /// # Arguments
    ///
    /// - `behaviour` - The [`DeleteBehavior`] to apply for foreign key constraints.
    /// - `filter` - An optional [`Filter`] to specify which records to delete.
    ///
    /// # Returns
    ///
    /// The number of rows deleted.
    fn delete<T>(&self, behaviour: DeleteBehavior, filter: Option<Filter>) -> IcDbmsResult<u64>
    where
        T: TableSchema,
    {
        if self.transaction.is_some() {
            let pks = self.existing_primary_keys_for_filter::<T>(filter.clone())?;
            let count = pks.len() as u64;

            self.with_transaction_mut(|tx| tx.delete::<T>(behaviour, filter, pks))?;

            return Ok(count);
        }

        // delete must be atomic
        let res = self.atomic(|db| {
            let mut table_registry = db.load_table_registry::<T>()?;
            let records = db.collect_matching_records::<T>(&table_registry, &filter)?;
            let mut count = records.len() as u64;
            for (record, record_values) in records {
                // match delete behaviour
                match behaviour {
                    DeleteBehavior::Cascade => {
                        // delete recursively foreign keys if cascade
                        count += self.delete_foreign_keys_cascade::<T>(&record_values)?;
                    }
                    DeleteBehavior::Restrict => {
                        if self.delete_foreign_keys_cascade::<T>(&record_values)? > 0 {
                            // it's okay; we panic here because we are in an atomic closure
                            return Err(IcDbmsError::Query(
                                QueryError::ForeignKeyConstraintViolation {
                                    referencing_table: T::table_name().to_string(),
                                    field: T::primary_key().to_string(),
                                },
                            ));
                        }
                    }
                }
                // eventually delete the record
                MEMORY_MANAGER
                    .with_borrow_mut(|mm| {
                        table_registry.delete(record.record, record.page, record.offset, mm)
                    })
                    .map_err(IcDbmsError::from)?;
            }

            Ok(count)
        });

        Ok(res)
    }

    /// Commits the current transaction.
    ///
    /// The transaction is consumed.
    ///
    /// Any error during commit will trap the canister to ensure consistency.
    fn commit(&mut self) -> IcDbmsResult<()> {
        // take transaction out of self and get the transaction out of the storage
        // this also invalidates the overlay, so we won't have conflicts during validation
        let Some(txid) = self.transaction.take() else {
            return Err(IcDbmsError::Transaction(
                TransactionError::NoActiveTransaction,
            ));
        };
        let transaction = TRANSACTION_SESSION.with_borrow_mut(|ts| ts.take_transaction(&txid))?;

        // iterate over operations and apply them;
        // for each operation, first validate, then apply
        // using `self.atomic` when applying to ensure consistency
        for op in transaction.operations {
            match op {
                TransactionOp::Insert { table, values } => {
                    // validate
                    self.schema.validate_insert(self, table, &values)?;
                    // insert
                    self.atomic(|db| db.schema.insert(db, table, &values));
                }
                TransactionOp::Delete {
                    table,
                    behaviour,
                    filter,
                } => {
                    self.atomic(|db| db.schema.delete(db, table, behaviour, filter));
                }
                TransactionOp::Update {
                    table,
                    patch,
                    filter,
                } => {
                    self.atomic(|db| db.schema.update(db, table, &patch, filter));
                }
            }
        }

        Ok(())
    }

    /// Rolls back the current transaction.
    ///
    /// The transaction is consumed.
    fn rollback(&mut self) -> IcDbmsResult<()> {
        let Some(txid) = self.transaction.take() else {
            return Err(IcDbmsError::Transaction(
                TransactionError::NoActiveTransaction,
            ));
        };

        TRANSACTION_SESSION.with_borrow_mut(|ts| ts.close_transaction(&txid));
        Ok(())
    }
}

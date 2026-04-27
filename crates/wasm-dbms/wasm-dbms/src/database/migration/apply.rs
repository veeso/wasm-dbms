//! Migration apply pipeline.
//!
//! Executes a planned, ordered `Vec<MigrationOp>` against the DBMS context.
//! All mutations run inside the existing journaled atomic block on
//! [`WasmDbmsDatabase::atomic`], so any failure rolls back every page touched
//! during the call. On success, each touched table's snapshot ledger is
//! refreshed and the cached drift flag is cleared so subsequent CRUD passes.
//!
//! # Scope
//!
//! Structural ops are implemented directly:
//!
//! - `CreateTable` — registers a new table from its compiled snapshot.
//! - `DropTable` — removes the entry from the schema registry and returns
//!   every page owned by the dropped table to the unclaimed-pages ledger
//!   for reuse by future `claim_page` calls.
//! - `AlterColumn` — snapshot-only edit, plus tightening validation through
//!   the schema dispatch.
//! - `AddIndex` / `DropIndex` — snapshot edit plus index-ledger refresh from
//!   live rows.
//!
//! Column-mutating ops (`AddColumn`, `DropColumn`, `RenameColumn`,
//! `WidenColumn`, `TransformColumn`) use the snapshot-driven record codec
//! (see [`super::codec`]) and the [`rewrite_table`] helper to walk every
//! record under the stored snapshot and re-encode it under the target
//! snapshot. Indexes are rebuilt from scratch after the rewrite.

use wasm_dbms_api::prelude::{
    ColumnChanges, ColumnSnapshot, DataTypeSnapshot, DbmsError, DbmsResult, Filter,
    ForeignKeySnapshot, MSize, MigrationError, MigrationOp, Query, TableSchemaSnapshot, Value,
};
use wasm_dbms_memory::TableRegistry;
use wasm_dbms_memory::prelude::{AccessControl, IndexLedger, MemoryProvider};

use crate::database::WasmDbmsDatabase;
use crate::database::migration::codec::{decode_record_by_snapshot, encode_record_by_snapshot};
use crate::database::migration::widen::widen_value;
use crate::transaction::journal::JournaledWriter;

/// Applies `ops` to the database under the existing journal.
///
/// Caller is responsible for sorting `ops` via
/// [`super::plan::order_ops`] and validating them via
/// [`super::plan::validate`] before calling this function. The apply layer
/// only re-checks invariants that depend on runtime state (e.g. tightening
/// validation against existing rows).
///
/// # Errors
///
/// Any [`MigrationError`] surfaced by the per-op handlers, propagated as
/// [`DbmsError::Migration`]. The journaled atomic block rolls back on the
/// first error — partial migrations are impossible.
pub(crate) fn apply<M, A>(db: &WasmDbmsDatabase<'_, M, A>, ops: Vec<MigrationOp>) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    db.ctx.set_migrating(true);
    let result = db.atomic(|db| {
        let mut touched_snapshots: Vec<TableSchemaSnapshot> = Vec::new();

        for op in ops {
            apply_op(db, op, &mut touched_snapshots)?;
        }

        commit_snapshots(db, &touched_snapshots)?;
        Ok(())
    });
    db.ctx.set_migrating(false);
    {
        let mut mm = db.ctx.mm.borrow_mut();
        let refreshed = wasm_dbms_memory::SchemaRegistry::load(&mut *mm)?;
        *db.ctx.schema_registry.borrow_mut() = refreshed;
    }
    result?;

    db.ctx.clear_drift();
    Ok(())
}

fn apply_op<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    op: MigrationOp,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    match op {
        MigrationOp::CreateTable { name: _, schema } => {
            create_table(db, &schema)?;
            // The freshly created snapshot is already persisted by
            // `register_table_from_snapshot`, but we still record it under
            // `touched` so the post-apply commit pass observes the full set
            // of changed tables.
            touched.push(schema);
            Ok(())
        }
        MigrationOp::DropTable { name } => drop_table(db, &name),
        MigrationOp::AlterColumn {
            table,
            column,
            changes,
        } => alter_column(db, &table, &column, &changes, touched),
        MigrationOp::AddIndex { table, index } => add_index(db, &table, &index, touched),
        MigrationOp::DropIndex { table, index } => drop_index(db, &table, &index, touched),
        MigrationOp::AddColumn { table, column } => add_column(db, &table, &column, touched),
        MigrationOp::DropColumn { table, column } => drop_column(db, &table, &column, touched),
        MigrationOp::RenameColumn { table, old, new } => {
            rename_column(db, &table, &old, &new, touched)
        }
        MigrationOp::WidenColumn {
            table,
            column,
            old_type,
            new_type,
        } => widen_column(db, &table, &column, &old_type, &new_type, touched),
        MigrationOp::TransformColumn {
            table,
            column,
            old_type,
            new_type,
        } => transform_column(db, &table, &column, &old_type, &new_type, touched),
    }
}

fn create_table<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    schema: &TableSchemaSnapshot,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut sr = db.ctx.schema_registry.borrow_mut();
    let mut mm = db.ctx.mm.borrow_mut();
    let mut journal_ref = db.ctx.journal.borrow_mut();
    let journal = journal_ref
        .as_mut()
        .expect("journal must be active inside atomic");
    let mut writer = JournaledWriter::new(&mut *mm, journal);
    sr.register_table_from_snapshot(schema, &mut writer)?;
    Ok(())
}

fn drop_table<M, A>(db: &WasmDbmsDatabase<'_, M, A>, name: &str) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut sr = db.ctx.schema_registry.borrow_mut();
    let mut mm = db.ctx.mm.borrow_mut();
    let mut journal_ref = db.ctx.journal.borrow_mut();
    let journal = journal_ref
        .as_mut()
        .expect("journal must be active inside atomic");
    let mut writer = JournaledWriter::new(&mut *mm, journal);
    sr.unregister_table(name, &mut writer)?;
    Ok(())
}

fn alter_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    changes: &ColumnChanges,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut snapshot = load_snapshot_for_mutation(db, table, touched)?;
    let target = snapshot
        .columns
        .iter_mut()
        .find(|c| c.name == column)
        .ok_or_else(|| {
            DbmsError::Migration(MigrationError::ConstraintViolation {
                table: table.to_string(),
                column: column.to_string(),
                reason: "column not present in stored snapshot".to_string(),
            })
        })?;

    if let Some(value) = changes.nullable {
        if !value {
            validate_no_nulls(db, table, column)?;
        }
        target.nullable = value;
    }
    if let Some(value) = changes.unique {
        if value {
            validate_unique(db, table, column)?;
        }
        target.unique = value;
    }
    if let Some(value) = changes.auto_increment {
        target.auto_increment = value;
    }
    if let Some(value) = changes.primary_key {
        target.primary_key = value;
    }
    if let Some(fk) = &changes.foreign_key {
        if let Some(new_fk) = fk {
            validate_foreign_key(db, table, column, new_fk)?;
        }
        target.foreign_key = fk.clone();
    }

    persist_pending_snapshot(touched, snapshot);
    Ok(())
}

/// Validate that every non-null value in `(table, column)` is present in
/// `(fk.table, fk.column)`. Runs before the snapshot edit so a violation
/// rolls back the entire apply pass.
fn validate_foreign_key<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    fk: &ForeignKeySnapshot,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let rows = db.schema.select(db, table, Query::builder().build())?;
    for row in rows {
        let Some((_, value)) = row.iter().find(|(c, _)| c.name == column) else {
            continue;
        };
        if matches!(value, Value::Null) {
            continue;
        }
        validate_foreign_key_value(db, table, column, fk, value)?;
    }
    Ok(())
}

fn validate_foreign_key_value<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    fk: &ForeignKeySnapshot,
    value: &Value,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let target_rows = db.schema.select(
        db,
        &fk.table,
        Query::builder()
            .filter(Some(Filter::eq(&fk.column, value.clone())))
            .build(),
    )?;
    if target_rows.is_empty() {
        return Err(DbmsError::Migration(MigrationError::ForeignKeyViolation {
            table: table.to_string(),
            column: column.to_string(),
            target_table: fk.table.clone(),
            value: format!("{value:?}"),
        }));
    }
    Ok(())
}

fn add_index<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    index: &wasm_dbms_api::prelude::IndexSnapshot,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut snapshot = load_snapshot_for_mutation(db, table, touched)?;
    if !snapshot.indexes.iter().any(|i| i == index) {
        snapshot.indexes.push(index.clone());
        rebuild_indexes_from_storage(db, table, &snapshot)?;
    }
    persist_pending_snapshot(touched, snapshot);
    Ok(())
}

fn drop_index<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    index: &wasm_dbms_api::prelude::IndexSnapshot,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut snapshot = load_snapshot_for_mutation(db, table, touched)?;
    snapshot.indexes.retain(|i| i != index);
    persist_pending_snapshot(touched, snapshot);
    Ok(())
}

/// Resolve the default value for a freshly added column.
///
/// Resolution order:
/// 1. `Migrate::default_value` hook on the schema dispatch.
/// 2. `#[default]` attribute baked into the column snapshot.
/// 3. `Value::Null` if the column is nullable.
/// 4. Otherwise, [`MigrationError::DefaultMissing`].
fn resolve_default<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &ColumnSnapshot,
) -> DbmsResult<Value>
where
    M: MemoryProvider,
    A: AccessControl,
{
    if let Some(v) = db.schema.migrate_default_dyn(table, &column.name) {
        return Ok(v);
    }
    if let Some(v) = column.default.clone() {
        return Ok(v);
    }
    if column.nullable {
        return Ok(Value::Null);
    }
    Err(DbmsError::Migration(MigrationError::DefaultMissing {
        table: table.to_string(),
        column: column.name.clone(),
    }))
}

/// Transform `column` via [`Migrate::transform_column`] for each row.
///
/// `Ok(Some(v))` — use the new value.
/// `Ok(None)` — error with [`MigrationError::TransformReturnedNone`] (the
/// caller could have used `WidenColumn` if the type pair were widening).
/// `Err(_)` — propagate; the journal session rolls back.
fn transform_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    _old_type: &DataTypeSnapshot,
    new_type: &DataTypeSnapshot,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let old_snapshot = load_snapshot_for_mutation(db, table, touched)?;
    if !old_snapshot.columns.iter().any(|c| c.name == column) {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: column.to_string(),
            reason: "column not present in stored snapshot".to_string(),
        }));
    }
    let mut new_snapshot = old_snapshot.clone();
    if let Some(col) = new_snapshot.columns.iter_mut().find(|c| c.name == column) {
        col.data_type = new_type.clone();
    }
    let table_owned = table.to_string();
    let column_owned = column.to_string();

    rewrite_table(db, table, &old_snapshot, &new_snapshot, |values| {
        values
            .into_iter()
            .map(|(n, v)| {
                if n != column_owned {
                    return Ok((n, v));
                }
                if matches!(v, Value::Null) {
                    return Ok((n, Value::Null));
                }
                let new_value = db
                    .schema
                    .migrate_transform_dyn(&table_owned, &column_owned, v)?;
                match new_value {
                    Some(new_value) => Ok((n, new_value)),
                    None => Err(DbmsError::Migration(
                        MigrationError::TransformReturnedNone {
                            table: table_owned.clone(),
                            column: column_owned.clone(),
                        },
                    )),
                }
            })
            .collect::<DbmsResult<Vec<(String, Value)>>>()
    })?;
    persist_pending_snapshot(touched, new_snapshot);
    Ok(())
}

/// Widen `column` in `table` from `old_type` to `new_type` using the
/// compatible-widening whitelist. Each record's value for the column is
/// replaced with its widened equivalent.
fn widen_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    old_type: &DataTypeSnapshot,
    new_type: &DataTypeSnapshot,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let old_snapshot = load_snapshot_for_mutation(db, table, touched)?;
    if !old_snapshot.columns.iter().any(|c| c.name == column) {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: column.to_string(),
            reason: "column not present in stored snapshot".to_string(),
        }));
    }
    let mut new_snapshot = old_snapshot.clone();
    if let Some(col) = new_snapshot.columns.iter_mut().find(|c| c.name == column) {
        col.data_type = new_type.clone();
    }
    let table_owned = table.to_string();
    let column_owned = column.to_string();
    let old_dt = old_type.clone();
    let new_dt = new_type.clone();
    rewrite_table(db, table, &old_snapshot, &new_snapshot, |values| {
        values
            .into_iter()
            .map(|(n, v)| {
                if n == column_owned {
                    let widened = widen_value(&table_owned, &column_owned, &old_dt, &new_dt, v)?;
                    Ok((n, widened))
                } else {
                    Ok((n, v))
                }
            })
            .collect::<DbmsResult<Vec<(String, Value)>>>()
    })?;
    persist_pending_snapshot(touched, new_snapshot);
    Ok(())
}

/// Rename `old` → `new` in `table`. Each record's column-keyed tuple is
/// rewritten with the new key; data is preserved verbatim.
fn rename_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    old: &str,
    new: &str,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let old_snapshot = load_snapshot_for_mutation(db, table, touched)?;
    if !old_snapshot.columns.iter().any(|c| c.name == old) {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: old.to_string(),
            reason: "column not present in stored snapshot".to_string(),
        }));
    }
    let mut new_snapshot = old_snapshot.clone();
    if let Some(col) = new_snapshot.columns.iter_mut().find(|c| c.name == old) {
        col.name = new.to_string();
    }
    let old_owned = old.to_string();
    let new_owned = new.to_string();
    rewrite_table(db, table, &old_snapshot, &new_snapshot, |values| {
        Ok(values
            .into_iter()
            .map(|(n, v)| {
                if n == old_owned {
                    (new_owned.clone(), v)
                } else {
                    (n, v)
                }
            })
            .collect())
    })?;
    persist_pending_snapshot(touched, new_snapshot);
    Ok(())
}

/// Drop `column` from `table`. Each existing record is rewritten without
/// the column's value; the snapshot mutation is published into `touched`.
fn drop_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let old_snapshot = load_snapshot_for_mutation(db, table, touched)?;
    if !old_snapshot.columns.iter().any(|c| c.name == column) {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: column.to_string(),
            reason: "column not present in stored snapshot".to_string(),
        }));
    }
    let mut new_snapshot = old_snapshot.clone();
    new_snapshot.columns.retain(|c| c.name != column);
    let column_owned = column.to_string();

    rewrite_table(db, table, &old_snapshot, &new_snapshot, |values| {
        Ok(values
            .into_iter()
            .filter(|(n, _)| n != &column_owned)
            .collect())
    })?;

    persist_pending_snapshot(touched, new_snapshot);
    Ok(())
}

/// Append `column` to `table` and back-fill every existing record with its
/// resolved default. Snapshot mutation is published into `touched` so the
/// final commit pass writes the new shape to the snapshot ledger.
fn add_column<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &ColumnSnapshot,
    touched: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let old_snapshot = load_snapshot_for_mutation(db, table, touched)?;
    let mut new_snapshot = old_snapshot.clone();
    new_snapshot.columns.push(column.clone());

    // Resolve default outside the rewrite borrow scope.
    let default = resolve_default(db, table, column)?;
    validate_added_column_constraints(db, table, column, &default)?;
    let column_name = column.name.clone();

    rewrite_table(db, table, &old_snapshot, &new_snapshot, |mut values| {
        values.push((column_name.clone(), default.clone()));
        Ok(values)
    })?;

    persist_pending_snapshot(touched, new_snapshot);
    Ok(())
}

/// Validate that `AddColumn` can backfill existing rows without violating the
/// constraints declared on the new column.
fn validate_added_column_constraints<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &ColumnSnapshot,
    default: &Value,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    if !column.nullable && matches!(default, Value::Null) {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: column.name.clone(),
            reason: "backfill default resolves to NULL for new NOT NULL column".to_string(),
        }));
    }

    let row_count = count_rows(db, table)?;
    if column.unique && row_count > 1 {
        return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: column.name.clone(),
            reason: "backfill default would duplicate values for new UNIQUE column".to_string(),
        }));
    }

    if let Some(fk) = &column.foreign_key
        && !matches!(default, Value::Null)
    {
        validate_foreign_key_value(db, table, &column.name, fk, default)?;
    }

    Ok(())
}

/// Read every live record under `old_snapshot`, run `project` over the
/// decoded column-value list, and re-insert under `new_snapshot`. Index
/// rebuild is deferred to the apply pass's commit step.
///
/// Runs inside the existing journal session opened by `apply`, so any
/// failure rolls back every page touched by the rewrite.
fn rewrite_table<M, A, F>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    old_snapshot: &TableSchemaSnapshot,
    new_snapshot: &TableSchemaSnapshot,
    mut project: F,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
    F: FnMut(Vec<(String, Value)>) -> DbmsResult<Vec<(String, Value)>>,
{
    let pages = table_registry_pages(db, table)?;
    let raw_rows = load_raw_rows(db, table, old_snapshot)?;
    let mut mm = db.ctx.mm.borrow_mut();
    let mut journal_ref = db.ctx.journal.borrow_mut();
    let journal = journal_ref
        .as_mut()
        .expect("journal must be active inside atomic");
    let mut writer = JournaledWriter::new(&mut *mm, journal);
    let mut registry = TableRegistry::load(pages, &mut writer)?;

    let mut new_rows: Vec<(wasm_dbms_memory::RecordAddress, Vec<(String, Value)>)> = Vec::new();
    for row in raw_rows {
        let values = decode_record_by_snapshot(&row.bytes, old_snapshot)?;
        let projected = project(values)?;
        let new_bytes = encode_record_by_snapshot(&projected, new_snapshot)?;
        registry.delete_raw(
            row.address,
            row.bytes.len() as MSize,
            old_snapshot.alignment as u16,
            &mut writer,
        )?;
        let new_address =
            registry.insert_raw(&new_bytes, new_snapshot.alignment as u16, &mut writer)?;
        new_rows.push((new_address, projected));
    }

    rebuild_indexes(&pages, new_snapshot, &new_rows, &mut writer)?;
    Ok(())
}

/// Rebuild indexes for `snapshot` by scanning the current live rows stored for
/// `table`.
fn rebuild_indexes_from_storage<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    snapshot: &TableSchemaSnapshot,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let pages = table_registry_pages(db, table)?;
    let rows = load_live_rows(db, table, snapshot)?;
    let mut mm = db.ctx.mm.borrow_mut();
    let mut journal_ref = db.ctx.journal.borrow_mut();
    let journal = journal_ref
        .as_mut()
        .expect("journal must be active inside atomic");
    let mut writer = JournaledWriter::new(&mut *mm, journal);
    rebuild_indexes(&pages, snapshot, &rows, &mut writer)
}

/// Re-initialise the table's index ledger and re-populate every surviving
/// index from `new_rows`. Called at the end of [`rewrite_table`] after every
/// record has been delete+insert-rewritten under the new snapshot.
fn rebuild_indexes<MA>(
    pages: &wasm_dbms_memory::prelude::TableRegistryPage,
    new_snapshot: &TableSchemaSnapshot,
    new_rows: &[(wasm_dbms_memory::RecordAddress, Vec<(String, Value)>)],
    mm: &mut MA,
) -> DbmsResult<()>
where
    MA: wasm_dbms_memory::MemoryAccess,
{
    let key_specs: Vec<Vec<String>> = new_snapshot
        .indexes
        .iter()
        .map(|idx| idx.columns.clone())
        .collect();
    IndexLedger::init_from_keys(pages.index_registry_page, key_specs.clone(), mm)?;
    let mut ledger = IndexLedger::load(pages.index_registry_page, mm)?;
    for (address, values) in new_rows {
        for index in &new_snapshot.indexes {
            let key: Vec<Value> = index
                .columns
                .iter()
                .map(|col| {
                    values
                        .iter()
                        .find(|(n, _)| n == col)
                        .map(|(_, v)| v.clone())
                        .unwrap_or(Value::Null)
                })
                .collect();
            let columns_refs: Vec<&str> = index.columns.iter().map(String::as_str).collect();
            ledger.insert(&columns_refs, key, *address, mm)?;
        }
    }
    Ok(())
}

/// Loads the stored snapshot for `table`, preferring the in-flight pending
/// version (already mutated earlier in the same apply pass) over the on-disk
/// copy.
fn load_snapshot_for_mutation<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    pending: &mut Vec<TableSchemaSnapshot>,
) -> DbmsResult<TableSchemaSnapshot>
where
    M: MemoryProvider,
    A: AccessControl,
{
    if let Some(idx) = pending.iter().position(|s| s.name == table) {
        return Ok(pending.swap_remove(idx));
    }

    let stored = {
        let sr = db.ctx.schema_registry.borrow();
        let mut mm = db.ctx.mm.borrow_mut();
        sr.stored_snapshots(&mut *mm)?
    };
    stored.into_iter().find(|s| s.name == table).ok_or_else(|| {
        DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: String::new(),
            reason: "table not present in stored schema".to_string(),
        })
    })
}

fn persist_pending_snapshot(pending: &mut Vec<TableSchemaSnapshot>, snapshot: TableSchemaSnapshot) {
    pending.push(snapshot);
}

fn table_registry_pages<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
) -> DbmsResult<wasm_dbms_memory::prelude::TableRegistryPage>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let sr = db.ctx.schema_registry.borrow();
    sr.table_registry_page_by_name(table).ok_or_else(|| {
        DbmsError::Migration(MigrationError::ConstraintViolation {
            table: table.to_string(),
            column: String::new(),
            reason: "table not present in schema registry".to_string(),
        })
    })
}

fn load_raw_rows<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    snapshot: &TableSchemaSnapshot,
) -> DbmsResult<Vec<wasm_dbms_memory::RawRecordBytes>>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let pages = table_registry_pages(db, table)?;
    let mut mm = db.ctx.mm.borrow_mut();
    let registry = TableRegistry::load(pages, &mut *mm)?;
    let mut reader = registry.iter_raw(snapshot.alignment as u16, &mut *mm);
    let mut rows = Vec::new();
    while let Some(row) = reader.try_next()? {
        rows.push(row);
    }
    Ok(rows)
}

type LoadedLiveRow = (wasm_dbms_memory::RecordAddress, Vec<(String, Value)>);

fn load_live_rows<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    snapshot: &TableSchemaSnapshot,
) -> DbmsResult<Vec<LoadedLiveRow>>
where
    M: MemoryProvider,
    A: AccessControl,
{
    load_raw_rows(db, table, snapshot)?
        .into_iter()
        .map(|row| {
            Ok((
                row.address,
                decode_record_by_snapshot(&row.bytes, snapshot)?,
            ))
        })
        .collect()
}

fn count_rows<M, A>(db: &WasmDbmsDatabase<'_, M, A>, table: &str) -> DbmsResult<usize>
where
    M: MemoryProvider,
    A: AccessControl,
{
    Ok(db.schema.select(db, table, Query::builder().build())?.len())
}

fn validate_no_nulls<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let rows = db.schema.select(db, table, Query::builder().build())?;
    for row in rows {
        let value_is_null = row
            .iter()
            .find(|(col_def, _)| col_def.name == column)
            .map(|(_, value)| matches!(value, wasm_dbms_api::prelude::Value::Null))
            .unwrap_or(false);
        if value_is_null {
            return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
                table: table.to_string(),
                column: column.to_string(),
                reason: "existing row contains NULL for new NOT NULL constraint".to_string(),
            }));
        }
    }
    Ok(())
}

fn validate_unique<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    table: &str,
    column: &str,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let rows = db.schema.select(db, table, Query::builder().build())?;
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if let Some((_, value)) = row.iter().find(|(col_def, _)| col_def.name == column)
            && !seen.insert(value.clone())
        {
            return Err(DbmsError::Migration(MigrationError::ConstraintViolation {
                table: table.to_string(),
                column: column.to_string(),
                reason: "existing rows contain duplicate values for new UNIQUE constraint"
                    .to_string(),
            }));
        }
    }
    Ok(())
}

/// Writes every pending snapshot to its `schema_snapshot_page`.
///
/// Runs as the final mutation inside the journaled atomic block, so a failure
/// here rolls back every previous page write performed by the apply pass.
fn commit_snapshots<M, A>(
    db: &WasmDbmsDatabase<'_, M, A>,
    snapshots: &[TableSchemaSnapshot],
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    use wasm_dbms_memory::prelude::TableRegistry;

    let pages_for: Vec<_> = {
        let sr = db.ctx.schema_registry.borrow();
        snapshots
            .iter()
            .filter_map(|snap| {
                sr.table_registry_page_by_name(&snap.name)
                    .map(|p| (snap.clone(), p))
            })
            .collect()
    };

    for (snapshot, pages) in pages_for {
        let mut mm = db.ctx.mm.borrow_mut();
        let mut journal_ref = db.ctx.journal.borrow_mut();
        let journal = journal_ref
            .as_mut()
            .expect("journal must be active inside atomic");
        let mut writer = JournaledWriter::new(&mut *mm, journal);
        let mut registry = TableRegistry::load(pages, &mut writer)?;
        registry.schema_snapshot_ledger_mut().write(
            pages.schema_snapshot_page,
            snapshot,
            &mut writer,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnSnapshot, DataTypeSnapshot, Database, IndexSnapshot, MigrationOp, MigrationPolicy,
        TableSchema, TableSchemaSnapshot, Text, Uint32,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::{HeapMemoryProvider, SchemaRegistry};

    use super::*;
    use crate::context::DbmsContext;
    use crate::database::migration::plan::order_ops;

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "users"]
    pub struct User {
        #[primary_key]
        pub id: Uint32,
        pub name: Text,
    }

    #[derive(DatabaseSchema)]
    #[tables(User = "users")]
    pub struct UserSchema;

    fn fresh_db() -> DbmsContext<HeapMemoryProvider> {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        UserSchema::register_tables(&ctx).unwrap();
        ctx
    }

    fn fresh_snapshot(name: &str) -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: name.to_string(),
            primary_key: "id".to_string(),
            alignment: 8,
            columns: vec![ColumnSnapshot {
                name: "id".to_string(),
                data_type: DataTypeSnapshot::Uint32,
                nullable: false,
                auto_increment: false,
                unique: true,
                primary_key: true,
                foreign_key: None,
                default: None,
            }],
            indexes: vec![],
        }
    }

    fn persisted_snapshots(ctx: &DbmsContext<HeapMemoryProvider>) -> Vec<TableSchemaSnapshot> {
        let mut mm = ctx.mm.borrow_mut();
        let persisted = SchemaRegistry::load(&mut *mm).expect("load schema registry");
        persisted
            .stored_snapshots(&mut *mm)
            .expect("load persisted snapshots")
    }

    #[test]
    fn test_create_table_registers_a_new_table() {
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        apply(
            &db,
            vec![MigrationOp::CreateTable {
                name: "fresh".to_string(),
                schema: fresh_snapshot("fresh"),
            }],
        )
        .unwrap();

        let snapshots = ctx
            .schema_registry
            .borrow()
            .stored_snapshots(&mut *ctx.mm.borrow_mut())
            .unwrap();
        assert!(snapshots.iter().any(|s| s.name == "fresh"));
    }

    #[test]
    fn test_drop_table_removes_entry_from_registry() {
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        apply(
            &db,
            vec![MigrationOp::DropTable {
                name: "users".to_string(),
            }],
        )
        .unwrap();

        let snapshots = ctx
            .schema_registry
            .borrow()
            .stored_snapshots(&mut *ctx.mm.borrow_mut())
            .unwrap();
        assert!(!snapshots.iter().any(|s| s.name == "users"));
    }

    #[test]
    fn test_drop_table_returns_owned_pages_to_unclaimed_ledger() {
        // Insert several rows to fan the page ledger out beyond a single
        // metadata page, then drop the table and verify a fresh
        // CreateTable reuses the released pages instead of bumping the
        // high-water mark.
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        for id in 0..50u32 {
            insert_user(&db, id, &format!("user_{id}"));
        }

        let pages_after_inserts = ctx.mm.borrow().pages_count();

        apply(
            &db,
            vec![MigrationOp::DropTable {
                name: "users".to_string(),
            }],
        )
        .expect("drop");

        // Re-create the table from the same snapshot. Because
        // `claim_page` consults the unclaimed-pages ledger first, the
        // overall high-water mark should not exceed the pre-drop count.
        apply(
            &db,
            vec![MigrationOp::CreateTable {
                name: "users".to_string(),
                schema: User::schema_snapshot(),
            }],
        )
        .expect("recreate");

        let pages_after_recreate = ctx.mm.borrow().pages_count();
        assert!(
            pages_after_recreate <= pages_after_inserts,
            "drop+recreate should not grow memory: {pages_after_inserts} → {pages_after_recreate}"
        );
    }

    #[test]
    fn test_failed_drop_table_rolls_back_registry_and_persisted_pages() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }

        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
        let result = apply(
            &db,
            vec![
                MigrationOp::DropTable {
                    name: "users".to_string(),
                },
                MigrationOp::AlterColumn {
                    table: "users".to_string(),
                    column: "missing".to_string(),
                    changes: ColumnChanges {
                        nullable: Some(true),
                        ..Default::default()
                    },
                },
            ],
        );
        assert!(result.is_err(), "migration should fail after drop");

        assert!(
            ctx.schema_registry
                .borrow()
                .table_registry_page_by_name("users")
                .is_some(),
            "in-memory registry must roll back failed drop"
        );

        let snapshots = persisted_snapshots(&ctx);
        assert!(
            snapshots.iter().any(|snapshot| snapshot.name == "users"),
            "persisted schema registry must retain the dropped table after rollback"
        );

        let rows = db
            .select::<User>(Query::builder().build())
            .expect("select after failed migration");
        assert_eq!(rows.len(), 1, "table rows must survive failed drop");
    }

    #[test]
    fn test_add_index_persists_in_snapshot() {
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        let new_index = IndexSnapshot {
            columns: vec!["name".to_string()],
            unique: false,
        };
        apply(
            &db,
            vec![MigrationOp::AddIndex {
                table: "users".to_string(),
                index: new_index.clone(),
            }],
        )
        .unwrap();

        let snapshots = ctx
            .schema_registry
            .borrow()
            .stored_snapshots(&mut *ctx.mm.borrow_mut())
            .unwrap();
        let users = snapshots.iter().find(|s| s.name == "users").unwrap();
        assert!(users.indexes.iter().any(|i| i == &new_index));
    }

    #[test]
    fn test_add_index_backfills_rows_after_planner_ordered_column_rewrite() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
            insert_user(&db, 2, "bob");
        }

        let mut ops = vec![
            MigrationOp::AddIndex {
                table: "users".to_string(),
                index: IndexSnapshot {
                    columns: vec!["email".to_string()],
                    unique: false,
                },
            },
            MigrationOp::AddColumn {
                table: "users".to_string(),
                column: ColumnSnapshot {
                    name: "email".to_string(),
                    data_type: DataTypeSnapshot::Text,
                    nullable: false,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: None,
                    default: Some(Value::Text(Text("guest@example.com".into()))),
                },
            },
        ];
        order_ops(&mut ops);

        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(&db, ops).unwrap();
        }

        let pages = ctx
            .schema_registry
            .borrow()
            .table_registry_page_by_name("users")
            .unwrap();
        let mut mm = ctx.mm.borrow_mut();
        let registry = TableRegistry::load(pages, &mut *mm).unwrap();
        let hits = registry
            .index_ledger()
            .search(
                &["email"],
                &vec![Value::Text(Text("guest@example.com".into()))],
                &mut *mm,
            )
            .unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn test_alter_column_relax_persists_nullability_change() {
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        apply(
            &db,
            vec![MigrationOp::AlterColumn {
                table: "users".to_string(),
                column: "name".to_string(),
                changes: ColumnChanges {
                    nullable: Some(true),
                    ..Default::default()
                },
            }],
        )
        .unwrap();

        let snapshots = ctx
            .schema_registry
            .borrow()
            .stored_snapshots(&mut *ctx.mm.borrow_mut())
            .unwrap();
        let users = snapshots.iter().find(|s| s.name == "users").unwrap();
        let name_col = users.columns.iter().find(|c| c.name == "name").unwrap();
        assert!(name_col.nullable);
    }

    /// Insert a `User` row through the live `Database` API and return its
    /// id. Used to seed v1 records before applying a column-mutating
    /// migration in tests below.
    fn insert_user(db: &WasmDbmsDatabase<'_, HeapMemoryProvider>, id: u32, name: &str) {
        use wasm_dbms_api::prelude::Database;
        db.insert::<User>(UserInsertRequest {
            id: Uint32(id),
            name: Text(name.to_string()),
        })
        .unwrap();
    }

    /// Read the stored snapshot for `name`.
    fn stored_snapshot(ctx: &DbmsContext<HeapMemoryProvider>, name: &str) -> TableSchemaSnapshot {
        ctx.schema_registry
            .borrow()
            .stored_snapshots(&mut *ctx.mm.borrow_mut())
            .unwrap()
            .into_iter()
            .find(|s| s.name == name)
            .expect("stored snapshot not found")
    }

    /// Read every record under `snapshot` via the snapshot codec.
    fn read_rows_under(
        ctx: &DbmsContext<HeapMemoryProvider>,
        snapshot: &TableSchemaSnapshot,
    ) -> Vec<Vec<(String, Value)>> {
        use crate::database::migration::codec::decode_record_by_snapshot;
        let pages = ctx
            .schema_registry
            .borrow()
            .table_registry_page_by_name(&snapshot.name)
            .expect("table not found");
        let mut mm = ctx.mm.borrow_mut();
        let registry = TableRegistry::load(pages, &mut *mm).unwrap();
        let mut reader = registry.iter_raw(snapshot.alignment as u16, &mut *mm);
        let mut rows = Vec::new();
        while let Some(row) = reader.try_next().unwrap() {
            rows.push(decode_record_by_snapshot(&row.bytes, snapshot).unwrap());
        }
        rows
    }

    #[test]
    fn test_add_column_nullable_backfills_null() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }

        let new_col = ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        };
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::AddColumn {
                    table: "users".to_string(),
                    column: new_col.clone(),
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let email = rows[0]
            .iter()
            .find(|(n, _)| n == "email")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(email, &Value::Null);
    }

    #[test]
    fn test_add_column_with_default_attribute_backfills_value() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }

        let default_value = Value::Text(Text("guest@example.com".into()));
        let new_col = ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: Some(default_value.clone()),
        };
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::AddColumn {
                    table: "users".to_string(),
                    column: new_col.clone(),
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let email = rows[0]
            .iter()
            .find(|(n, _)| n == "email")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(email, &default_value);
    }

    #[test]
    fn test_add_column_non_null_no_default_returns_default_missing() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }

        let bad_col = ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        };
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
        let result = apply(
            &db,
            vec![MigrationOp::AddColumn {
                table: "users".to_string(),
                column: bad_col,
            }],
        );
        assert!(matches!(
            result,
            Err(DbmsError::Migration(MigrationError::DefaultMissing { .. }))
        ));
    }

    #[test]
    fn test_add_column_unique_default_is_rejected_and_rolled_back() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
            insert_user(&db, 2, "bob");
        }

        let result = {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::AddColumn {
                    table: "users".to_string(),
                    column: ColumnSnapshot {
                        name: "email".to_string(),
                        data_type: DataTypeSnapshot::Text,
                        nullable: false,
                        auto_increment: false,
                        unique: true,
                        primary_key: false,
                        foreign_key: None,
                        default: Some(Value::Text(Text("same@example.com".into()))),
                    },
                }],
            )
        };
        assert!(matches!(
            result,
            Err(DbmsError::Migration(
                MigrationError::ConstraintViolation { .. }
            ))
        ));

        let snap = stored_snapshot(&ctx, "users");
        assert!(!snap.columns.iter().any(|c| c.name == "email"));
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn test_drop_column_removes_value_from_existing_records() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::DropColumn {
                    table: "users".to_string(),
                    column: "name".to_string(),
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        assert!(!snap.columns.iter().any(|c| c.name == "name"));
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].iter().all(|(n, _)| n != "name"));
        let id = rows[0]
            .iter()
            .find(|(n, _)| n == "id")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(id, &Value::Uint32(Uint32(1)));
    }

    #[test]
    fn test_rename_column_preserves_data_under_new_name() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 1, "alice");
        }
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::RenameColumn {
                    table: "users".to_string(),
                    old: "name".to_string(),
                    new: "username".to_string(),
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        assert!(snap.columns.iter().any(|c| c.name == "username"));
        assert!(!snap.columns.iter().any(|c| c.name == "name"));
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let username = rows[0]
            .iter()
            .find(|(n, _)| n == "username")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(username, &Value::Text(Text("alice".into())));
    }

    #[test]
    fn test_widen_column_uint32_to_uint64_preserves_values() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 7, "alice");
        }
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![MigrationOp::WidenColumn {
                    table: "users".to_string(),
                    column: "id".to_string(),
                    old_type: DataTypeSnapshot::Uint32,
                    new_type: DataTypeSnapshot::Uint64,
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        let id_col = snap.columns.iter().find(|c| c.name == "id").unwrap();
        assert_eq!(id_col.data_type, DataTypeSnapshot::Uint64);
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let id = rows[0]
            .iter()
            .find(|(n, _)| n == "id")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(id, &Value::Uint64(wasm_dbms_api::prelude::Uint64(7)));
    }

    #[test]
    fn test_widen_column_incompatible_returns_widening_incompatible() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 7, "alice");
        }
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
        let result = apply(
            &db,
            vec![MigrationOp::WidenColumn {
                table: "users".to_string(),
                column: "id".to_string(),
                old_type: DataTypeSnapshot::Uint32,
                new_type: DataTypeSnapshot::Uint8,
            }],
        );
        assert!(matches!(
            result,
            Err(DbmsError::Migration(
                MigrationError::WideningIncompatible { .. }
            ))
        ));
    }

    /// Schema fixture with a custom `Migrate::transform_column` impl that
    /// converts `Uint32` ids into prefixed `Text` "user-N" values.
    mod transform_fixture {
        use wasm_dbms_api::prelude::Migrate;

        use super::*;

        #[derive(Debug, Table, Clone, PartialEq, Eq)]
        #[table = "users"]
        #[migrate]
        pub struct User {
            #[primary_key]
            pub id: Uint32,
            pub name: Text,
        }

        impl Migrate for User {
            fn transform_column(column: &str, old: Value) -> DbmsResult<Option<Value>> {
                if column == "id"
                    && let Value::Uint32(Uint32(n)) = old
                {
                    return Ok(Some(Value::Text(Text(format!("user-{n}")))));
                }
                Ok(None)
            }
        }

        #[derive(DatabaseSchema)]
        #[tables(User = "users")]
        pub struct UserSchema;
    }

    #[test]
    fn test_transform_column_invokes_migrate_transform_per_row() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        transform_fixture::UserSchema::register_tables(&ctx).unwrap();
        {
            use transform_fixture::User as TUser;
            let db = WasmDbmsDatabase::oneshot(&ctx, transform_fixture::UserSchema);
            use wasm_dbms_api::prelude::Database;
            db.insert::<TUser>(transform_fixture::UserInsertRequest {
                id: Uint32(7),
                name: Text("alice".into()),
            })
            .unwrap();
        }
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, transform_fixture::UserSchema);
            apply(
                &db,
                vec![MigrationOp::TransformColumn {
                    table: "users".to_string(),
                    column: "id".to_string(),
                    old_type: DataTypeSnapshot::Uint32,
                    new_type: DataTypeSnapshot::Text,
                }],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        let id_col = snap.columns.iter().find(|c| c.name == "id").unwrap();
        assert_eq!(id_col.data_type, DataTypeSnapshot::Text);
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let id = rows[0]
            .iter()
            .find(|(n, _)| n == "id")
            .map(|(_, v)| v)
            .unwrap();
        assert_eq!(id, &Value::Text(Text("user-7".into())));
    }

    /// Two-table schema (users + posts) for FK-related tests.
    mod fk_fixture {
        use super::*;

        #[derive(Debug, Table, Clone, PartialEq, Eq)]
        #[table = "users"]
        pub struct User {
            #[primary_key]
            pub id: Uint32,
            pub name: Text,
        }

        #[derive(Debug, Table, Clone, PartialEq, Eq)]
        #[table = "posts"]
        pub struct Post {
            #[primary_key]
            pub id: Uint32,
            pub owner: Uint32,
            pub title: Text,
        }

        #[derive(DatabaseSchema)]
        #[tables(User = "users", Post = "posts")]
        pub struct Schema;
    }

    #[test]
    fn test_add_column_fk_invalid_default_is_rejected_and_rolled_back() {
        use wasm_dbms_api::prelude::{Database, ForeignKeySnapshot, OnDeleteSnapshot};

        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        fk_fixture::Schema::register_tables(&ctx).unwrap();

        {
            let db = WasmDbmsDatabase::oneshot(&ctx, fk_fixture::Schema);
            db.insert::<fk_fixture::User>(fk_fixture::UserInsertRequest {
                id: Uint32(1),
                name: Text("alice".into()),
            })
            .unwrap();
            db.insert::<fk_fixture::Post>(fk_fixture::PostInsertRequest {
                id: Uint32(10),
                owner: Uint32(1),
                title: Text("x".into()),
            })
            .unwrap();
        }

        let result = {
            let db = WasmDbmsDatabase::oneshot(&ctx, fk_fixture::Schema);
            apply(
                &db,
                vec![MigrationOp::AddColumn {
                    table: "posts".to_string(),
                    column: ColumnSnapshot {
                        name: "reviewer".to_string(),
                        data_type: DataTypeSnapshot::Uint32,
                        nullable: false,
                        auto_increment: false,
                        unique: false,
                        primary_key: false,
                        foreign_key: Some(ForeignKeySnapshot {
                            table: "users".to_string(),
                            column: "id".to_string(),
                            on_delete: OnDeleteSnapshot::Restrict,
                        }),
                        default: Some(Value::Uint32(Uint32(99))),
                    },
                }],
            )
        };
        assert!(matches!(
            result,
            Err(DbmsError::Migration(
                MigrationError::ForeignKeyViolation { .. }
            ))
        ));

        let snap = stored_snapshot(&ctx, "posts");
        assert!(!snap.columns.iter().any(|c| c.name == "reviewer"));
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn test_multi_op_apply_is_atomic() {
        // Apply an AddColumn + RenameColumn + WidenColumn sequence in one
        // call and verify the final state is consistent across all three
        // ops.
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 7, "alice");
        }

        let new_email = ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: Some(Value::Text(Text("missing@example.com".into()))),
        };
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            apply(
                &db,
                vec![
                    MigrationOp::AddColumn {
                        table: "users".to_string(),
                        column: new_email.clone(),
                    },
                    MigrationOp::RenameColumn {
                        table: "users".to_string(),
                        old: "name".to_string(),
                        new: "username".to_string(),
                    },
                    MigrationOp::WidenColumn {
                        table: "users".to_string(),
                        column: "id".to_string(),
                        old_type: DataTypeSnapshot::Uint32,
                        new_type: DataTypeSnapshot::Uint64,
                    },
                ],
            )
            .unwrap();
        }

        let snap = stored_snapshot(&ctx, "users");
        assert!(snap.columns.iter().any(|c| c.name == "email"));
        assert!(snap.columns.iter().any(|c| c.name == "username"));
        assert!(!snap.columns.iter().any(|c| c.name == "name"));
        assert_eq!(
            snap.columns
                .iter()
                .find(|c| c.name == "id")
                .unwrap()
                .data_type,
            DataTypeSnapshot::Uint64
        );

        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        let id = row.iter().find(|(n, _)| n == "id").unwrap();
        assert_eq!(id.1, Value::Uint64(wasm_dbms_api::prelude::Uint64(7)));
        let username = row.iter().find(|(n, _)| n == "username").unwrap();
        assert_eq!(username.1, Value::Text(Text("alice".into())));
        let email = row.iter().find(|(n, _)| n == "email").unwrap();
        assert_eq!(email.1, Value::Text(Text("missing@example.com".into())));
    }

    #[test]
    fn test_widen_incompatible_rolls_back_data_rewrite() {
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 7, "alice");
        }

        // Sequence: a valid AddColumn, then an invalid WidenColumn that
        // must abort the apply pass and roll back the AddColumn rewrite.
        let bad_widen = MigrationOp::WidenColumn {
            table: "users".to_string(),
            column: "id".to_string(),
            old_type: DataTypeSnapshot::Uint32,
            new_type: DataTypeSnapshot::Uint8,
        };
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            let result = apply(
                &db,
                vec![
                    MigrationOp::AddColumn {
                        table: "users".to_string(),
                        column: ColumnSnapshot {
                            name: "email".to_string(),
                            data_type: DataTypeSnapshot::Text,
                            nullable: true,
                            auto_increment: false,
                            unique: false,
                            primary_key: false,
                            foreign_key: None,
                            default: None,
                        },
                    },
                    bad_widen,
                ],
            );
            assert!(matches!(
                result,
                Err(DbmsError::Migration(
                    MigrationError::WideningIncompatible { .. }
                ))
            ));
        }

        // Snapshot must be unchanged: still {id, name}.
        let snap = stored_snapshot(&ctx, "users");
        assert!(!snap.columns.iter().any(|c| c.name == "email"));
        assert!(snap.columns.iter().any(|c| c.name == "name"));
        assert_eq!(
            snap.columns
                .iter()
                .find(|c| c.name == "id")
                .unwrap()
                .data_type,
            DataTypeSnapshot::Uint32
        );
        let rows = read_rows_under(&ctx, &snap);
        assert_eq!(rows.len(), 1);
        let id = rows[0].iter().find(|(n, _)| n == "id").unwrap();
        assert_eq!(id.1, Value::Uint32(Uint32(7)));
    }

    #[test]
    fn test_indexes_rebuilt_after_column_rewrite() {
        // Schema with a secondary index on `name`.
        #[derive(Debug, Table, Clone, PartialEq, Eq)]
        #[table = "users"]
        pub struct IndexedUser {
            #[primary_key]
            pub id: Uint32,
            #[index]
            pub name: Text,
        }

        #[derive(DatabaseSchema)]
        #[tables(IndexedUser = "users")]
        pub struct Schema;

        use wasm_dbms_api::prelude::Database;

        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        Schema::register_tables(&ctx).unwrap();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, Schema);
            db.insert::<IndexedUser>(IndexedUserInsertRequest {
                id: Uint32(1),
                name: Text("alice".into()),
            })
            .unwrap();
        }
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, Schema);
            apply(
                &db,
                vec![MigrationOp::AddColumn {
                    table: "users".to_string(),
                    column: ColumnSnapshot {
                        name: "email".to_string(),
                        data_type: DataTypeSnapshot::Text,
                        nullable: true,
                        auto_increment: false,
                        unique: false,
                        primary_key: false,
                        foreign_key: None,
                        default: None,
                    },
                }],
            )
            .unwrap();
        }

        // Index on `name` should have been rebuilt against the new record
        // address. Verify by reading via the index ledger.
        let pages = ctx
            .schema_registry
            .borrow()
            .table_registry_page_by_name("users")
            .unwrap();
        let mut mm = ctx.mm.borrow_mut();
        let registry = TableRegistry::load(pages, &mut *mm).unwrap();
        let hits = registry
            .index_ledger()
            .search(
                &["name"],
                &vec![Value::Text(Text("alice".into()))],
                &mut *mm,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        // The address must be a real live record — read back via raw reader.
        let body = registry.read_raw_at(hits[0], &mut *mm).unwrap();
        assert!(!body.is_empty());
    }

    #[test]
    fn test_alter_column_add_fk_with_broken_row_returns_violation() {
        use wasm_dbms_api::prelude::{Database, ForeignKeySnapshot, OnDeleteSnapshot};

        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        fk_fixture::Schema::register_tables(&ctx).unwrap();

        let db = WasmDbmsDatabase::oneshot(&ctx, fk_fixture::Schema);
        // No users inserted. Insert a post pointing at a non-existent user.
        db.insert::<fk_fixture::User>(fk_fixture::UserInsertRequest {
            id: Uint32(1),
            name: Text("alice".into()),
        })
        .unwrap();
        db.insert::<fk_fixture::Post>(fk_fixture::PostInsertRequest {
            id: Uint32(10),
            owner: Uint32(99),
            title: Text("x".into()),
        })
        .unwrap();

        let result = apply(
            &db,
            vec![MigrationOp::AlterColumn {
                table: "posts".to_string(),
                column: "owner".to_string(),
                changes: ColumnChanges {
                    foreign_key: Some(Some(ForeignKeySnapshot {
                        table: "users".to_string(),
                        column: "id".to_string(),
                        on_delete: OnDeleteSnapshot::Restrict,
                    })),
                    ..Default::default()
                },
            }],
        );
        assert!(matches!(
            result,
            Err(DbmsError::Migration(
                MigrationError::ForeignKeyViolation { .. }
            ))
        ));
    }

    #[test]
    fn test_alter_column_add_fk_with_valid_rows_succeeds() {
        use wasm_dbms_api::prelude::{Database, ForeignKeySnapshot, OnDeleteSnapshot};

        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        fk_fixture::Schema::register_tables(&ctx).unwrap();

        let db = WasmDbmsDatabase::oneshot(&ctx, fk_fixture::Schema);
        db.insert::<fk_fixture::User>(fk_fixture::UserInsertRequest {
            id: Uint32(1),
            name: Text("alice".into()),
        })
        .unwrap();
        db.insert::<fk_fixture::Post>(fk_fixture::PostInsertRequest {
            id: Uint32(10),
            owner: Uint32(1),
            title: Text("x".into()),
        })
        .unwrap();

        apply(
            &db,
            vec![MigrationOp::AlterColumn {
                table: "posts".to_string(),
                column: "owner".to_string(),
                changes: ColumnChanges {
                    foreign_key: Some(Some(ForeignKeySnapshot {
                        table: "users".to_string(),
                        column: "id".to_string(),
                        on_delete: OnDeleteSnapshot::Restrict,
                    })),
                    ..Default::default()
                },
            }],
        )
        .unwrap();
    }

    #[test]
    fn test_transform_column_returning_none_errors() {
        // Default UserSchema's Migrate impl returns Ok(None) — should fail.
        let ctx = fresh_db();
        {
            let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
            insert_user(&db, 7, "alice");
        }
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);
        let result = apply(
            &db,
            vec![MigrationOp::TransformColumn {
                table: "users".to_string(),
                column: "id".to_string(),
                old_type: DataTypeSnapshot::Uint32,
                new_type: DataTypeSnapshot::Text,
            }],
        );
        assert!(matches!(
            result,
            Err(DbmsError::Migration(
                MigrationError::TransformReturnedNone { .. }
            ))
        ));
    }

    #[test]
    fn test_apply_clears_drift_flag_on_success() {
        let ctx = fresh_db();
        ctx.set_drift(true);
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        apply(&db, vec![]).unwrap();
        assert!(ctx.cached_drift().is_none());
    }

    /// Touch the `MigrationPolicy` import so the test module compiles cleanly
    /// when the policy is later threaded through the apply call signature.
    #[test]
    fn test_migration_policy_default_remains_non_destructive() {
        assert!(!MigrationPolicy::default().allow_destructive);
    }
}

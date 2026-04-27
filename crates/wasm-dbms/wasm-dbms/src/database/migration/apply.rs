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
//! - `DropTable` — removes the entry from the schema registry; pages are
//!   currently leaked (issue #90 tracks reclamation).
//! - `AlterColumn` — snapshot-only edit, plus tightening validation through
//!   the schema dispatch.
//! - `AddIndex` / `DropIndex` — snapshot-only edit; index population from
//!   existing rows is deferred to the snapshot-driven I/O follow-up.
//!
//! Column-mutating ops (`AddColumn`, `DropColumn`, `RenameColumn`,
//! `WidenColumn`, `TransformColumn`) require a snapshot-driven record
//! (de)serializer that does not yet exist; they return
//! [`MigrationError::DataRewriteUnsupported`] so callers see a clear,
//! recoverable error and can defer the migration until issue #91 lands.

use wasm_dbms_api::prelude::{
    ColumnChanges, DbmsError, DbmsResult, MigrationError, MigrationOp, Query, TableSchemaSnapshot,
};
use wasm_dbms_memory::prelude::{AccessControl, MemoryProvider};

use crate::database::WasmDbmsDatabase;

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
        MigrationOp::AddColumn { .. } => unsupported("AddColumn"),
        MigrationOp::DropColumn { .. } => unsupported("DropColumn"),
        MigrationOp::RenameColumn { .. } => unsupported("RenameColumn"),
        MigrationOp::WidenColumn { .. } => unsupported("WidenColumn"),
        MigrationOp::TransformColumn { .. } => unsupported("TransformColumn"),
    }
}

fn unsupported(op: &str) -> DbmsResult<()> {
    Err(DbmsError::Migration(
        MigrationError::DataRewriteUnsupported { op: op.to_string() },
    ))
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
    sr.register_table_from_snapshot(schema, &mut *mm)?;
    Ok(())
}

fn drop_table<M, A>(db: &WasmDbmsDatabase<'_, M, A>, name: &str) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut sr = db.ctx.schema_registry.borrow_mut();
    let mut mm = db.ctx.mm.borrow_mut();
    sr.unregister_table(name, &mut *mm)?;
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
        target.foreign_key = fk.clone();
    }

    persist_pending_snapshot(touched, snapshot);
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
        let mut registry = TableRegistry::load(pages, &mut *mm)?;
        registry.schema_snapshot_ledger_mut().write(
            pages.schema_snapshot_page,
            snapshot,
            &mut *mm,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnSnapshot, DataTypeSnapshot, IndexSnapshot, MigrationOp, MigrationPolicy,
        TableSchemaSnapshot, Text, Uint32,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;
    use crate::context::DbmsContext;

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

    #[test]
    fn test_data_rewrite_op_returns_unsupported_error() {
        let ctx = fresh_db();
        let db = WasmDbmsDatabase::oneshot(&ctx, UserSchema);

        let result = apply(
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
        );
        assert!(matches!(
            result,
            Err(DbmsError::Migration(MigrationError::DataRewriteUnsupported { ref op })) if op == "AddColumn"
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

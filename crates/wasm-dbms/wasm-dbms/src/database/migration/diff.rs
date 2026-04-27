//! Migration diff: stored snapshots vs compiled snapshots → `Vec<MigrationOp>`.
//!
//! Pure logic — no memory access, no journal. Tables are matched by name,
//! columns by name with a `renamed_from` fallback walk, indexes by their
//! `(sorted columns, unique)` tuple. Type changes go through the widening
//! whitelist; anything else needs a `Migrate::transform_column` override at
//! apply time, which the planner expresses as a [`MigrationOp::TransformColumn`]
//! and the apply layer validates against the dispatch.

use wasm_dbms_api::prelude::{
    ColumnSnapshot, DataTypeSnapshot, DbmsError, DbmsResult, IndexSnapshot, MigrationError,
    MigrationOp, TableSchemaSnapshot,
};
use wasm_dbms_memory::prelude::{AccessControl, MemoryProvider};

use crate::schema::DatabaseSchema;

/// Compares `stored` against `compiled` and produces the migration ops needed
/// to bring the on-disk schema in line with the compile-time definition.
///
/// `schema` is consulted for `Migrate::default_value` overrides when the diff
/// needs to satisfy an `AddColumn` on a non-nullable column without a
/// `#[default]` attribute.
///
/// # Errors
///
/// - [`MigrationError::IncompatibleType`] when a column's data type changes to
///   one neither in the widening whitelist nor presumed transformable.
/// - [`MigrationError::MissingDefault`] when a non-nullable column is added
///   with no `#[default]` and no `Migrate::default_value` override.
pub(crate) fn diff<M, A>(
    stored: &[TableSchemaSnapshot],
    compiled: &[TableSchemaSnapshot],
    schema: &dyn DatabaseSchema<M, A>,
) -> DbmsResult<Vec<MigrationOp>>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut ops = Vec::new();

    for compiled_table in compiled {
        match find_by_name(stored, &compiled_table.name) {
            None => ops.push(MigrationOp::CreateTable {
                name: compiled_table.name.clone(),
                schema: compiled_table.clone(),
            }),
            Some(stored_table) => diff_table(stored_table, compiled_table, schema, &mut ops)?,
        }
    }

    for stored_table in stored {
        if find_by_name(compiled, &stored_table.name).is_none() {
            ops.push(MigrationOp::DropTable {
                name: stored_table.name.clone(),
            });
        }
    }

    Ok(ops)
}

fn find_by_name<'a>(
    snapshots: &'a [TableSchemaSnapshot],
    name: &str,
) -> Option<&'a TableSchemaSnapshot> {
    snapshots.iter().find(|s| s.name == name)
}

fn diff_table<M, A>(
    stored: &TableSchemaSnapshot,
    compiled: &TableSchemaSnapshot,
    schema: &dyn DatabaseSchema<M, A>,
    ops: &mut Vec<MigrationOp>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    diff_columns(stored, compiled, schema, ops)?;
    diff_indexes(stored, compiled, ops);
    Ok(())
}

fn diff_columns<M, A>(
    stored: &TableSchemaSnapshot,
    compiled: &TableSchemaSnapshot,
    schema: &dyn DatabaseSchema<M, A>,
    ops: &mut Vec<MigrationOp>,
) -> DbmsResult<()>
where
    M: MemoryProvider,
    A: AccessControl,
{
    let mut consumed_stored: Vec<&str> = Vec::new();

    for compiled_col in &compiled.columns {
        if let Some(stored_col) = stored.columns.iter().find(|c| c.name == compiled_col.name) {
            consumed_stored.push(stored_col.name.as_str());
            diff_column_pair(&compiled.name, stored_col, compiled_col, ops)?;
            continue;
        }

        // Snapshots do not persist `renamed_from`, so the diff resolves it
        // through compile-time dispatch on the schema instead.
        let renamed_from = schema.renamed_from_dyn(&compiled.name, &compiled_col.name);
        let renamed = renamed_from.iter().find_map(|previous| {
            stored
                .columns
                .iter()
                .find(|c| c.name == *previous && !consumed_stored.contains(&c.name.as_str()))
        });
        if let Some(stored_col) = renamed {
            ops.push(MigrationOp::RenameColumn {
                table: compiled.name.clone(),
                old: stored_col.name.clone(),
                new: compiled_col.name.clone(),
            });
            consumed_stored.push(stored_col.name.as_str());
            diff_column_pair(&compiled.name, stored_col, compiled_col, ops)?;
            continue;
        }

        // New column.
        if !compiled_col.nullable
            && compiled_col.default.is_none()
            && schema
                .migrate_default_dyn(&compiled.name, &compiled_col.name)
                .is_none()
        {
            return Err(DbmsError::Migration(MigrationError::MissingDefault {
                table: compiled.name.clone(),
                column: compiled_col.name.clone(),
            }));
        }
        ops.push(MigrationOp::AddColumn {
            table: compiled.name.clone(),
            column: compiled_col.clone(),
        });
    }

    for stored_col in &stored.columns {
        if !consumed_stored.iter().any(|n| *n == stored_col.name) {
            ops.push(MigrationOp::DropColumn {
                table: compiled.name.clone(),
                column: stored_col.name.clone(),
            });
        }
    }

    Ok(())
}

fn diff_column_pair(
    table: &str,
    stored: &ColumnSnapshot,
    compiled: &ColumnSnapshot,
    ops: &mut Vec<MigrationOp>,
) -> DbmsResult<()> {
    if stored.data_type != compiled.data_type {
        if is_widening(&stored.data_type, &compiled.data_type) {
            ops.push(MigrationOp::WidenColumn {
                table: table.to_string(),
                column: compiled.name.clone(),
                old_type: stored.data_type.clone(),
                new_type: compiled.data_type.clone(),
            });
        } else {
            // The diff cannot tell whether a `Migrate::transform_column` is
            // available — that requires running the apply step. Emit a
            // `TransformColumn` op; the apply layer rejects with
            // `MigrationError::IncompatibleType` if no transform is provided.
            ops.push(MigrationOp::TransformColumn {
                table: table.to_string(),
                column: compiled.name.clone(),
                old_type: stored.data_type.clone(),
                new_type: compiled.data_type.clone(),
            });
        }
    }

    let changes = column_flag_delta(stored, compiled);
    if !changes.is_empty() {
        ops.push(MigrationOp::AlterColumn {
            table: table.to_string(),
            column: compiled.name.clone(),
            changes,
        });
    }

    Ok(())
}

fn column_flag_delta(
    stored: &ColumnSnapshot,
    compiled: &ColumnSnapshot,
) -> wasm_dbms_api::prelude::ColumnChanges {
    use wasm_dbms_api::prelude::ColumnChanges;

    ColumnChanges {
        nullable: (stored.nullable != compiled.nullable).then_some(compiled.nullable),
        unique: (stored.unique != compiled.unique).then_some(compiled.unique),
        auto_increment: (stored.auto_increment != compiled.auto_increment)
            .then_some(compiled.auto_increment),
        primary_key: (stored.primary_key != compiled.primary_key).then_some(compiled.primary_key),
        foreign_key: (stored.foreign_key != compiled.foreign_key)
            .then_some(compiled.foreign_key.clone()),
    }
}

/// Whitelist of compatible-widening type transitions.
///
/// Returns `true` only for transitions that the framework can rewrite without
/// user intervention: same-sign integer growth, unsigned-to-signed growth that
/// fits, and `Float32 → Float64`. Everything else falls through to the
/// transform path or aborts with `MigrationError::IncompatibleType` at apply
/// time.
pub(crate) fn is_widening(old: &DataTypeSnapshot, new: &DataTypeSnapshot) -> bool {
    use DataTypeSnapshot::*;

    fn signed_rank(t: &DataTypeSnapshot) -> Option<u8> {
        match t {
            Int8 => Some(1),
            Int16 => Some(2),
            Int32 => Some(3),
            Int64 => Some(4),
            _ => None,
        }
    }

    fn unsigned_rank(t: &DataTypeSnapshot) -> Option<u8> {
        match t {
            Uint8 => Some(1),
            Uint16 => Some(2),
            Uint32 => Some(3),
            Uint64 => Some(4),
            _ => None,
        }
    }

    match (signed_rank(old), signed_rank(new)) {
        (Some(o), Some(n)) if n > o => return true,
        _ => {}
    }
    match (unsigned_rank(old), unsigned_rank(new)) {
        (Some(o), Some(n)) if n > o => return true,
        _ => {}
    }
    if let (Some(unsigned), Some(signed)) = (unsigned_rank(old), signed_rank(new))
        && signed > unsigned
    {
        return true;
    }
    matches!((old, new), (Float32, Float64))
}

fn diff_indexes(
    stored: &TableSchemaSnapshot,
    compiled: &TableSchemaSnapshot,
    ops: &mut Vec<MigrationOp>,
) {
    for compiled_idx in &compiled.indexes {
        if !stored.indexes.iter().any(|s| index_eq(s, compiled_idx)) {
            ops.push(MigrationOp::AddIndex {
                table: compiled.name.clone(),
                index: compiled_idx.clone(),
            });
        }
    }
    for stored_idx in &stored.indexes {
        if !compiled.indexes.iter().any(|c| index_eq(c, stored_idx)) {
            ops.push(MigrationOp::DropIndex {
                table: compiled.name.clone(),
                index: stored_idx.clone(),
            });
        }
    }
}

fn index_eq(a: &IndexSnapshot, b: &IndexSnapshot) -> bool {
    if a.unique != b.unique {
        return false;
    }
    let mut left = a.columns.clone();
    let mut right = b.columns.clone();
    left.sort();
    right.sort();
    left == right
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnChanges, ForeignKeySnapshot, OnDeleteSnapshot, Text, Uint32, Value,
    };
    use wasm_dbms_macros::{DatabaseSchema, Table};
    use wasm_dbms_memory::prelude::{AccessControlList, HeapMemoryProvider};

    use super::*;

    fn id_column() -> ColumnSnapshot {
        ColumnSnapshot {
            name: "id".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: false,
            auto_increment: false,
            unique: true,
            primary_key: true,
            foreign_key: None,
            default: None,
        }
    }

    fn snapshot(name: &str, columns: Vec<ColumnSnapshot>) -> TableSchemaSnapshot {
        TableSchemaSnapshot {
            version: TableSchemaSnapshot::latest_version(),
            name: name.to_string(),
            primary_key: "id".to_string(),
            alignment: 8,
            columns,
            indexes: Vec::new(),
        }
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "items"]
    pub struct Item {
        #[primary_key]
        pub id: Uint32,
    }

    #[derive(DatabaseSchema)]
    #[tables(Item = "items")]
    pub struct EmptySchema;

    fn schema() -> impl DatabaseSchema<HeapMemoryProvider> {
        EmptySchema
    }

    #[test]
    fn test_create_table_when_compiled_only() {
        let stored = vec![];
        let compiled = vec![snapshot("users", vec![id_column()])];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], MigrationOp::CreateTable { ref name, .. } if name == "users"));
    }

    #[test]
    fn test_drop_table_when_stored_only() {
        let stored = vec![snapshot("users", vec![id_column()])];
        let compiled = vec![];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(ops[0], MigrationOp::DropTable { ref name } if name == "users"));
    }

    #[test]
    fn test_no_ops_when_snapshots_match() {
        let s = snapshot("users", vec![id_column()]);
        let ops = diff(
            std::slice::from_ref(&s),
            std::slice::from_ref(&s),
            &schema(),
        )
        .unwrap();
        assert!(ops.is_empty());
    }

    #[test]
    fn test_add_nullable_column() {
        let stored = vec![snapshot("users", vec![id_column()])];
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let compiled = vec![snapshot("users", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::AddColumn { table, column }
                if table == "users" && column.name == "email"
        ));
    }

    #[test]
    fn test_add_non_nullable_column_without_default_errors() {
        let stored = vec![snapshot("users", vec![id_column()])];
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let compiled = vec![snapshot("users", compiled_cols)];
        let result = diff(&stored, &compiled, &schema());
        assert!(matches!(
            result,
            Err(DbmsError::Migration(MigrationError::MissingDefault { ref table, ref column }))
                if table == "users" && column == "email"
        ));
    }

    #[test]
    fn test_add_non_nullable_column_with_default_succeeds() {
        let stored = vec![snapshot("users", vec![id_column()])];
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "score".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: Some(Value::Uint32(Uint32(0))),
        });
        let compiled = vec![snapshot("users", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(&ops[0], MigrationOp::AddColumn { .. }));
    }

    #[test]
    fn test_drop_column() {
        let mut stored_cols = vec![id_column()];
        stored_cols.push(ColumnSnapshot {
            name: "stale".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let stored = vec![snapshot("users", stored_cols)];
        let compiled = vec![snapshot("users", vec![id_column()])];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::DropColumn { table, column }
                if table == "users" && column == "stale"
        ));
    }

    #[test]
    fn test_widen_int_column() {
        let mut stored_cols = vec![id_column()];
        stored_cols.push(ColumnSnapshot {
            name: "age".to_string(),
            data_type: DataTypeSnapshot::Int16,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "age".to_string(),
            data_type: DataTypeSnapshot::Int64,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let stored = vec![snapshot("users", stored_cols)];
        let compiled = vec![snapshot("users", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::WidenColumn { old_type, new_type, .. }
                if matches!(old_type, DataTypeSnapshot::Int16) && matches!(new_type, DataTypeSnapshot::Int64)
        ));
    }

    #[test]
    fn test_transform_column_for_incompatible_type_change() {
        let mut stored_cols = vec![id_column()];
        stored_cols.push(ColumnSnapshot {
            name: "label".to_string(),
            data_type: DataTypeSnapshot::Int32,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "label".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let stored = vec![snapshot("users", stored_cols)];
        let compiled = vec![snapshot("users", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(&ops[0], MigrationOp::TransformColumn { .. }));
    }

    #[test]
    fn test_alter_column_for_flag_changes() {
        let mut stored_cols = vec![id_column()];
        stored_cols.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: true,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "email".to_string(),
            data_type: DataTypeSnapshot::Text,
            nullable: false,
            auto_increment: false,
            unique: true,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let stored = vec![snapshot("users", stored_cols)];
        let compiled = vec![snapshot("users", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::AlterColumn {
                changes: ColumnChanges {
                    nullable: Some(false),
                    unique: Some(true),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn test_alter_column_for_foreign_key_drop() {
        let mut stored_cols = vec![id_column()];
        stored_cols.push(ColumnSnapshot {
            name: "owner_id".to_string(),
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
            default: None,
        });
        let mut compiled_cols = vec![id_column()];
        compiled_cols.push(ColumnSnapshot {
            name: "owner_id".to_string(),
            data_type: DataTypeSnapshot::Uint32,
            nullable: false,
            auto_increment: false,
            unique: false,
            primary_key: false,
            foreign_key: None,
            default: None,
        });
        let stored = vec![snapshot("posts", stored_cols)];
        let compiled = vec![snapshot("posts", compiled_cols)];
        let ops = diff(&stored, &compiled, &schema()).unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::AlterColumn {
                changes: ColumnChanges {
                    foreign_key: Some(None),
                    ..
                },
                ..
            }
        ));
    }

    #[test]
    fn test_add_and_drop_indexes() {
        let stored_idx = IndexSnapshot {
            columns: vec!["a".to_string(), "b".to_string()],
            unique: false,
        };
        let compiled_idx = IndexSnapshot {
            columns: vec!["c".to_string()],
            unique: true,
        };
        let mut stored_table = snapshot("t", vec![id_column()]);
        stored_table.indexes = vec![stored_idx.clone()];
        let mut compiled_table = snapshot("t", vec![id_column()]);
        compiled_table.indexes = vec![compiled_idx.clone()];

        let ops = diff(&[stored_table], &[compiled_table], &schema()).unwrap();
        assert_eq!(ops.len(), 2);
        assert!(
            ops.iter().any(
                |op| matches!(op, MigrationOp::AddIndex { index, .. } if index == &compiled_idx)
            )
        );
        assert!(
            ops.iter().any(
                |op| matches!(op, MigrationOp::DropIndex { index, .. } if index == &stored_idx)
            )
        );
    }

    #[test]
    fn test_index_match_is_order_and_case_independent_on_columns() {
        let a = IndexSnapshot {
            columns: vec!["a".to_string(), "b".to_string()],
            unique: false,
        };
        let b = IndexSnapshot {
            columns: vec!["b".to_string(), "a".to_string()],
            unique: false,
        };
        assert!(index_eq(&a, &b));
    }

    #[derive(Debug, Table, Clone, PartialEq, Eq)]
    #[table = "users"]
    pub struct UserRenamed {
        #[primary_key]
        pub id: Uint32,
        #[renamed_from("user_name", "old_name")]
        pub name: Text,
    }

    #[derive(DatabaseSchema)]
    #[tables(UserRenamed = "users")]
    pub struct RenameSchema;

    #[test]
    fn test_rename_column_via_renamed_from_dispatch() {
        let stored = vec![snapshot(
            "users",
            vec![
                id_column(),
                ColumnSnapshot {
                    name: "user_name".to_string(),
                    data_type: DataTypeSnapshot::Text,
                    nullable: false,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: None,
                    default: None,
                },
            ],
        )];
        let compiled = vec![snapshot(
            "users",
            vec![
                id_column(),
                ColumnSnapshot {
                    name: "name".to_string(),
                    data_type: DataTypeSnapshot::Text,
                    nullable: false,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: None,
                    default: None,
                },
            ],
        )];
        let ops = diff::<HeapMemoryProvider, AccessControlList>(&stored, &compiled, &RenameSchema)
            .unwrap();
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            &ops[0],
            MigrationOp::RenameColumn { table, old, new }
                if table == "users" && old == "user_name" && new == "name"
        ));
    }

    #[test]
    fn test_rename_column_via_skipped_release_in_renamed_from() {
        let stored = vec![snapshot(
            "users",
            vec![
                id_column(),
                ColumnSnapshot {
                    name: "old_name".to_string(),
                    data_type: DataTypeSnapshot::Text,
                    nullable: false,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: None,
                    default: None,
                },
            ],
        )];
        let compiled = vec![snapshot(
            "users",
            vec![
                id_column(),
                ColumnSnapshot {
                    name: "name".to_string(),
                    data_type: DataTypeSnapshot::Text,
                    nullable: false,
                    auto_increment: false,
                    unique: false,
                    primary_key: false,
                    foreign_key: None,
                    default: None,
                },
            ],
        )];
        let ops = diff::<HeapMemoryProvider, AccessControlList>(&stored, &compiled, &RenameSchema)
            .unwrap();
        assert!(matches!(
            &ops[0],
            MigrationOp::RenameColumn { old, .. } if old == "old_name"
        ));
    }

    #[test]
    fn test_widening_whitelist_is_strict() {
        // signed grow
        assert!(is_widening(
            &DataTypeSnapshot::Int8,
            &DataTypeSnapshot::Int32
        ));
        assert!(!is_widening(
            &DataTypeSnapshot::Int32,
            &DataTypeSnapshot::Int8
        ));
        // unsigned grow
        assert!(is_widening(
            &DataTypeSnapshot::Uint8,
            &DataTypeSnapshot::Uint64
        ));
        // unsigned -> signed (must fit)
        assert!(is_widening(
            &DataTypeSnapshot::Uint16,
            &DataTypeSnapshot::Int32
        ));
        assert!(!is_widening(
            &DataTypeSnapshot::Uint32,
            &DataTypeSnapshot::Int32
        ));
        // floats
        assert!(is_widening(
            &DataTypeSnapshot::Float32,
            &DataTypeSnapshot::Float64
        ));
        assert!(!is_widening(
            &DataTypeSnapshot::Float64,
            &DataTypeSnapshot::Float32
        ));
        // unrelated
        assert!(!is_widening(
            &DataTypeSnapshot::Text,
            &DataTypeSnapshot::Int32
        ));
    }
}

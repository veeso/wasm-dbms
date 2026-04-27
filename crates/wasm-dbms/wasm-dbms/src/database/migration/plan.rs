//! Migration plan: deterministic op ordering and policy-driven validation.
//!
//! The diff stage produces an unordered set of [`MigrationOp`]s; this module
//! sorts them into the order required for a safe apply pass, and rejects ops
//! that violate the caller-supplied [`MigrationPolicy`] before any memory
//! mutation begins. A single source of ordering keeps the apply pipeline
//! testable and reproducible across runs.

use wasm_dbms_api::prelude::{
    ColumnChanges, DbmsError, DbmsResult, MigrationError, MigrationOp, MigrationPolicy,
};

/// Deterministic apply order, sorted by the priority returned by
/// [`op_priority`].
///
/// Stable within a priority bucket, so ops produced in the same bucket retain
/// the relative order from the diff stage — handy for predictable test output
/// and easier debugging of multi-op migrations.
pub(crate) fn order_ops(ops: &mut [MigrationOp]) {
    ops.sort_by_key(op_priority);
}

/// Validates the planned ops against `policy` before any memory mutation.
///
/// Defense-in-depth: catches destructive ops the diff layer can produce
/// before the journaled apply pass opens.
///
/// # Errors
///
/// - [`MigrationError::DestructiveOpDenied`] when `policy.allow_destructive`
///   is `false` and the plan includes a `DropTable` or `DropColumn`.
pub(crate) fn validate(ops: &[MigrationOp], policy: MigrationPolicy) -> DbmsResult<()> {
    for op in ops {
        match op {
            MigrationOp::DropTable { name } if !policy.allow_destructive => {
                return Err(DbmsError::Migration(MigrationError::DestructiveOpDenied {
                    op: format!("DropTable({name})"),
                }));
            }
            MigrationOp::DropColumn { table, column } if !policy.allow_destructive => {
                return Err(DbmsError::Migration(MigrationError::DestructiveOpDenied {
                    op: format!("DropColumn({table}.{column})"),
                }));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Apply-order priority for `op`. Lower runs first.
///
/// `AlterColumn` is split into "relax" (drop nullability/uniqueness/FK) and
/// "tighten" (add nullability/uniqueness/FK) buckets so relaxations land
/// before any data rewrite that depends on the relaxed constraint, and
/// tightenings run after every other mutation has finished filling in the
/// rows they will validate.
fn op_priority(op: &MigrationOp) -> u8 {
    match op {
        MigrationOp::CreateTable { .. } => 0,
        MigrationOp::DropIndex { .. } => 1,
        MigrationOp::DropColumn { .. } => 2,
        MigrationOp::RenameColumn { .. } => 3,
        MigrationOp::AlterColumn { changes, .. } if is_relaxation(changes) => 4,
        MigrationOp::WidenColumn { .. } => 5,
        MigrationOp::TransformColumn { .. } => 6,
        MigrationOp::AddColumn { .. } => 7,
        MigrationOp::AlterColumn { .. } => 8,
        MigrationOp::AddIndex { .. } => 9,
        MigrationOp::DropTable { .. } => 10,
    }
}

/// Returns `true` when every set flag in `changes` represents a relaxation
/// (nullable→true, unique→false, drop FK). Tightenings (or mixed deltas) fall
/// into the later "tighten" bucket so they run after every data rewrite.
fn is_relaxation(changes: &ColumnChanges) -> bool {
    if let Some(false) = changes.nullable {
        return false;
    }
    if let Some(true) = changes.unique {
        return false;
    }
    if let Some(true) = changes.primary_key {
        return false;
    }
    if let Some(Some(_)) = &changes.foreign_key {
        return false;
    }
    if changes.auto_increment.is_some() {
        // Toggling auto-increment is not a pure relaxation; defer to the
        // tightening bucket for safety.
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{
        ColumnSnapshot, DataTypeSnapshot, IndexSnapshot, MigrationOp, MigrationPolicy,
    };

    use super::*;

    fn add_column(table: &str, name: &str, nullable: bool, default_set: bool) -> MigrationOp {
        let default = if default_set {
            Some(wasm_dbms_api::prelude::Value::Uint32(
                wasm_dbms_api::prelude::Uint32(0),
            ))
        } else {
            None
        };
        MigrationOp::AddColumn {
            table: table.to_string(),
            column: ColumnSnapshot {
                name: name.to_string(),
                data_type: DataTypeSnapshot::Uint32,
                nullable,
                auto_increment: false,
                unique: false,
                primary_key: false,
                foreign_key: None,
                default,
            },
        }
    }

    fn alter_relax() -> MigrationOp {
        MigrationOp::AlterColumn {
            table: "t".to_string(),
            column: "c".to_string(),
            changes: ColumnChanges {
                nullable: Some(true),
                unique: Some(false),
                ..Default::default()
            },
        }
    }

    fn alter_tighten() -> MigrationOp {
        MigrationOp::AlterColumn {
            table: "t".to_string(),
            column: "c".to_string(),
            changes: ColumnChanges {
                nullable: Some(false),
                ..Default::default()
            },
        }
    }

    fn dummy_index() -> IndexSnapshot {
        IndexSnapshot {
            columns: vec!["a".to_string()],
            unique: false,
        }
    }

    #[test]
    fn test_order_ops_sorts_by_documented_priority() {
        let mut ops = vec![
            MigrationOp::DropTable {
                name: "old".to_string(),
            },
            MigrationOp::AddIndex {
                table: "t".to_string(),
                index: dummy_index(),
            },
            alter_tighten(),
            add_column("t", "new", true, false),
            MigrationOp::TransformColumn {
                table: "t".to_string(),
                column: "x".to_string(),
                old_type: DataTypeSnapshot::Int32,
                new_type: DataTypeSnapshot::Text,
            },
            MigrationOp::WidenColumn {
                table: "t".to_string(),
                column: "y".to_string(),
                old_type: DataTypeSnapshot::Int8,
                new_type: DataTypeSnapshot::Int32,
            },
            alter_relax(),
            MigrationOp::RenameColumn {
                table: "t".to_string(),
                old: "a".to_string(),
                new: "b".to_string(),
            },
            MigrationOp::DropColumn {
                table: "t".to_string(),
                column: "z".to_string(),
            },
            MigrationOp::DropIndex {
                table: "t".to_string(),
                index: dummy_index(),
            },
            MigrationOp::CreateTable {
                name: "fresh".to_string(),
                schema: dummy_snapshot(),
            },
        ];
        order_ops(&mut ops);

        let order: Vec<u8> = ops.iter().map(op_priority).collect();
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(order, sorted, "ops not strictly ordered by priority");

        // Spot-check first and last buckets.
        assert!(matches!(
            ops.first().unwrap(),
            MigrationOp::CreateTable { .. }
        ));
        assert!(matches!(ops.last().unwrap(), MigrationOp::DropTable { .. }));
    }

    fn dummy_snapshot() -> wasm_dbms_api::prelude::TableSchemaSnapshot {
        wasm_dbms_api::prelude::TableSchemaSnapshot {
            version: wasm_dbms_api::prelude::TableSchemaSnapshot::latest_version(),
            name: "fresh".to_string(),
            primary_key: "id".to_string(),
            alignment: 8,
            columns: vec![],
            indexes: vec![],
        }
    }

    #[test]
    fn test_validate_blocks_drop_table_when_destructive_disallowed() {
        let policy = MigrationPolicy::default();
        let ops = vec![MigrationOp::DropTable {
            name: "users".to_string(),
        }];
        let result = validate(&ops, policy);
        assert!(matches!(
            result,
            Err(DbmsError::Migration(MigrationError::DestructiveOpDenied { ref op })) if op.contains("DropTable")
        ));
    }

    #[test]
    fn test_validate_blocks_drop_column_when_destructive_disallowed() {
        let policy = MigrationPolicy::default();
        let ops = vec![MigrationOp::DropColumn {
            table: "users".to_string(),
            column: "stale".to_string(),
        }];
        let result = validate(&ops, policy);
        assert!(matches!(
            result,
            Err(DbmsError::Migration(MigrationError::DestructiveOpDenied { ref op })) if op.contains("DropColumn")
        ));
    }

    #[test]
    fn test_validate_passes_with_allow_destructive() {
        let policy = MigrationPolicy {
            allow_destructive: true,
        };
        let ops = vec![
            MigrationOp::DropTable {
                name: "x".to_string(),
            },
            MigrationOp::DropColumn {
                table: "y".to_string(),
                column: "z".to_string(),
            },
        ];
        validate(&ops, policy).expect("should pass");
    }

    #[test]
    fn test_validate_allows_add_column_without_static_default() {
        let ops = vec![add_column("users", "email", false, false)];
        validate(&ops, MigrationPolicy::default()).expect("apply resolves dynamic defaults");
    }

    #[test]
    fn test_validate_passes_when_non_nullable_add_column_has_default() {
        let ops = vec![add_column("users", "score", false, true)];
        validate(&ops, MigrationPolicy::default()).expect("default present");
    }
}

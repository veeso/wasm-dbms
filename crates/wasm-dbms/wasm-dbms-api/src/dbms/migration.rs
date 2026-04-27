//! Schema migration types and traits.
//!
//! The migration subsystem lets compiled `#[derive(Table)]` schemas evolve
//! across releases without manual stable-memory surgery. The flow is:
//!
//! 1. On boot, the DBMS compares the hash of every compiled
//!    [`TableSchemaSnapshot`](crate::dbms::table::TableSchemaSnapshot) against
//!    the hash stored in the schema registry.
//! 2. If they differ, the DBMS enters drift state and refuses CRUD.
//! 3. The user calls `Dbms::migrate(policy)`, which diffs the stored snapshots
//!    against the compiled ones and produces a [`Vec<MigrationOp>`].
//! 4. The ops are applied transactionally; on success the new snapshots and
//!    schema hash are persisted and the drift flag is cleared.
//!
//! This module owns the *types* (`MigrationOp`, `ColumnChanges`,
//! `MigrationPolicy`, `MigrationError`) and the per-table extension hook
//! [`Migrate`]. The diff algorithm and apply logic live in the engine crate.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dbms::table::{ColumnSnapshot, DataTypeSnapshot, IndexSnapshot, TableSchema};
use crate::dbms::value::Value;
use crate::error::DbmsResult;

/// Per-table extension hook for schema migrations.
///
/// The derive macro `#[derive(Table)]` emits an empty `impl Migrate for T {}`
/// for every table by default, so callers only need to provide a manual impl
/// when they tag the struct with `#[migrate]`. The trait extends
/// [`TableSchema`] so implementors automatically have access to the table's
/// column definitions and snapshot.
pub trait Migrate
where
    Self: TableSchema,
{
    /// Dynamic default for an [`MigrationOp::AddColumn`] operation on a non-nullable column.
    ///
    /// Returning `None` falls back to the static `#[default = ...]` attribute
    /// declared on the column. If neither produces a value, migration aborts
    /// with [`MigrationError::MissingDefault`].
    fn default_value(_column: &str) -> Option<Value> {
        None
    }

    /// Transform a stored value when its column changes to an incompatible
    /// type that does not fit the framework's widening whitelist.
    ///
    /// - `Ok(None)` — no transform; the framework errors with
    ///   [`MigrationError::IncompatibleType`] unless widening already applies.
    /// - `Ok(Some(v))` — use `v` as the new value.
    /// - `Err(_)` — abort the migration; the journaled session rolls back.
    fn transform_column(_column: &str, _old: Value) -> DbmsResult<Option<Value>> {
        Ok(None)
    }
}

/// Single atomic step produced by the migration planner.
///
/// Ops are sorted into a deterministic apply order (creates → drops → renames
/// → relaxations → widening/transforms → adds → tightenings → indexes →
/// table drops) and then executed inside a single journaled session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum MigrationOp {
    /// Create a new table with the given snapshot.
    CreateTable {
        /// Name of the new table.
        name: String,
        /// Snapshot of the compiled schema for the new table.
        schema: crate::dbms::table::TableSchemaSnapshot,
    },
    /// Drop a table and all of its data. Destructive.
    DropTable {
        /// Name of the table to drop.
        name: String,
    },
    /// Append a new column to an existing table.
    ///
    /// If the column is non-nullable, the planner must have resolved a default
    /// value (`#[default]` or [`Migrate::default_value`]) before emitting this
    /// op.
    AddColumn {
        /// Table the column belongs to.
        table: String,
        /// Snapshot of the new column.
        column: ColumnSnapshot,
    },
    /// Drop a column and discard its data. Destructive.
    DropColumn {
        /// Table the column belongs to.
        table: String,
        /// Name of the column to drop.
        column: String,
    },
    /// Rename a column, preserving its data and constraints.
    RenameColumn {
        /// Table the column belongs to.
        table: String,
        /// Previous column name as it appears in the stored snapshot.
        old: String,
        /// New column name as it appears in the compiled snapshot.
        new: String,
    },
    /// Change one or more constraint flags on an existing column.
    AlterColumn {
        /// Table the column belongs to.
        table: String,
        /// Name of the column to alter.
        column: String,
        /// Flag deltas to apply.
        changes: ColumnChanges,
    },
    /// Widen a column to a larger compatible type (sign-extend, zero-extend,
    /// `Float32` → `Float64`).
    WidenColumn {
        /// Table the column belongs to.
        table: String,
        /// Name of the column being widened.
        column: String,
        /// Stored data type before widening.
        old_type: DataTypeSnapshot,
        /// Compiled data type after widening.
        new_type: DataTypeSnapshot,
    },
    /// Convert a column to an incompatible type using
    /// [`Migrate::transform_column`].
    TransformColumn {
        /// Table the column belongs to.
        column: String,
        /// Name of the column being transformed.
        table: String,
        /// Stored data type before the transform.
        old_type: DataTypeSnapshot,
        /// Compiled data type after the transform.
        new_type: DataTypeSnapshot,
    },
    /// Build a new secondary index.
    AddIndex {
        /// Table the index belongs to.
        table: String,
        /// Snapshot of the new index.
        index: IndexSnapshot,
    },
    /// Drop an existing secondary index.
    DropIndex {
        /// Table the index belongs to.
        table: String,
        /// Snapshot of the index to drop.
        index: IndexSnapshot,
    },
}

/// Bundle of constraint-flag deltas for an [`MigrationOp::AlterColumn`].
///
/// Each field is `Some(new_value)` only when that flag changed between the
/// stored and compiled snapshots; otherwise it stays `None`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct ColumnChanges {
    /// New value for the `nullable` flag, if changed.
    pub nullable: Option<bool>,
    /// New value for the `unique` flag, if changed.
    pub unique: Option<bool>,
    /// New value for the `auto_increment` flag, if changed.
    pub auto_increment: Option<bool>,
    /// New value for the `primary_key` flag, if changed.
    pub primary_key: Option<bool>,
    /// New foreign-key state. `Some(None)` means the foreign key was dropped;
    /// `Some(Some(fk))` means it was added or replaced.
    pub foreign_key: Option<Option<crate::dbms::table::ForeignKeySnapshot>>,
}

impl ColumnChanges {
    /// Returns `true` if no flag actually changed.
    pub fn is_empty(&self) -> bool {
        self.nullable.is_none()
            && self.unique.is_none()
            && self.auto_increment.is_none()
            && self.primary_key.is_none()
            && self.foreign_key.is_none()
    }
}

/// Caller-supplied policy that gates destructive migration ops.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub struct MigrationPolicy {
    /// When `false`, the planner refuses to emit `DropTable` or `DropColumn`
    /// ops and aborts with [`MigrationError::DestructiveOpDenied`].
    pub allow_destructive: bool,
}

/// Error variants produced by the migration planner and apply pipeline.
#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
pub enum MigrationError {
    /// CRUD attempted while the DBMS is in drift state. Only ACL and
    /// migration entry points are allowed until [`MigrationOp`]s are applied.
    #[error("Schema drift: stored schema differs from compiled schema")]
    SchemaDrift,
    /// A column changed to a type that is neither in the widening whitelist
    /// nor handled by [`Migrate::transform_column`].
    #[error(
        "Incompatible type change for column `{column}` in table `{table}`: {old:?} -> {new:?}"
    )]
    IncompatibleType {
        /// Table the column belongs to.
        table: String,
        /// Name of the offending column.
        column: String,
        /// Stored data type.
        old: DataTypeSnapshot,
        /// Compiled data type.
        new: DataTypeSnapshot,
    },
    /// `AddColumn` on a non-nullable column has neither a `#[default]`
    /// attribute nor a [`Migrate::default_value`] override.
    #[error("Missing default for non-nullable new column `{column}` in table `{table}`")]
    MissingDefault {
        /// Table the column belongs to.
        table: String,
        /// Name of the offending column.
        column: String,
    },
    /// A tightening `AlterColumn` op (e.g. `nullable: false`, `unique: true`,
    /// add FK) found existing data that violates the new constraint.
    #[error("Constraint violation for column `{column}` in table `{table}`: {reason}")]
    ConstraintViolation {
        /// Table the column belongs to.
        table: String,
        /// Name of the offending column.
        column: String,
        /// Human-readable description of which row(s) violated the constraint.
        reason: String,
    },
    /// Planner produced a destructive op while
    /// [`MigrationPolicy::allow_destructive`] is `false`.
    #[error("Destructive migration op denied by policy: {op}")]
    DestructiveOpDenied {
        /// Short tag for the offending op (e.g. `"DropTable"`, `"DropColumn"`).
        op: String,
    },
    /// User-supplied [`Migrate::transform_column`] returned `Err`.
    #[error("Migration transform aborted for column `{column}` in table `{table}`: {reason}")]
    TransformAborted {
        /// Table the column belongs to.
        table: String,
        /// Name of the column being transformed.
        column: String,
        /// Reason propagated from the user transform.
        reason: String,
    },
    /// Column-mutating op (`AddColumn`, `DropColumn`, `RenameColumn`,
    /// `WidenColumn`, `TransformColumn`) requires a snapshot-driven record
    /// (de)serializer that is tracked separately and not yet wired into the
    /// apply pipeline.
    #[error("Migration op `{op}` requires snapshot-driven record rewrite (see issue #91)")]
    DataRewriteUnsupported {
        /// Short tag for the offending op kind.
        op: String,
    },
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::dbms::table::{ColumnSnapshot, DataTypeSnapshot};

    #[test]
    fn test_should_default_migration_policy_to_non_destructive() {
        let policy = MigrationPolicy::default();
        assert!(!policy.allow_destructive);
    }

    #[test]
    fn test_should_detect_empty_column_changes() {
        let changes = ColumnChanges::default();
        assert!(changes.is_empty());

        let nullable = ColumnChanges {
            nullable: Some(true),
            ..Default::default()
        };
        assert!(!nullable.is_empty());
    }

    #[test]
    fn test_should_display_migration_error() {
        let err = MigrationError::SchemaDrift;
        assert_eq!(
            err.to_string(),
            "Schema drift: stored schema differs from compiled schema"
        );

        let err = MigrationError::IncompatibleType {
            table: "users".into(),
            column: "id".into(),
            old: DataTypeSnapshot::Int32,
            new: DataTypeSnapshot::Text,
        };
        assert!(err.to_string().contains("Incompatible type change"));

        let err = MigrationError::MissingDefault {
            table: "users".into(),
            column: "email".into(),
        };
        assert!(err.to_string().contains("Missing default"));

        let err = MigrationError::ConstraintViolation {
            table: "users".into(),
            column: "email".into(),
            reason: "duplicate value".into(),
        };
        assert!(err.to_string().contains("Constraint violation"));

        let err = MigrationError::DestructiveOpDenied {
            op: "DropTable".into(),
        };
        assert!(err.to_string().contains("Destructive migration op denied"));

        let err = MigrationError::TransformAborted {
            table: "users".into(),
            column: "id".into(),
            reason: "negative ids unsupported".into(),
        };
        assert!(err.to_string().contains("Migration transform aborted"));
    }

    #[test]
    fn test_should_construct_migration_ops() {
        let _drop = MigrationOp::DropTable { name: "old".into() };
        let _add = MigrationOp::AddColumn {
            table: "users".into(),
            column: ColumnSnapshot {
                name: "email".into(),
                data_type: DataTypeSnapshot::Text,
                nullable: true,
                auto_increment: false,
                unique: false,
                primary_key: false,
                foreign_key: None,
                default: None,
            },
        };
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_roundtrip_migration_policy() {
        let policy = MigrationPolicy {
            allow_destructive: true,
        };
        let encoded = candid::encode_one(&policy).expect("failed to encode");
        let decoded: MigrationPolicy = candid::decode_one(&encoded).expect("failed to decode");
        assert_eq!(policy, decoded);
    }

    #[cfg(feature = "candid")]
    #[test]
    fn test_should_candid_roundtrip_migration_error() {
        let err = MigrationError::SchemaDrift;
        let encoded = candid::encode_one(&err).expect("failed to encode");
        let decoded: MigrationError = candid::decode_one(&encoded).expect("failed to decode");
        assert_eq!(err, decoded);
    }
}

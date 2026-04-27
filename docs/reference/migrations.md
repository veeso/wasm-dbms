# Schema Migrations

- [Schema Migrations](#schema-migrations)
  - [Overview](#overview)
  - [Lifecycle](#lifecycle)
  - [Drift Detection](#drift-detection)
  - [Schema Snapshots](#schema-snapshots)
  - [Migration Plan](#migration-plan)
    - [MigrationOp](#migrationop)
    - [Apply Order](#apply-order)
  - [Compatible Widening Whitelist](#compatible-widening-whitelist)
  - [Per-Table Hooks](#per-table-hooks)
    - [`#[default]` Attribute](#default-attribute)
    - [`#[renamed_from]` Attribute](#renamed_from-attribute)
    - [`Migrate` Trait](#migrate-trait)
  - [Migration Policy](#migration-policy)
  - [Errors](#errors)
  - [API Surface](#api-surface)
    - [Generic (`wasm-dbms`)](#generic-wasm-dbms)
    - [`DatabaseSchema` Dispatch](#databaseschema-dispatch)
    - [IC Endpoints](#ic-endpoints)
  - [Non-Goals](#non-goals)
  - [Worked Example](#worked-example)
  - [Best Practices](#best-practices)

---

## Overview

Schema migrations let `#[derive(Table)]` schemas evolve across releases without losing data or requiring manual stable-memory surgery. The framework:

1. Stores a `TableSchemaSnapshot` for every registered table on disk.
2. Hashes those snapshots into a single `schema_hash` cached on Page 0 of the schema registry.
3. On every boot, recomputes the hash from the compiled schema and compares it against the stored hash to detect drift.
4. Refuses CRUD while in drift state and waits for an explicit `dbms.migrate(policy)` call.
5. Applies the diff between stored and compiled snapshots transactionally via the journaled writer.

Migrations are **forward-only** and **explicit**. The DBMS never auto-migrates on init — the caller decides when (and whether) to run them.

---

## Lifecycle

```txt
┌─────────────────────────────────────────────────────────┐
│  boot / post_upgrade                                    │
│    ├─ load SchemaRegistry (Page 0) → read schema_hash   │
│    ├─ compute current_hash from compiled schemas        │
│    └─ drift = (stored_hash != current_hash)             │
├─────────────────────────────────────────────────────────┤
│  drift == false                                         │
│    ├─ CRUD allowed                                      │
│    └─ migrate() is a no-op                              │
├─────────────────────────────────────────────────────────┤
│  drift == true                                          │
│    ├─ CRUD returns DbmsError::Migration(SchemaDrift)    │
│    ├─ ACL methods bypass the check                      │
│    ├─ plan_migration() → Vec<MigrationOp>               │
│    └─ migrate(policy) applies ops, clears drift         │
└─────────────────────────────────────────────────────────┘
```

**Performance contract:**

- **Boot**: one `u64` read from Page 0 plus one `xxh3` hash of the encoded compiled snapshots. `O(tables × columns)`.
- **Hot path (CRUD)**: a single `bool` load (drift flag on the DBMS context) plus a branch. No snapshot decode, no hash recompute.
- **Snapshot decode**: only on `plan_migration()` or `migrate()`. Never during CRUD.

---

## Drift Detection

Drift is the only signal the DBMS uses to decide whether migration is required. It is computed once on boot:

1. Load `SchemaRegistry` from Page 0.
2. For each table in `DatabaseSchema::compiled_snapshots()`, encode the snapshot.
3. Compute `current_hash = xxh3(sorted-by-name concatenation of encoded bytes)`.
4. `drift = (schema_registry.schema_hash != current_hash)`.
5. Cache `drift: bool` on the DBMS context.

Every CRUD entry point early-returns `Err(DbmsError::Migration(MigrationError::SchemaDrift))` while `drift == true`. ACL methods (`acl_add_principal`, `acl_remove_principal`, `acl_allowed_principals`) bypass the check so the operator can recover even if the drift state is stuck.

---

## Schema Snapshots

A snapshot is a self-describing, versioned view of a table's compile-time shape. It captures only what is meaningful for migration; transient or derivable fields are intentionally omitted.

```rust
pub struct TableSchemaSnapshot {
    pub version: u8,                    // bumped on any breaking layout change
    pub name: String,
    pub primary_key: String,
    pub alignment: u32,
    pub columns: Vec<ColumnSnapshot>,   // declaration order preserved
    pub indexes: Vec<IndexSnapshot>,
}

pub struct ColumnSnapshot {
    pub name: String,
    pub data_type: DataTypeSnapshot,
    pub nullable: bool,
    pub auto_increment: bool,
    pub unique: bool,
    pub primary_key: bool,
    pub foreign_key: Option<ForeignKeySnapshot>,
    pub default: Option<Value>,
}

#[repr(u8)]
pub enum DataTypeSnapshot {
    Int8 = 0x01, Int16 = 0x02, Int32 = 0x03, Int64 = 0x04,
    Uint8 = 0x10, Uint16 = 0x11, Uint32 = 0x12, Uint64 = 0x13,
    Float32 = 0x20, Float64 = 0x21, Decimal = 0x22,
    Boolean = 0x30,
    Date = 0x40, Datetime = 0x41,
    Blob = 0x50, Text = 0x51, Uuid = 0x52,
    Json = 0x60,
    Custom { tag: String, wire_size: WireSize } = 0xF0,
}

pub enum WireSize {
    Fixed(u32),       // column occupies exactly N bytes
    LengthPrefixed,   // body preceded by 2-byte LE length prefix
}
```

`WireSize` is derived at compile time from the custom type's `Encode::SIZE`:
`DataSize::Fixed(n)` → `WireSize::Fixed(n)`, `DataSize::Dynamic` →
`WireSize::LengthPrefixed`. The migration codec uses it to slice column
bytes during a snapshot-driven rewrite without invoking the user's
`Encode::decode` impl.

**Stability rules:**

1. `DataTypeSnapshot` discriminants are **frozen**. Never reorder, never reuse a removed slot.
2. Adding a field appends at the tail and bumps the container `version`. Old readers stop at the previous length prefix.
3. Removing a field leaves the slot reserved. Do not shift later fields.
4. Wire format per struct: length-prefix + field-by-field little-endian. `String` = `u16` length + UTF-8 bytes. `Option` = `u8` flag + body. `Vec` = `u32` length + entries.

> The snapshot encoder enforces hard caps on identifier lengths and table shape — see the [Schema Definition warning](./schema.md) for the full list. Names exceeding 255 bytes will truncate or panic at runtime.

---

## Migration Plan

The planner takes two inputs:

- `stored: Vec<TableSchemaSnapshot>` — read from each table's snapshot page.
- `compiled: Vec<TableSchemaSnapshot>` — built from compile-time `TableSchema::schema_snapshot()`.

Tables match by exact, case-sensitive name. The diff produces three buckets:

- `compiled \ stored` → `CreateTable`.
- `stored ∩ compiled` → per-table column + index diff (see below).
- `stored \ compiled` → `DropTable`.

**Column diff (per matched table):**

For each compiled column:

1. Look up the stored column by name. Match → step 3.
2. On miss, walk the compiled column's `renamed_from` slice. The first stored column hit emits `RenameColumn`; continue at step 3 with the renamed stored column.
3. Compare `(data_type, nullable, auto_increment, unique, primary_key, foreign_key)`:
   - Types differ and the change is in the [widening whitelist](#compatible-widening-whitelist) → `WidenColumn`.
   - Types differ and `Migrate::transform_column` returns a non-trivial override → `TransformColumn`.
   - Types differ and neither applies → `MigrationError::IncompatibleType`.
   - Any constraint flag changed → `AlterColumn { changes }`.

Stored columns not matched by any compiled column (directly or via `renamed_from`) → `DropColumn`. Compiled columns not matched → `AddColumn`. If non-nullable, the planner requires either `#[default = ...]` or `Migrate::default_value` returning `Some`, otherwise `MigrationError::DefaultMissing`.

**Index diff:**

Indexes are matched by `(sorted column list, unique)` tuple. Differences emit `AddIndex` / `DropIndex`.

### MigrationOp

```rust
pub enum MigrationOp {
    CreateTable { name: String, schema: TableSchemaSnapshot },
    DropTable { name: String },                            // destructive
    AddColumn { table: String, column: ColumnSnapshot },
    DropColumn { table: String, column: String },          // destructive
    RenameColumn { table: String, old: String, new: String },
    AlterColumn {
        table: String,
        column: String,
        changes: ColumnChanges,
    },
    WidenColumn {
        table: String,
        column: String,
        old_type: DataTypeSnapshot,
        new_type: DataTypeSnapshot,
    },
    TransformColumn {
        table: String,
        column: String,
        old_type: DataTypeSnapshot,
        new_type: DataTypeSnapshot,
    },
    AddIndex { table: String, index: IndexSnapshot },
    DropIndex { table: String, index: IndexSnapshot },
}

pub struct ColumnChanges {
    pub nullable: Option<bool>,
    pub unique: Option<bool>,
    pub auto_increment: Option<bool>,
    pub primary_key: Option<bool>,
    pub foreign_key: Option<Option<ForeignKeySnapshot>>, // Some(None) = drop FK
}
```

### Apply Order

Ops are sorted into a deterministic order so an `AddColumn` referencing a new FK target finds its target table already created, and so tightenings run only after data is in place:

1. `CreateTable` — new FK targets must exist first.
2. `DropIndex`.
3. `DropColumn`.
4. `RenameColumn`.
5. `AlterColumn` — **relaxations only** (`nullable: true`, `unique: false`, drop FK).
6. `WidenColumn`.
7. `TransformColumn`.
8. `AddColumn`.
9. `AlterColumn` — **tightenings** (`nullable: false`, `unique: true`, add FK). The planner validates existing data; offending rows trigger `MigrationError::ConstraintViolation`.
10. `AddIndex`.
11. `DropTable`.

All ops execute inside a single `JournaledWriter` session. Any failure rolls back every page touched; stored snapshots, `schema_hash`, and the `drift` flag are **not** mutated on failure.

**Commit step (on success):**

1. Write each updated `TableSchemaSnapshot` to its `schema_snapshot_page`.
2. Recompute `schema_hash` and write to `SchemaRegistry` on Page 0.
3. Clear the in-memory `drift` flag.

All three writes live in the same journal session as the data rewrites, so partial migrations are impossible.

**Pre-flight validation:** before opening the journal session, the planner runs `plan_migration()`, checks `MigrationPolicy`, and verifies each op is applicable (`AddColumn` has a default or is nullable, type changes are widenings or have a transform, etc.). Errors in this phase do not touch memory.

---

## Compatible Widening Whitelist

Auto-applied without user code. The framework rewrites records in place.

| From → To                | Semantics               |
| ------------------------ | ----------------------- |
| `IntN` → `IntM`, M > N   | sign-extend             |
| `UintN` → `UintM`, M > N | zero-extend             |
| `UintN` → `IntM`, M > N  | zero-extend into signed |
| `Float32` → `Float64`    | widen                   |

Everything else (narrowing, sign flips, int↔float, int↔text, etc.) falls through to `TransformColumn` or errors with `MigrationError::IncompatibleType`.

---

## Per-Table Hooks

Three macro features feed the planner. They produce no runtime cost on CRUD.

### `#[default]` Attribute

Static per-column default for `AddColumn` ops on non-nullable columns.

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,

    #[default = 0]
    pub login_count: Uint32,
}
```

The expression must convert into the column's `Value` variant via `From`/`Into`. See the [Default Value section in the schema reference](./schema.md#default-value) for the full rules.

### `#[renamed_from]` Attribute

Lists previous names for a column so the planner can emit `RenameColumn` instead of `DropColumn` + `AddColumn`:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[renamed_from("username", "user_name")]
    pub name: Text,
}
```

Multiple entries support recovery from skipped releases. See the [Renamed From section in the schema reference](./schema.md#renamed-from).

### `Migrate` Trait

`#[derive(Table)]` emits an empty `impl Migrate for T {}` for every table by default. Override it by adding `#[migrate]` at the struct level and writing the impl yourself:

```rust
pub trait Migrate
where
    Self: TableSchema,
{
    /// Dynamic default for AddColumn on a non-nullable column.
    /// `None` falls back to the static `#[default]` attribute, else
    /// DefaultMissing.
    fn default_value(_column: &str) -> Option<Value> { None }

    /// Transform a stored value during an incompatible type change.
    /// `Ok(None)` → no transform (errors unless widening applies).
    /// `Ok(Some(v))` → use `v`.
    /// `Err(_)` → abort migration; journal rolls back.
    fn transform_column(
        _column: &str,
        _old: Value,
    ) -> DbmsResult<Option<Value>> {
        Ok(None)
    }
}
```

See the [Migrate Override section in the schema reference](./schema.md#migrate-override) for usage examples.

---

## Migration Policy

```rust
pub struct MigrationPolicy {
    pub allow_destructive: bool,   // DropTable, DropColumn
}

impl Default for MigrationPolicy {
    fn default() -> Self {
        Self { allow_destructive: false }
    }
}
```

The default policy refuses destructive ops. Pre-flight planning emits `MigrationError::DestructiveOpDenied { op }` if any `DropTable` or `DropColumn` op is present and `allow_destructive` is `false`.

```rust
// Allow drops
dbms.migrate(MigrationPolicy { allow_destructive: true })?;
```

---

## Errors

`DbmsError::Migration(MigrationError)` covers the full migration pipeline:

| Variant                  | When                                                                                              |
| ------------------------ | ------------------------------------------------------------------------------------------------- |
| `SchemaDrift`            | CRUD called while `drift == true`. Call `migrate(policy)` first.                                  |
| `IncompatibleType`       | Type change is neither in the widening whitelist nor handled by `transform_column`.               |
| `DefaultMissing`         | `AddColumn` on a non-nullable column without `#[default]` or `default_value` override.            |
| `ConstraintViolation`    | Tightening op found data that violates the new constraint.                                        |
| `DestructiveOpDenied`    | Planner emitted `DropTable` / `DropColumn` while `allow_destructive` is `false`.                  |
| `TransformAborted`       | User `transform_column` impl returned `Err`.                                                      |
| `WideningIncompatible`   | `WidenColumn` op falls outside the widening whitelist (and no `transform_column` impl handled it). |
| `TransformReturnedNone`  | `Migrate::transform_column` returned `Ok(None)` while a transform was required.                   |
| `ForeignKeyViolation`    | Add-FK tightening found a row whose value is absent from the target table's column.               |

See the [Migration Errors section in the errors reference](./errors.md#migration-errors) for matching examples and remediation.

---

## API Surface

### Generic (`wasm-dbms`)

```rust
impl<M, A, S> Dbms<M, A, S>
where
    M: MemoryProvider,
    A: AccessControl,
    S: DatabaseSchema<M, A>,
{
    /// O(1). True iff compiled schema differs from stored.
    pub fn has_drift(&self) -> bool;

    /// Compute the diff without applying. Safe to call during drift.
    pub fn plan_migration(&self) -> DbmsResult<Vec<MigrationOp>>;

    /// Apply the diff. Transactional. Errors leave the database unchanged.
    pub fn migrate(&mut self, policy: MigrationPolicy) -> DbmsResult<()>;
}
```

### `DatabaseSchema` Dispatch

`#[derive(DatabaseSchema)]` emits three migration dispatch methods alongside the CRUD dispatch methods:

```rust
pub trait DatabaseSchema<M, A>
where
    M: MemoryProvider,
    A: AccessControl,
{
    // ... existing CRUD dispatch ...

    fn migrate_default(table: &str, column: &str) -> Option<Value>
    where
        Self: Sized;

    fn migrate_transform(
        table: &str,
        column: &str,
        old: Value,
    ) -> DbmsResult<Option<Value>>
    where
        Self: Sized;

    fn compiled_snapshots() -> Vec<TableSchemaSnapshot>
    where
        Self: Sized;
}
```

The macro generates match arms keyed by table name. `migrate_default` chains `Migrate::default_value` → `ColumnDef::default`; `migrate_transform` dispatches to `Migrate::transform_column`; `compiled_snapshots` calls `T::schema_snapshot()` for every table in the `#[tables(...)]` list.

### IC Endpoints

`#[derive(DbmsCanister)]` emits three additional admin-gated endpoints:

```candid
service : (IcDbmsCanisterArgs) -> {
  // ...
  has_schema_drift : () -> (bool) query;
  plan_migration  : () -> (Result_Vec_MigrationOp);
  migrate         : (MigrationPolicy) -> (Result);
}
```

All three honour the existing ACL check. `MigrationOp`, `MigrationPolicy`, `TableSchemaSnapshot`, `ColumnSnapshot`, `IndexSnapshot`, `ForeignKeySnapshot`, `DataTypeSnapshot`, and `ColumnChanges` derive `CandidType + Deserialize` behind the `candid` feature in `wasm-dbms-api`, so they appear in the generated `.did` automatically.

---

## Non-Goals

The following are intentionally out of scope:

- **Table rename.** Detect via `renamed_from` on columns; full table rename requires manual migration.
- **Custom data type binary evolution.** User-defined types are keyed by name; binary layout stability remains the user's responsibility.
- **Downgrade / rollback to an older schema.** Migrations are forward-only. Failed migrations roll back to the pre-migration state, but there is no path from a newer snapshot to an older compiled schema.
- **Automatic migration on DB init.** Migration is explicit, triggered by the operator.

---

## Worked Example

```rust
use wasm_dbms_api::prelude::*;

// Release v1
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct UserV1 {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

// Release v2: rename `name` → `full_name`, add a non-nullable
// `login_count` column with a default of 0, and keep an index on
// `full_name`.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct UserV2 {
    #[primary_key]
    pub id: Uint32,

    #[renamed_from("name")]
    #[index]
    pub full_name: Text,

    #[default = 0]
    pub login_count: Uint32,
}
```

After upgrading the canister, `dbms.has_drift()` returns `true`. Calling `dbms.migrate(MigrationPolicy::default())` produces the following ops (in apply order):

1. `RenameColumn { table: "users", old: "name", new: "full_name" }`
2. `AddColumn { table: "users", column: ColumnSnapshot { name: "login_count", default: Some(Value::Uint32(Uint32(0))), ... } }`
3. `AddIndex { table: "users", index: IndexSnapshot { columns: vec!["full_name".into()], unique: false } }`

The session commits atomically; existing rows now carry `login_count = 0` and the rename preserves their stored values.

---

## Best Practices

**1. Land schema changes one release at a time.**

Combining a rename, a tightening, and a non-nullable add in one release multiplies the chance of `ConstraintViolation` mid-apply. Stage each kind in its own release where feasible.

**2. Tighten only after backfilling.**

Plan a `nullable: false` flip in two steps: first add the column nullable + backfill, then tighten in the next release. This isolates `MigrationError::ConstraintViolation` to a release where the cause is obvious.

**3. Always start with `allow_destructive: false`.**

Run `plan_migration()` and inspect the ops before flipping the policy. A surprise `DropTable` because of a typo in `#[table = "..."]` is much cheaper to catch in pre-flight than after the journal commits.

**4. Test drift with the real binary format.**

Hand-rolled snapshots in tests are risky because the encoder is the source of truth for the wire format. Roundtrip via `Encode::encode` / `Encode::decode` and assert equality.

**5. Treat `DataTypeSnapshot` discriminants as frozen.**

Adding a new variant takes a fresh tag. Renaming or reordering existing tags breaks every snapshot in production.

**6. Persist migration logs externally.**

The DBMS does not retain a history of applied migrations beyond the new `schema_hash`. If you need an audit trail, log `plan_migration()` output before calling `migrate()`.

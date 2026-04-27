# Schema Migrations

- [Schema Migrations](#schema-migrations)
  - [Overview](#overview)
  - [When You Need a Migration](#when-you-need-a-migration)
  - [The Workflow](#the-workflow)
  - [Adding a Column](#adding-a-column)
    - [Nullable Columns](#nullable-columns)
    - [Non-Nullable Columns with a Static Default](#non-nullable-columns-with-a-static-default)
    - [Non-Nullable Columns with a Dynamic Default](#non-nullable-columns-with-a-dynamic-default)
  - [Renaming a Column](#renaming-a-column)
  - [Changing a Column Type](#changing-a-column-type)
    - [Compatible Widening](#compatible-widening)
    - [Custom Transform](#custom-transform)
  - [Dropping a Column or Table](#dropping-a-column-or-table)
  - [Tightening Constraints](#tightening-constraints)
  - [Adding and Dropping Indexes](#adding-and-dropping-indexes)
  - [Running Migrations](#running-migrations)
    - [Generic Backend](#generic-backend)
    - [IC Canister](#ic-canister)
  - [Inspecting Drift Without Migrating](#inspecting-drift-without-migrating)
  - [Recovering from a Failed Migration](#recovering-from-a-failed-migration)
  - [Testing Migrations](#testing-migrations)
  - [Common Pitfalls](#common-pitfalls)

> For the full type and API reference (snapshot format, op enum, error variants, IC endpoints), see the [Migrations Reference](../reference/migrations.md).

---

## Overview

A migration in wasm-dbms is the process of bringing the on-disk data layout into agreement with the schema your binary was compiled against. The framework persists a `TableSchemaSnapshot` for every table on disk and hashes them into a single `schema_hash`. On boot, the DBMS recomputes the hash from the compiled schema and compares it. If they differ, the database enters **drift** state and refuses CRUD until you call `migrate(policy)`.

Migrations are:

- **Forward-only.** Failed migrations roll back to the pre-migration state, but the framework provides no path from a newer snapshot to an older compiled schema.
- **Explicit.** The DBMS never auto-migrates on init. The operator decides when (and whether) to run them.
- **Atomic.** Every op runs inside a single journaled session — either every byte change commits, or none does.
- **Pre-flighted.** Each plan is validated against the current data before any page is touched. Errors here cost nothing.

---

## When You Need a Migration

Drift fires whenever the encoded snapshot of any compiled table differs from the snapshot stored on disk. In practice, that means **any** of:

- Adding, removing, or renaming a struct that derives `Table`.
- Adding, removing, or renaming a field on such a struct.
- Changing a field's type (e.g. `Uint32` → `Uint64`, or `Text` → custom enum).
- Toggling `#[primary_key]`, `#[unique]`, `#[autoincrement]`, `Nullable<T>`, or `#[foreign_key(...)]`.
- Adding or removing an `#[index]` (single-column or grouped).
- Bumping `#[alignment = N]`.

**Never trigger drift:**

- Adding `#[validate(...)]`, `#[sanitizer(...)]`, or `#[default = ...]` on its own (sanitizer/validator are runtime-only; `#[default]` is migration metadata that lives in the snapshot but is consulted by the planner, not by the drift hash for unrelated changes).
- Reordering doc comments or `Debug` derives.
- Changing the table's Rust struct name without changing `#[table = "..."]`.

---

## The Workflow

For most schema changes, the loop is:

1. **Edit the schema** in your `#[derive(Table)]` structs.
2. **Build and deploy** the new binary. On the IC, this is a canister upgrade.
3. **Inspect drift.** Call `dbms.has_drift()` (or the `has_schema_drift` Candid query). Skip if `false`.
4. **Plan.** Call `dbms.plan_migration()` and review the `Vec<MigrationOp>`.
5. **Apply.** Call `dbms.migrate(policy)` once the plan looks right.

The remaining sections walk through the common shapes of step 1 and the policy choices for step 5.

---

## Adding a Column

### Nullable Columns

Easiest case. The new column is implicitly `NULL` for every existing row.

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,

    pub bio: Nullable<Text>,   // NEW — no further work needed
}
```

Plan output:

```text
AddColumn { table: "users", column: ColumnSnapshot { name: "bio", nullable: true, default: None, ... } }
```

`migrate(MigrationPolicy::default())` applies it cleanly.

### Non-Nullable Columns with a Static Default

If the new column is `NOT NULL`, the planner needs a default value to backfill existing rows. The cheapest way is the `#[default = ...]` attribute:

```rust
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,

    #[default = 0]
    pub login_count: Uint32,
}
```

The expression must convert into the column's `Value` variant via `From`/`Into`. Examples:

```rust
#[default = 0]                                pub login_count: Uint32,
#[default = false]                            pub is_admin: Boolean,
#[default = ""]                               pub locale: Text,
#[default = MyCustomEnum::Default]            pub status: MyCustomEnum,  // requires #[custom_type]
```

### Non-Nullable Columns with a Dynamic Default

Sometimes the default depends on runtime context (e.g. derived from another column, or generated by a hash). Mark the table `#[migrate]` and override `Migrate::default_value`:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "events"]
#[migrate]
pub struct Event {
    #[primary_key]
    pub id: Uint32,
    pub kind: Text,

    pub severity: Uint8,   // NEW
}

impl Migrate for Event {
    fn default_value(column: &str) -> Option<Value> {
        match column {
            "severity" => Some(Value::Uint8(Uint8(1))),  // medium severity by default
            _ => None,
        }
    }
}
```

Returning `None` here falls back to the `#[default]` attribute. Returning `None` from both produces `MigrationError::DefaultMissing`.

> **Note:** without `#[migrate]`, the `Table` macro emits an empty `impl Migrate for T {}` for you. Adding a hand-written impl on top of it would be a duplicate.

---

## Renaming a Column

A naive rename — change the field name and ship — looks to the planner like a `DropColumn` followed by an `AddColumn`. That destroys the data. Use `#[renamed_from(...)]` to tell the planner the rename history:

```rust
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[renamed_from("name", "username")]
    pub full_name: Text,
}
```

The planner walks the slice in order: it first looks for a stored column named `name`; if that misses, it tries `username`. The first hit emits `RenameColumn { old, new: "full_name" }` and the column's data carries over intact.

**Multiple renames across releases:** keep older entries at the tail. If you renamed `username` → `name` in v2 and `name` → `full_name` in v3, list `["name", "username"]` so a v1-installed canister upgrading directly to v3 still finds its column.

---

## Changing a Column Type

### Compatible Widening

The framework auto-widens these without user code:

| From → To                | Semantics               |
| ------------------------ | ----------------------- |
| `IntN` → `IntM`, M > N   | sign-extend             |
| `UintN` → `UintM`, M > N | zero-extend             |
| `UintN` → `IntM`, M > N  | zero-extend into signed |
| `Float32` → `Float64`    | widen                   |

Just edit the field type and migrate. Plan output is `WidenColumn { ... }`.

### Custom Transform

Anything else — narrowing, sign flip, int↔float, int↔text, custom enum reshape — needs a `transform_column` impl:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "events"]
#[migrate]
pub struct Event {
    #[primary_key]
    pub id: Uint32,

    pub severity: Uint8,   // was: Text("low" | "medium" | "high")
}

impl Migrate for Event {
    fn default_value(_column: &str) -> Option<Value> { None }

    fn transform_column(column: &str, old: Value) -> DbmsResult<Option<Value>> {
        match column {
            "severity" => match old {
                Value::Text(Text(s)) => match s.as_str() {
                    "low" => Ok(Some(Value::Uint8(Uint8(1)))),
                    "medium" => Ok(Some(Value::Uint8(Uint8(5)))),
                    "high" => Ok(Some(Value::Uint8(Uint8(9)))),
                    other => Err(DbmsError::Migration(MigrationError::TransformAborted {
                        table: "events".into(),
                        column: column.into(),
                        reason: format!("unknown severity `{other}`"),
                    })),
                },
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }
}
```

Return values:

- `Ok(Some(v))` → store `v`. The planner emits `TransformColumn { old_type: Text, new_type: Uint8 }`.
- `Ok(None)` → no transform. The framework errors with `MigrationError::IncompatibleType` unless a widening already applies.
- `Err(_)` → abort the migration. The journal rolls back.

---

## Dropping a Column or Table

`DropColumn` and `DropTable` are **destructive**. The default `MigrationPolicy::default()` refuses them:

```rust
let plan = dbms.plan_migration()?;   // shows DropTable / DropColumn ops
let result = dbms.migrate(MigrationPolicy::default());
// → Err(DbmsError::Migration(MigrationError::DestructiveOpDenied { op: "DropColumn" }))
```

Opt in explicitly:

```rust
dbms.migrate(MigrationPolicy { allow_destructive: true })?;
```

> **Tip:** keep `allow_destructive: false` in the standard upgrade path and set it to `true` only when the operator has manually inspected `plan_migration()` output. A typo in `#[table = "..."]` looks identical to a deliberate drop in the diff.

---

## Tightening Constraints

A *tightening* is any `AlterColumn` change in the restrictive direction:

- `nullable: true` → `nullable: false`
- `unique: false` → `unique: true`
- adding a `#[foreign_key(...)]`

Tightenings run **after** all data rewrites (relaxations, widenings, transforms, adds). The planner validates existing rows against the new constraint at this step. Any violation produces `MigrationError::ConstraintViolation { table, column, reason }` and rolls back the entire session.

**Recommended pattern (split across two releases):**

1. **Release N** — relax + backfill:

   ```rust
   pub email: Nullable<Text>,   // still nullable
   ```

   Backfill `NULL` rows manually or via a one-off update before shipping the next release.

2. **Release N+1** — tighten:

   ```rust
   #[unique]
   pub email: Text,             // now NOT NULL + unique
   ```

This isolates `ConstraintViolation` to a release whose cause is obvious.

---

## Adding and Dropping Indexes

Add an `#[index]` and the planner emits `AddIndex`. Remove it and you get `DropIndex`. Composite indexes match by `(sorted column list, unique)`, so changing the group name on a composite index is equivalent to dropping the old one and adding a new one with the same shape.

```rust
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[index]                                  // NEW
    #[unique]
    pub email: Text,
}
```

> Index migrations rebuild the B+ tree from scratch, so they scale `O(n log n)` with row count.

---

## Running Migrations

### Generic Backend

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::MigrationPolicy;

fn boot(mut dbms: Dbms<...>) -> DbmsResult<()> {
    if dbms.has_drift() {
        let plan = dbms.plan_migration()?;
        eprintln!("schema drift detected, applying {} ops", plan.len());
        for op in &plan {
            eprintln!("  {op:?}");
        }
        dbms.migrate(MigrationPolicy::default())?;
    }
    Ok(())
}
```

`migrate` is idempotent: when there is no drift, it is a no-op.

### IC Canister

The `#[derive(DbmsCanister)]` macro emits three admin-gated endpoints:

```candid
has_schema_drift : () -> (bool) query;
plan_migration  : () -> (Result_Vec_MigrationOp);
migrate         : (MigrationPolicy) -> (Result);
```

Wire them into your `post_upgrade` hook so that an upgrade automatically heals drift, gated on operator confirmation:

```rust
#[ic_cdk::post_upgrade]
fn post_upgrade() {
    DBMS_CONTEXT.with(|ctx| {
        // Inspect drift and decide whether to auto-migrate. For
        // safety the framework refuses destructive ops by default.
        let mut db = WasmDbmsDatabase::oneshot(ctx, MyDbmsCanister);
        if db.has_drift() {
            db.migrate(MigrationPolicy::default())
                .expect("migration failed");
        }
    });
}
```

Or, for stricter control, leave the canister in drift state after upgrade and run `migrate` from a tooling script after operator review.

---

## Inspecting Drift Without Migrating

`plan_migration()` is safe to call regardless of drift state and never touches stable memory. Use it to:

- Diff a development branch against production data.
- Generate a changelog entry from `MigrationOp` Debug output.
- Catch unintended drops in CI before the binary ships.

```rust
let plan = dbms.plan_migration()?;
for op in plan {
    println!("{op:?}");
}
```

---

## Recovering from a Failed Migration

A failed `migrate()` call rolls back every page touched in the journal session. Stored snapshots, `schema_hash`, and the in-memory `drift` flag are **not** mutated on failure. So after an error:

- The DBMS stays in drift state.
- Stored data is byte-identical to its pre-migration state.
- ACL methods still work.

Recovery is iterative:

1. Read the error variant. `IncompatibleType`, `DefaultMissing`, `ConstraintViolation`, `DestructiveOpDenied`, and `TransformAborted` each call out the offending table/column/reason.
2. Fix the cause: add `#[default]`, write a `transform_column` arm, clean offending rows via ACL-allowed admin endpoints, or relax the policy.
3. Redeploy the binary (or just retry `migrate` if the fix is data-side, not schema-side).

There is no partial-success state to clean up. Either the plan applied in full or it didn't apply at all.

---

## Testing Migrations

The migration pipeline is testable end-to-end on the heap memory provider:

1. Register the **old** schema with a fresh `DbmsContext`.
2. Insert representative fixtures.
3. Drop the context and reopen it with the **new** schema (no rebuild, since this is just Rust code).
4. Assert `has_drift() == true`, inspect `plan_migration()`, call `migrate(policy)`.
5. Read the rows back and assert the expected post-migration state.

```rust
#[test]
fn renames_preserve_data() {
    // v1 schema: column "name"
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    SchemaV1::register_tables(&ctx).unwrap();
    let mut db = WasmDbmsDatabase::oneshot(&ctx, SchemaV1);
    db.insert::<UserV1>(/* ... */).unwrap();
    drop(db);

    // v2 schema: column renamed to "full_name"
    let mut db = WasmDbmsDatabase::oneshot(&ctx, SchemaV2);
    assert!(db.has_drift());
    db.migrate(MigrationPolicy::default()).unwrap();

    let users: Vec<UserV2Record> = db.select::<UserV2>(Query::builder().build()).unwrap();
    assert_eq!(users[0].full_name, Some(/* ... */));
}
```

Round-trip the snapshots through `Encode::encode` / `Encode::decode` to confirm the wire format hasn't shifted.

---

## Common Pitfalls

- **Renaming without `#[renamed_from]`.** The planner has no way to know your intent; it will emit `DropColumn` + `AddColumn` and silently lose data the moment `allow_destructive: true` is set.
- **Adding a non-nullable column without a default.** Pre-flight will reject the plan with `DefaultMissing`. Either provide `#[default]`, override `Migrate::default_value`, or make the column `Nullable<T>`.
- **Tightening on dirty data.** A `nullable: false` flip after a release that allowed nulls will fail unless every row already satisfies the constraint. Backfill in a prior release.
- **Reordering `DataTypeSnapshot` discriminants.** The on-disk format depends on the exact tag bytes. Treat the enum as frozen — new variants take fresh tags, removed ones leave a reserved hole.
- **Bumping `#[alignment = N]`.** This changes the on-disk record layout for the table. Until `WidenColumn` is generalised to handle alignment changes, this requires a manual rewrite. Avoid unless absolutely necessary.
- **Calling `migrate` before `register_tables`.** The drift hash is computed from the registered set. Always register every table that backs a `#[derive(Table)]` struct in the compiled binary, even if you don't expect to write to it this release.

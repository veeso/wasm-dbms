# Schema Migrations (IC)

> **Note:** This is the IC-specific migrations guide. The schema-design rules
> (`#[default]`, `#[renamed_from]`, `#[migrate]`, the `Migrate` trait,
> `MigrationOp` semantics) are identical to the generic backend; see the
> [generic Schema Migrations Guide](../../guides/migrations.md) and the
> [Migrations Reference](../../reference/migrations.md) for the conceptual
> material. This page covers only what changes when the database lives inside
> an IC canister.

- [Schema Migrations (IC)](#schema-migrations-ic)
  - [Overview](#overview)
  - [Generated Endpoints](#generated-endpoints)
  - [Candid Types](#candid-types)
  - [Upgrade Workflow](#upgrade-workflow)
  - [Calling From a Client](#calling-from-a-client)
    - [Inter-Canister](#inter-canister)
    - [External Agent](#external-agent)
    - [PocketIC Tests](#pocketic-tests)
  - [Driving Migration From `post_upgrade`](#driving-migration-from-post_upgrade)
  - [Operator-Driven Migration](#operator-driven-migration)
  - [Error Handling](#error-handling)
  - [Drift While Serving Traffic](#drift-while-serving-traffic)

---

## Overview

A canister upgrade replaces the WASM but keeps stable memory. If the new
binary's `#[derive(Table)]` schemas differ from the snapshots persisted on
disk, the DBMS enters drift state and refuses CRUD until you call `migrate`.
ACL endpoints stay available so you can rotate principals without first
healing the schema.

The drift hash is recomputed lazily, on the first `has_drift` /
`pending_migrations` / CRUD call after boot, and cached on the DBMS context.
There is no post-upgrade hook: the canister simply boots, declares drift on
first access, and waits for the operator (or a `post_upgrade` snippet you
write yourself) to call `migrate`.

---

## Generated Endpoints

`#[derive(DbmsCanister)]` emits three additional endpoints alongside the
per-table CRUD methods:

| Endpoint             | Kind   | Purpose                                                   |
| -------------------- | ------ | --------------------------------------------------------- |
| `has_drift`          | query  | `O(1)` once cached; `true` iff a migration is needed.     |
| `pending_migrations` | query  | Returns the planned `Vec<MigrationOp>` without applying.  |
| `migrate`            | update | Plans, validates, sorts, and applies the diff atomically. |

All three are **admin-gated** through the same ACL check used by the rest of
the CRUD surface — anonymous and unlisted principals are rejected before the
DBMS is touched.

`migrate` is an `update` because it journals writes. `has_drift` and
`pending_migrations` are `query` calls and consume no cycles for the caller
beyond the standard query overhead.

---

## Candid Types

The Candid signatures are:

```candid
type MigrationPolicy = record { allow_destructive : bool };

type MigrationOp = variant {
  CreateTable   : record { name : text; schema : TableSchemaSnapshot };
  DropTable     : record { name : text };
  AddColumn     : record { table : text; column : ColumnSnapshot };
  DropColumn    : record { table : text; column : text };
  RenameColumn  : record { table : text; old : text; new : text };
  AlterColumn   : record { table : text; column : text; changes : ColumnChanges };
  WidenColumn   : record { table : text; column : text; old_type : DataTypeSnapshot; new_type : DataTypeSnapshot };
  TransformColumn : record { table : text; column : text; old_type : DataTypeSnapshot; new_type : DataTypeSnapshot };
  AddIndex      : record { table : text; index : IndexSnapshot };
  DropIndex     : record { table : text; index : IndexSnapshot };
};

has_drift           : () -> (variant { Ok : bool;             Err : IcDbmsError }) query;
pending_migrations  : () -> (variant { Ok : vec MigrationOp;  Err : IcDbmsError }) query;
migrate             : (MigrationPolicy)
                    -> (variant { Ok;                         Err : IcDbmsError });
```

Snapshot types (`TableSchemaSnapshot`, `ColumnSnapshot`, `IndexSnapshot`,
`ForeignKeySnapshot`, `DataTypeSnapshot`, `OnDeleteSnapshot`,
`ColumnChanges`) are the same Candid records the snapshot reference describes
in the [generic schema reference](../../reference/schema.md). They are
exported automatically by `ic_cdk::export_candid!()`.

---

## Upgrade Workflow

The end-to-end flow for a schema-changing release:

1. **Edit the schema.** Modify the `#[derive(Table)]` structs and add
   `#[default]` / `#[renamed_from]` / `#[migrate]` as needed.
2. **Build the canister.** `just build_all` compiles to `wasm32-unknown-unknown`,
   shrinks the WASM, and extracts the new `.did`.
3. **Deploy via `dfx canister install --mode upgrade`.** Stable memory carries
   over untouched.
4. **Inspect drift.** Call `has_drift` from `dfx`, an admin tool, or a
   `Client`. Skip the rest if `false`.
5. **Plan.** Call `pending_migrations` and review the returned ops. Look in
   particular for unintended `DropTable` / `DropColumn` ops, which usually
   signal a typo in `#[table = "..."]` or a missing `#[renamed_from]`.
6. **Apply.** Call `migrate(record { allow_destructive = false })`. If the
   plan contains a deliberate destructive op, set `allow_destructive = true`
   only after the review step.
7. **Verify.** Re-run `has_drift`; expect `false`. CRUD endpoints now work
   again.

`migrate` is idempotent: when there is no drift, the call is a cheap no-op.

---

## Calling From a Client

The three methods are part of the `Client` trait. The signatures are
identical across `IcDbmsCanisterClient`, `IcDbmsAgentClient`, and
`IcDbmsPocketIcClient`:

```rust
async fn has_drift(&self) -> Result<IcDbmsResult<bool>>;
async fn pending_migrations(&self) -> Result<IcDbmsResult<Vec<MigrationOp>>>;
async fn migrate(&self, policy: MigrationPolicy) -> Result<IcDbmsResult<()>>;
```

The outer `Result` wraps transport / canister-call failures; the inner
`IcDbmsResult` wraps `IcDbmsError` (including
`IcDbmsError::Migration(MigrationError::...)`).

### Inter-Canister

```rust
use ic_dbms_api::prelude::{MigrationPolicy};
use ic_dbms_client::{Client as _, IcDbmsCanisterClient};
use candid::Principal;

#[ic_cdk::update]
async fn heal_schema(canister: Principal) -> Result<u64, String> {
    let client = IcDbmsCanisterClient::new(canister);

    if !client.has_drift().await.map_err(|e| e.to_string())??.then_some(()).is_some() {
        return Ok(0);
    }

    let ops = client.pending_migrations().await.map_err(|e| e.to_string())??;
    client
        .migrate(MigrationPolicy::default())
        .await
        .map_err(|e| e.to_string())??;

    Ok(ops.len() as u64)
}
```

### External Agent

```rust
use ic_agent::Agent;
use ic_dbms_api::prelude::MigrationPolicy;
use ic_dbms_client::{Client as _, IcDbmsAgentClient};
use candid::Principal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .with_identity(load_identity()?)
        .build()?;
    let canister = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai")?;
    let client = IcDbmsAgentClient::new(canister, &agent);

    if client.has_drift().await?? {
        let plan = client.pending_migrations().await??;
        eprintln!("planning {} ops", plan.len());
        for op in &plan {
            eprintln!("  {op:?}");
        }
        client
            .migrate(MigrationPolicy { allow_destructive: false })
            .await??;
    }

    Ok(())
}
```

### PocketIC Tests

```rust
use ic_dbms_api::prelude::MigrationPolicy;
use ic_dbms_client::{Client as _, IcDbmsPocketIcClient};

#[tokio::test]
async fn upgrade_heals_drift() {
    let pic = pocket_ic::PocketIc::new();
    let canister = install_v1_canister(&pic);
    insert_fixtures(&pic, canister).await;

    upgrade_to_v2(&pic, canister);

    let client = IcDbmsPocketIcClient::new(canister, admin_principal(), &pic);
    assert!(client.has_drift().await.unwrap().unwrap());
    let plan = client.pending_migrations().await.unwrap().unwrap();
    assert!(!plan.is_empty());
    client
        .migrate(MigrationPolicy::default())
        .await
        .unwrap()
        .unwrap();
    assert!(!client.has_drift().await.unwrap().unwrap());
}
```

---

## Driving Migration From `post_upgrade`

For canisters where the deployment pipeline already owns the upgrade flow,
you can wire `migrate` directly into a `#[ic_cdk::post_upgrade]` hook so the
schema heals before the first CRUD call lands.

```rust
use ic_dbms_api::prelude::{MigrationPolicy};
use ic_dbms_canister::prelude::{DatabaseSchema as _, DBMS_CONTEXT, WasmDbmsDatabase};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct MyCanister;

#[ic_cdk::post_upgrade]
fn post_upgrade() {
    DBMS_CONTEXT.with(|ctx| {
        // Re-register tables so the registry matches the compiled schema
        // before drift detection runs.
        MyCanister::register_tables(ctx).expect("failed to register tables");

        let mut db = WasmDbmsDatabase::oneshot(ctx, MyCanister);
        if db.has_drift().expect("drift check failed") {
            db.migrate(MigrationPolicy::default())
                .expect("migration failed");
        }
    });
}
```

This pattern is convenient but **trades safety for convenience**:

- An accidental schema change ships destructive ops to production with no
  human review.
- A bug in `transform_column` traps the canister on upgrade.
- `MigrationPolicy::default()` (i.e. `allow_destructive: false`) refuses
  destructive ops, but everything else applies silently.

For high-stakes deployments prefer the operator-driven flow below.

---

## Operator-Driven Migration

The recommended flow for production canisters:

1. Upgrade the canister. The new WASM boots in drift state.
2. Run a one-shot script (CLI / admin canister / `dfx`) that calls
   `pending_migrations`, prints the plan, and waits for confirmation.
3. On confirmation, call `migrate`.

`dfx` example:

```bash
dfx canister call my_dbms has_drift
# (variant { Ok = true })

dfx canister call my_dbms pending_migrations
# (variant { Ok = vec { ... } })

dfx canister call my_dbms migrate '(record { allow_destructive = false })'
# (variant { Ok })
```

Until `migrate` succeeds the canister rejects every CRUD endpoint with
`MigrationError::SchemaDrift`, so any traffic that arrives between the
upgrade and the operator action receives a clear, structured error.

---

## Error Handling

Migration errors propagate through `IcDbmsError::Migration(MigrationError)`.
The variants worth handling explicitly on the client:

| Variant                 | Meaning                                                                          | Caller action                                                                   |
| ----------------------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `SchemaDrift`           | CRUD called while drift is set.                                                  | Call `migrate`.                                                                 |
| `IncompatibleType`      | Column type changed without a widening or transform.                             | Add a `transform_column` arm or a release that widens via an intermediate type. |
| `DefaultMissing`        | New non-nullable column with no `#[default]` or `default_value`.                 | Add the default; redeploy.                                                      |
| `ConstraintViolation`   | Tightening rejected an existing row.                                             | Backfill the offending rows in a prior release.                                 |
| `DestructiveOpDenied`   | Plan contained `DropTable` / `DropColumn` and policy disallowed it.              | Re-run with `allow_destructive: true` after operator review.                    |
| `TransformAborted`      | User `transform_column` returned `Err`.                                          | Fix the transform; redeploy.                                                    |
| `WideningIncompatible`  | `WidenColumn` outside the widening whitelist with no `transform_column` handler. | Provide a `transform_column` impl or split the change across multiple releases. |
| `TransformReturnedNone` | `Migrate::transform_column` returned `Ok(None)` for a column that needs one.     | Implement the transform branch.                                                 |
| `ForeignKeyViolation`   | Add-FK tightening found a row referencing a missing target.                      | Clean up orphan rows in a prior release.                                        |

Tip: never `unwrap` `migrate` in a `post_upgrade` hook — a panic there bricks
the canister. Trap with a descriptive message instead, or fall back to the
operator-driven flow.

---

## Drift While Serving Traffic

CRUD endpoints fail fast when drift is set: the very first line of every
`select_*` / `insert_*` / `update_*` / `delete_*` / `aggregate_*` handler
checks the cached drift flag and returns `Err(MigrationError::SchemaDrift)`
without touching the journal. Cost is a single boolean load.

ACL endpoints (`acl_add_principal`, `acl_remove_principal`,
`acl_allowed_principals`) bypass the drift check so the operator can rotate
keys without first migrating. The migration endpoints themselves are also
exempt — `pending_migrations` is safe to call regardless of state.

After a successful `migrate`, the in-memory drift flag is cleared inside the
same journal session that wrote the new snapshots, so the next CRUD call
proceeds against the new schema with no extra round trip.

# Access Control (IC)

> **Note:** This is the IC-specific access control guide. Access control is an IC-only feature.

ic-dbms uses a granular Access Control List (ACL) keyed by `Principal`. Each
identity carries an `IdentityPerms` record:

| Field        | Type           | Meaning                                                |
|--------------|----------------|--------------------------------------------------------|
| `admin`      | `bool`         | Bypass all per-table checks. Does NOT imply other ops. |
| `manage_acl` | `bool`         | Grant/revoke perms; add/remove identities.             |
| `migrate`    | `bool`         | Run `migrate` / `pending_migrations` / `has_drift`.    |
| `all_tables` | `TablePerms`   | Per-op bits applied to every table.                    |
| `per_table`  | `Vec<(Table, TablePerms)>` | Per-table additive grants.               |

`TablePerms` is a `u8` bitfield over `READ`, `INSERT`, `UPDATE`, `DELETE`.

`admin` bypasses table checks but does not silently elevate to `manage_acl`
or `migrate` — a data admin cannot escalate to ACL/ops roles by accident.

## Initialization

```rust
use ic_dbms_api::prelude::IcDbmsCanisterInitArgs;

let args = IcDbmsCanisterInitArgs {
    allowed_principals: Some(vec![operator_principal]),
};
```

Bootstrap rules:

| `allowed_principals` | Result                                             |
|----------------------|----------------------------------------------------|
| `None`               | Deployer principal becomes a full admin.           |
| `Some(vec![])`       | Same as `None` — deployer becomes a full admin.    |
| `Some(vec![p, q])`   | Each listed principal becomes a full admin.        |

A "full admin" carries `admin = true`, `manage_acl = true`, `migrate = true`,
and `all_tables = TablePerms::all()`.

## Endpoints

### Operational flags

| Endpoint            | Required perm  | Effect                          |
|---------------------|----------------|---------------------------------|
| `grant_admin`       | `manage_acl`   | Set `admin` on target.          |
| `revoke_admin`      | `manage_acl`   | Clear `admin` on target.        |
| `grant_manage_acl`  | `manage_acl`   | Set `manage_acl` on target.     |
| `revoke_manage_acl` | `manage_acl`   | Clear `manage_acl` on target.   |
| `grant_migrate`     | `manage_acl`   | Set `migrate` on target.        |
| `revoke_migrate`    | `manage_acl`   | Clear `migrate` on target.      |

### Table perms

| Endpoint                  | Required perm  | Effect                                   |
|---------------------------|----------------|------------------------------------------|
| `grant_all_tables_perms`  | `manage_acl`   | OR `perms` into `all_tables`.            |
| `revoke_all_tables_perms` | `manage_acl`   | Mask `perms` out of `all_tables`.        |
| `grant_table_perms`       | `manage_acl`   | OR `perms` into `per_table[table]`.      |
| `revoke_table_perms`      | `manage_acl`   | Mask `perms` out of `per_table[table]`.  |

### Identity lifecycle

| Endpoint            | Required perm  | Effect                                    |
|---------------------|----------------|-------------------------------------------|
| `remove_identity`   | `manage_acl`   | Drop the identity entirely.               |
| `list_identities`   | `manage_acl`   | List every identity with its perms.       |
| `my_perms`          | (none)         | Return the caller's own perms.            |

### CRUD enforcement

`#[derive(DbmsCanister)]` injects a `granted` check before each generated
endpoint:

| Endpoint kind                          | Required perm         |
|----------------------------------------|-----------------------|
| `select_*` / `aggregate_*` / `select`  | `TablePerms::READ`    |
| `insert_*`                             | `TablePerms::INSERT`  |
| `update_*`                             | `TablePerms::UPDATE`  |
| `delete_*`                             | `TablePerms::DELETE`  |

Effective check: `admin || (all_tables | per_table[table]).contains(required)`.

`select_join` enforces READ on the **root** table only. Joined tables are
not checked separately in v1.

### Migration

| Endpoint              | Required perm |
|-----------------------|---------------|
| `has_drift`           | `migrate`     |
| `pending_migrations`  | `migrate`     |
| `migrate`             | `migrate`     |

### Transactions

`begin_transaction` / `commit` / `rollback` are unconditional — per-op CRUD
checks gate the data accesses inside the transaction. An identity with no
perms can open and commit an empty transaction; the moment it tries to
read or write, `AccessDenied` is returned.

## Last-`manage_acl` guard

`revoke(ManageAcl)` and `remove_identity` refuse the operation when it
would leave the ACL with zero `manage_acl`-carrying identities:

```text
DbmsError::Memory(MemoryError::ConstraintViolation(
    "at least one identity must retain manage_acl"
))
```

`admin` and `migrate` carry no such guard — they can be re-granted from any
`manage_acl` holder.

## Errors

A failed perm check returns:

```rust
DbmsError::AccessDenied {
    table: Option<TableFingerprint>,
    required: RequiredPerm,
}
```

`RequiredPerm` enumerates the missing perm class:

- `RequiredPerm::Table(TablePerms)` — a table operation.
- `RequiredPerm::Admin`             — admin bypass missing.
- `RequiredPerm::ManageAcl`         — ACL management missing.
- `RequiredPerm::Migrate`           — migration missing.

## Recipes

### Read-only viewer

```rust
client.grant_all_tables_perms(viewer, TablePerms::READ).await?;
```

### Per-table writer

```rust
client.grant_table_perms(svc, "users", TablePerms::INSERT | TablePerms::UPDATE).await?;
```

### Migration bot

```rust
client.grant_migrate(bot).await?;
```

### ACL deputy

```rust
client.grant_manage_acl(deputy).await?;
```


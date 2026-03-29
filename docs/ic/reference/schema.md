# Schema Reference (IC)

> **Note:** This is the IC-specific schema reference. For complete Table macro details, column attributes, generated types, and best practices, see the [generic schema reference](../../reference/schema.md).

- [Schema Reference (IC)](#schema-reference-ic)
  - [Overview](#overview)
  - [IC-Specific Required Derives](#ic-specific-required-derives)
  - [DatabaseSchema Macro](#databaseschema-macro)
  - [DbmsCanister Macro](#dbmscanister-macro)
    - [Basic Usage](#basic-usage)
    - [Generated Candid API](#generated-candid-api)
  - [Candid Integration](#candid-integration)
    - [CandidType and Deserialize](#candidtype-and-deserialize)
    - [Candid Export](#candid-export)
  - [Complete IC Example](#complete-ic-example)

---

## Overview

When deploying wasm-dbms on the Internet Computer, your schema definitions need additional IC-specific derives, the `#[candid]` attribute, and a canister generation macro. The core `Table` macro, column attributes (`#[primary_key]`, `#[index]`, `#[foreign_key(...)]`, `#[sanitizer(...)]`, `#[validate(...)]`, `#[custom_type]`, `#[alignment]`), and generated types (`Record`, `InsertRequest`, `UpdateRequest`, `ForeignFetcher`) work exactly as described in the [generic schema reference](../../reference/schema.md). This document covers only the IC-specific additions.

---

## IC-Specific Required Derives

Every table struct for IC deployment must include `CandidType` and `Deserialize` in addition to the standard `Table` and `Clone` derives. You must also add the `#[candid]` attribute so that generated types (`Record`, `InsertRequest`, `UpdateRequest`) derive `CandidType`, `Serialize`, and `Deserialize` as well:

```rust
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}
```

| Derive / Attribute | Required for IC | Purpose |
|--------------------|-----------------|---------|
| `Table` | Yes | Generates table schema and related types |
| `CandidType` | Yes (IC-specific) | Enables Candid serialization for the table struct |
| `Deserialize` | Yes (IC-specific) | Enables deserialization from Candid wire format |
| `#[candid]` | Yes (IC-specific) | Adds Candid/Serde derives to generated `Record`, `InsertRequest`, `UpdateRequest` types |
| `Clone` | Yes | Required by the macro system |
| `Debug` | Recommended | Useful for debugging |
| `PartialEq`, `Eq` | Recommended | Useful for comparisons in tests |

Without `CandidType`, `Deserialize`, and `#[candid]`, the generated canister API will not compile because Candid is the serialization format used for all IC inter-canister calls.

---

## DatabaseSchema Macro

The `DatabaseSchema` derive macro generates a `DatabaseSchema<M, A>` trait implementation that provides schema dispatch -- routing database operations to the correct table by name at runtime. This is required by the `DbmsCanister` macro.

The `DatabaseSchema` macro is provided by `wasm-dbms-macros` and re-exported through the `ic-dbms-canister` prelude.

```rust
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};
use my_schema::{User, Post};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct MyDbmsCanister;
```

The macro reads the `#[tables(...)]` attribute and generates:

- A `DatabaseSchema<M, A>` trait implementation that dispatches `select`, `insert`, `update`, `delete`, and `select_raw` calls to the correct table by name
- A `register_tables` associated method for convenient table registration during canister initialization

---

## DbmsCanister Macro

The `DbmsCanister` macro is an IC-specific procedural macro that generates a complete Internet Computer canister API from your table definitions. It is provided by the `ic-dbms-canister` crate. It requires the `DatabaseSchema` derive to also be present on the same struct.

### Basic Usage

```rust
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};
use my_schema::{User, Post, Comment};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts", Comment = "comments")]
pub struct MyDbmsCanister;

ic_cdk::export_candid!();
```

**Format:** `#[tables(StructName = "table_name", ...)]`

- `StructName` is the Rust struct name (must be in scope via `use`)
- `"table_name"` is the table name matching the `#[table = "..."]` attribute on the struct

### Generated Candid API

For each table, the macro generates four CRUD endpoints plus shared transaction and ACL endpoints:

```candid
service : (IcDbmsCanisterArgs) -> {
  // Per-table CRUD (example for "users" table)
  insert_users : (UserInsertRequest, opt nat) -> (Result);
  select_users : (Query, opt nat) -> (Result_Vec_UserRecord) query;
  update_users : (UserUpdateRequest, opt nat) -> (Result_u64);
  delete_users : (DeleteBehavior, opt Filter, opt nat) -> (Result_u64);

  // Per-table CRUD (example for "posts" table)
  insert_posts : (PostInsertRequest, opt nat) -> (Result);
  select_posts : (Query, opt nat) -> (Result_Vec_PostRecord) query;
  update_posts : (PostUpdateRequest, opt nat) -> (Result_u64);
  delete_posts : (DeleteBehavior, opt Filter, opt nat) -> (Result_u64);

  // Transaction methods (shared)
  begin_transaction : () -> (nat);
  commit : (nat) -> (Result);
  rollback : (nat) -> (Result);

  // ACL methods (shared)
  acl_add_principal : (principal) -> (Result);
  acl_remove_principal : (principal) -> (Result);
  acl_allowed_principals : () -> (vec principal) query;
}
```

**Method naming convention:** `{operation}_{table_name}` (e.g., `insert_users`, `select_posts`, `delete_comments`)

**Parameter patterns:**
- `opt nat` is the optional transaction ID
- `select` methods are `query` calls (no state changes, no cycles consumed)
- All other methods are `update` calls

**Init arguments:**

The generated canister expects `IcDbmsCanisterArgs` at initialization:

```candid
type IcDbmsCanisterArgs = variant {
  Init : IcDbmsCanisterInitArgs;
  Upgrade;
};

type IcDbmsCanisterInitArgs = record {
  allowed_principals : vec principal;
};
```

---

## Candid Integration

### CandidType and Deserialize

These derives are needed because the IC uses [Candid](https://github.com/dfinity/candid) as its interface description language. All data crossing canister boundaries must be Candid-serializable.

The `ic-dbms-api` types (via `wasm-dbms-api`) already implement `CandidType` and `Deserialize`, so your struct only needs the derives:

```rust
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

// All field types (Uint32, Text, DateTime, etc.) already implement CandidType
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "events"]
pub struct Event {
    #[primary_key]
    pub id: Uuid,
    pub name: Text,
    pub date: DateTime,
    pub metadata: Nullable<Json>,
}
```

### Candid Export

The `ic_cdk::export_candid!()` macro at the end of your canister `lib.rs` generates the `.did` file that describes your canister's interface. This is required for:

- `dfx` deployment
- Frontend integration
- Inter-canister calls with type checking
- Candid UI interaction

```rust
// canister/src/lib.rs
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};
use my_schema::{User, Post};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct MyDbmsCanister;

// This MUST be at the end of the file
ic_cdk::export_candid!();
```

---

## Complete IC Example

```rust
// schema/src/lib.rs
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,

    #[index]
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(LowerCaseSanitizer)]
    #[validate(EmailValidator)]
    pub email: Text,

    pub created_at: DateTime,
    pub is_active: Boolean,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uuid,

    #[validate(MaxStrlenValidator(200))]
    pub title: Text,

    pub content: Text,
    pub published: Boolean,

    #[index(group = "author_date")]
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,

    pub metadata: Nullable<Json>,

    #[index(group = "author_date")]
    pub created_at: DateTime,
}
```

```rust
// canister/src/lib.rs
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};
use my_schema::{User, Post};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct BlogDbmsCanister;

ic_cdk::export_candid!();
```

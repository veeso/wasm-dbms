# Get Started with IC-DBMS (IC)

> **Note:** This is the IC-specific getting started guide for deploying wasm-dbms as an Internet Computer canister. For the generic wasm-dbms getting started guide (schema definition, core concepts), see the [generic get-started guide](../../guides/get-started.md).

- [Prerequisites](#prerequisites)
- [Project Setup](#project-setup)
  - [Workspace Structure](#workspace-structure)
  - [Cargo Configuration](#cargo-configuration)
- [Define Your Schema](#define-your-schema)
  - [Create the Schema Crate](#create-the-schema-crate)
  - [Define Tables](#define-tables)
- [Create the DBMS Canister](#create-the-dbms-canister)
  - [Canister Dependencies](#canister-dependencies)
  - [Generate the Canister API](#generate-the-canister-api)
  - [Build the Canister](#build-the-canister)
- [Deploy the Canister](#deploy-the-canister)
  - [Canister Init Arguments](#canister-init-arguments)
  - [Deploy with dfx](#deploy-with-dfx)
- [Quick Example: Complete Workflow](#quick-example-complete-workflow)
- [Integration Testing](#integration-testing)
- [Next Steps](#next-steps)

This guide walks you through setting up a complete database canister on the Internet Computer using ic-dbms. The ic-dbms framework is built on top of the [wasm-dbms](https://github.com/veeso/wasm-dbms) core engine, adding IC-specific functionality such as Candid serialization, canister lifecycle management, ACL-based access control, and inter-canister communication. By the end of this guide, you will have a working canister with CRUD operations, transactions, and access control.

---

## Prerequisites

Before starting, ensure you have:

- Rust 1.85.1 or later
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`
- [dfx](https://internetcomputer.org/docs/current/developer-docs/setup/install/) (Internet Computer SDK)
- `ic-wasm`: `cargo install ic-wasm`
- `candid-extractor`: `cargo install candid-extractor`

---

## Project Setup

### Workspace Structure

We recommend organizing your project as a Cargo workspace with two crates:

```
my-dbms-project/
├── Cargo.toml          # Workspace manifest
├── schema/             # Schema definitions (reusable types)
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
└── canister/           # The DBMS canister
    ├── Cargo.toml
    └── src/
        └── lib.rs
```

**Workspace Cargo.toml:**

```toml
[workspace]
members = ["schema", "canister"]
resolver = "2"
```

### Cargo Configuration

Create `.cargo/config.toml` to configure the `getrandom` crate for WebAssembly:

```toml
[target.wasm32-unknown-unknown]
rustflags = ['--cfg', 'getrandom_backend="custom"']
```

This is required because the `uuid` crate depends on `getrandom`.

---

## Define Your Schema

### Create the Schema Crate

Create `schema/Cargo.toml`:

```toml
[package]
name = "my-schema"
version = "0.1.0"
edition = "2024"

[dependencies]
candid = "0.10"
ic-dbms-api = "0.6"
serde = "1"
```

> **Note:** `ic-dbms-api` re-exports types from `wasm-dbms-api`, so `use ic_dbms_api::prelude::*` gives you access to the full set of wasm-dbms data types, validators, and sanitizers.

### Define Tables

In `schema/src/lib.rs`, define your database tables using the `Table` derive macro:

```rust
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
    #[validate(EmailValidator)]
    pub email: Text,
    pub created_at: DateTime,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    #[validate(MaxStrlenValidator(200))]
    pub title: Text,
    pub content: Text,
    pub published: Boolean,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}
```

**Required derives:** `Table`, `CandidType`, `Deserialize`, `Clone`

The `Table` macro generates additional types for each table:

| Generated Type | Purpose |
|----------------|---------|
| `UserRecord` | Full record returned from queries |
| `UserInsertRequest` | Request type for inserting records |
| `UserUpdateRequest` | Request type for updating records |
| `UserForeignFetcher` | Internal type for relationship loading |

---

## Create the DBMS Canister

### Canister Dependencies

Create `canister/Cargo.toml`:

```toml
[package]
name = "my-canister"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
candid = "0.10"
ic-cdk = "0.19"
ic-dbms-api = "0.6"
ic-dbms-canister = "0.6"
my-schema = { path = "../schema" }
serde = "1"
```

### Generate the Canister API

In `canister/src/lib.rs`:

```rust
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};
use my_schema::{User, Post};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct MyDbmsCanister;

ic_cdk::export_candid!();
```

The `DatabaseSchema` derive generates the `DatabaseSchema<M>` trait implementation that provides schema dispatch (routing operations to the correct table by name). The `DbmsCanister` derive generates the complete canister API:

```candid
service : (IcDbmsCanisterArgs) -> {
  // ACL Management
  acl_add_principal : (principal) -> (Result);
  acl_allowed_principals : () -> (vec principal) query;
  acl_remove_principal : (principal) -> (Result);

  // Transactions
  begin_transaction : () -> (nat);
  commit : (nat) -> (Result);
  rollback : (nat) -> (Result);

  // Users CRUD
  insert_users : (UserInsertRequest, opt nat) -> (Result);
  select_users : (Query, opt nat) -> (Result_1) query;
  update_users : (UserUpdateRequest, opt nat) -> (Result_2);
  delete_users : (DeleteBehavior, opt Filter, opt nat) -> (Result_2);

  // Posts CRUD
  insert_posts : (PostInsertRequest, opt nat) -> (Result);
  select_posts : (Query, opt nat) -> (Result_3) query;
  update_posts : (PostUpdateRequest, opt nat) -> (Result_2);
  delete_posts : (DeleteBehavior, opt Filter, opt nat) -> (Result_2);
}
```

### Build the Canister

Create a build script or use the following commands:

```bash
# Build the canister
cargo build --target wasm32-unknown-unknown --release -p my-canister

# Optimize the WASM
ic-wasm target/wasm32-unknown-unknown/release/my_canister.wasm \
    -o my_canister.wasm shrink

# Extract Candid interface
candid-extractor my_canister.wasm > my_canister.did

# Optionally compress
gzip -k my_canister.wasm --force
```

---

## Deploy the Canister

### Canister Init Arguments

The canister requires initialization arguments specifying which principals can access the database:

```candid
type IcDbmsCanisterArgs = variant {
  Init : IcDbmsCanisterInitArgs;
  Upgrade;
};

type IcDbmsCanisterInitArgs = record {
  allowed_principals : vec principal;
};
```

> **Warning:** Only principals in `allowed_principals` can perform database operations. Make sure to include all necessary principals (your frontend canister, admin principal, etc.).

### Deploy with dfx

Create `dfx.json`:

```json
{
  "canisters": {
    "my_dbms": {
      "type": "custom",
      "candid": "my_canister.did",
      "wasm": "my_canister.wasm",
      "build": []
    }
  }
}
```

Deploy:

```bash
dfx deploy my_dbms --argument '(variant { Init = record { allowed_principals = vec { principal "your-principal-here" } } })'
```

---

## Quick Example: Complete Workflow

Here's a complete example showing insert, query, update, and delete operations:

```rust
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};
use my_schema::{User, UserInsertRequest, UserUpdateRequest};
use ic_dbms_api::prelude::*;

async fn example(canister_id: Principal) -> Result<(), Box<dyn std::error::Error>> {
    let client = IcDbmsCanisterClient::new(canister_id);

    // 1. INSERT a new user
    let insert_req = UserInsertRequest {
        id: 1.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        created_at: DateTime::now(),
    };
    client.insert::<User>(User::table_name(), insert_req, None).await??;

    // 2. SELECT users
    let query = Query::builder()
        .filter(Filter::eq("name", Value::Text("Alice".into())))
        .build();
    let users = client.select::<User>(User::table_name(), query, None).await??;
    println!("Found {} user(s)", users.len());

    // 3. UPDATE the user
    let update_req = UserUpdateRequest::builder()
        .set_email("alice.new@example.com".into())
        .filter(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let updated = client.update::<User>(User::table_name(), update_req, None).await??;
    println!("Updated {} record(s)", updated);

    // 4. DELETE the user
    let deleted = client.delete::<User>(
        User::table_name(),
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(1.into()))),
        None
    ).await??;
    println!("Deleted {} record(s)", deleted);

    Ok(())
}
```

---

## Integration Testing

For integration tests using PocketIC, add `ic-dbms-client` with the `pocket-ic` feature:

```toml
[dev-dependencies]
ic-dbms-client = { version = "0.6", features = ["pocket-ic"] }
pocket-ic = "9"
```

Example test:

```rust
use ic_dbms_client::prelude::{Client as _, IcDbmsPocketIcClient};
use my_schema::{User, UserInsertRequest};
use pocket_ic::PocketIc;

#[tokio::test]
async fn test_insert_and_select() {
    let pic = PocketIc::new();
    // ... setup canister ...

    let client = IcDbmsPocketIcClient::new(canister_id, admin_principal, &pic);

    let insert_req = UserInsertRequest {
        id: 1.into(),
        name: "Test User".into(),
        email: "test@example.com".into(),
        created_at: DateTime::now(),
    };

    client
        .insert::<User>(User::table_name(), insert_req, None)
        .await
        .expect("call failed")
        .expect("insert failed");

    let query = Query::builder().all().build();
    let users = client
        .select::<User>(User::table_name(), query, None)
        .await
        .expect("call failed")
        .expect("select failed");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name.as_str(), "Test User");
}
```

---

## Next Steps

Now that you have a working canister, explore these topics:

- [CRUD Operations (IC)](./crud-operations.md) - Detailed guide on all database operations via the IC client
- [Access Control](./access-control.md) - Managing the ACL
- [Client API](./client-api.md) - All client types and usage patterns
- [Schema Definition (IC)](../reference/schema.md) - IC-specific schema reference (DbmsCanister macro, Candid API)
- [Data Types (IC)](../reference/data-types.md) - IC-specific data types (Principal, Candid mappings)
- [Errors (IC)](../reference/errors.md) - IC-specific error handling (double-Result pattern)

For core wasm-dbms concepts (querying, transactions, relationships, validators, sanitizers), see the [generic guides](../../guides/).

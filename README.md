# WASM DBMS

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms.svg)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![ko-fi](https://img.shields.io/badge/donate-ko--fi-red)](https://ko-fi.com/veeso)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms/badge.svg)](https://docs.rs/wasm-dbms)

A Rust framework for building database applications on WASM runtimes, with first-class support for Internet Computer
canisters.

## Overview

This repository contains two crate families:

- **wasm-dbms** - A runtime-agnostic DBMS engine that runs on any WASM runtime (Wasmtime, Wasmer, WasmEdge, IC)
- **ic-dbms** - A thin IC-specific adapter that provides Internet Computer canister integration

### Crate Architecture

| Crate              | Description                                                 |
|--------------------|-------------------------------------------------------------|
| `wasm-dbms-api`    | Shared types, traits, validators, sanitizers                |
| `wasm-dbms-memory` | Memory abstraction and page management                      |
| `wasm-dbms`        | Core DBMS engine with transactions, joins, integrity checks |
| `wasm-dbms-macros` | Procedural macros: `Encode`, `Table`, `CustomDataType`      |
| `ic-dbms-api`      | IC-specific types (re-exports `wasm-dbms-api`)              |
| `ic-dbms-canister` | IC canister DBMS implementation                             |
| `ic-dbms-macros`   | IC-specific macro: `DbmsCanister`                           |
| `ic-dbms-client`   | Client libraries for canister interaction                   |

## Quick Start (Generic)

Define your database schema using Rust structs with derive macros:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
    #[validate(EmailValidator)]
    pub email: Text,
}
```

Wire tables together with the `DatabaseSchema` macro and use the `Database` trait:

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;

#[derive(DatabaseSchema)]
#[tables(User = "users")]
pub struct MySchema;

// Create a database context with any MemoryProvider
let ctx = DbmsContext::new(HeapMemoryProvider::default());
MySchema::register_tables(&ctx)?;

let database = WasmDbmsDatabase::oneshot(&ctx, MySchema);

// Insert
database.insert::<User>(UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
})?;

// Query
let users = database.select::<User>(Query::builder().all().build())?;
```

The `MemoryProvider` trait abstracts storage — use `HeapMemoryProvider` for testing, `FileMemoryProvider` for
Wasmtime/WASI, or implement your own for any WASM runtime.

### Component Model (WIT)

wasm-dbms can be exposed as a [WebAssembly Component](https://component-model.bytecodealliance.org/) via a WIT
interface (`/wit/dbms.wit`), making it accessible from any Component Model host — Go, Python, JavaScript, or any
language with Component Model tooling. See the [Wasmtime example](https://wasm-dbms.cc/guides/wasmtime-example.html)
and the reference implementation in `crates/wasm-dbms/example/`.

## Quick Start (IC Canister)

Define your database schema using Rust structs:

```rust
use candid::CandidType;
use ic_dbms_api::prelude::{Text, Uint32};
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister, Table};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint64,
    #[sanitizer(ic_dbms_api::prelude::TrimSanitizer)]
    #[validate(ic_dbms_api::prelude::MaxStrlenValidator(20))]
    name: Text,
    #[validate(ic_dbms_api::prelude::EmailValidator)]
    email: Text,
    age: Nullable<Uint32>,
}
```

Define relationships between tables:

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    id: Uint32,
    title: Text,
    content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    author: Uint32,
}
```

> [!NOTE]
> Deriving `CandidType`, `Deserialize` and `Clone` is required for IC canister tables.

Instantiate the database canister:

```rust
#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct IcDbmsCanisterGenerator;
```

This generates a fully functional database canister with all CRUD operations.

## Generated Canister API

```candid
service : (IcDbmsCanisterArgs) -> {
  acl_add_principal : (principal) -> (Result);
  acl_allowed_principals : () -> (vec principal) query;
  acl_remove_principal : (principal) -> (Result);
  begin_transaction : () -> (nat);
  commit : (nat) -> (Result);
  delete_posts : (DeleteBehavior, opt Filter_1, opt nat) -> (Result_1);
  delete_users : (DeleteBehavior, opt Filter_1, opt nat) -> (Result_1);
  insert_posts : (PostInsertRequest, opt nat) -> (Result);
  insert_users : (UserInsertRequest, opt nat) -> (Result);
  rollback : (nat) -> (Result);
  select_posts : (Query, opt nat) -> (Result_2) query;
  select_users : (Query_1, opt nat) -> (Result_3) query;
  update_posts : (PostUpdateRequest, opt nat) -> (Result_1);
  update_users : (UserUpdateRequest, opt nat) -> (Result_1);
}
```

### ACL Management

- `acl_add_principal(principal)`: Adds a principal to the ACL.
- `acl_allowed_principals()`: Returns the list of principals in the ACL.
- `acl_remove_principal(principal)`: Removes a principal from the ACL.

### Transaction Management

- `begin_transaction()`: Starts a new transaction and returns its ID.
- `commit(transaction_id)`: Commits the transaction with the given ID.
- `rollback(transaction_id)`: Rolls back the transaction with the given ID.

### Data Manipulation

For each table defined in the schema:

- `insert_<table_name>(records, transaction_id)`: Inserts records into the specified table.
- `select_<table_name>(query, transaction_id)`: Selects records from the specified table.
- `update_<table_name>(updates, transaction_id)`: Updates records in the specified table.
- `delete_<table_name>(delete_behavior, filter, transaction_id)`: Deletes records from the specified table.

## Getting Started

See the [Getting Started Guide](https://wasm-dbms.cc/guides/get-started.html) for more information on how to setup and
deploy the DBMS canister.

## Interacting with the Canister

See the [ic-dbms-client](./crates/ic-dbms/ic-dbms-client/README.md) for more information on how to interact with the
canister.

## Features

- [x] Define tables with common attributes
- [x] CRUD operations
- [x] Complex queries with filtering and pagination
- [x] Relationships between tables with foreign keys
- [x] Transactions with commit and rollback
- [x] Access Control Lists (ACL) to restrict access to the database
- [x] Validation, Sanitizers and constraints on table columns
- [x] JOIN operations between tables
- [x] Custom data types
- [x] Runtime-agnostic core (wasm-dbms) for any WASM runtime
- [x] Indexes for faster queries
- [ ] Migrations to update the database schema on canister upgrades
- [ ] SQL query support

## Documentation

Read the documentation at <https://wasm-dbms.cc>

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

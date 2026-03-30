---
title: "wasm-dbms"
description: "A framework to implement relational databases on any WASM runtime."
nav_order: 1
---

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms.svg)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms/badge.svg)](https://docs.rs/wasm-dbms)

---

## Documentation

### Guides

Step-by-step guides for building databases with wasm-dbms:

- [Getting Started](./guides/get-started.md) - Set up your first wasm-dbms database
- [CRUD Operations](./guides/crud-operations.md) - Insert, select, update, and delete records
- [Querying](./guides/querying.md) - Filters, ordering, pagination, and field selection
- [Transactions](./guides/transactions.md) - ACID transactions with commit/rollback
- [Relationships](./guides/relationships.md) - Foreign keys, delete behaviors, and eager loading
- [Custom Data Types](./guides/custom-data-types.md) - Define your own data types (enums, structs)
- [Wasmtime Example](./guides/wasmtime-example.md) - Using wasm-dbms with the WIT Component Model and Wasmtime

### Reference

API and type reference documentation:

- [Data Types](./reference/data-types.md) - All supported column types
- [Schema Definition](./reference/schema.md) - Table attributes and generated types
- [Validation](./reference/validation.md) - Built-in and custom validators
- [Sanitization](./reference/sanitization.md) - Built-in and custom sanitizers
- [JSON](./reference/json.md) - JSON data type and filtering
- [Errors](./reference/errors.md) - Error types and handling

### IC Integration

For deploying wasm-dbms as an Internet Computer canister:

- [IC Getting Started](./ic/guides/get-started.md) - Deploy a database canister on the IC
- [Access Control](./ic/guides/access-control.md) - Managing the ACL
- [Client API](./ic/guides/client-api.md) - Using the IC client library

### Technical Documentation

For advanced users and contributors:

- [Architecture](./technical/architecture.md) - Three-layer system overview
- [Memory Management](./technical/memory.md) - Stable memory internals
- [Join Engine](./technical/join-engine.md) - Cross-table join query internals

---

## Quick Example

Define your schema:

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

Use the `Database` trait for CRUD operations:

```rust
use wasm_dbms_api::prelude::*;

// Insert
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
};
database.insert::<User>(user)?;

// Query
let query = Query::builder()
    .filter(Filter::eq("name", Value::Text("Alice".into())))
    .build();
let users = database.select::<User>(query)?;
```

---

## Features

- **Schema-driven**: Define tables as Rust structs with derive macros
- **Runtime-agnostic**: Works on any WASM runtime, not tied to a specific platform
- **CRUD operations**: Full insert, select, update, delete support
- **ACID transactions**: Commit/rollback with isolation
- **Foreign keys**: Referential integrity with cascade/restrict behaviors
- **Validation & Sanitization**: Built-in validators and sanitizers
- **JSON support**: Store and query semi-structured data
- **IC Integration**: First-class support for Internet Computer canisters via `ic-dbms`

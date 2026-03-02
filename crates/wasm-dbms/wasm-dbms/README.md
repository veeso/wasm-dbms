# wasm-dbms

[![license-mit](https://img.shields.io/crates/l/wasm-dbms.svg)](https://opensource.org/licenses/MIT)
[![downloads](https://img.shields.io/crates/d/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms.svg)](https://crates.io/crates/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms/badge.svg)](https://docs.rs/wasm-dbms)

A runtime-agnostic DBMS engine for WASM environments. Define your database schema using Rust structs with derive macros
and get CRUD operations, ACID transactions, foreign key integrity, and validation out of the box.

## Crate Architecture

wasm-dbms is part of a family of crates:

```txt
wasm-dbms-macros <── wasm-dbms-api <── wasm-dbms-memory <── wasm-dbms
```

| Crate              | Description                                                 |
|--------------------|-------------------------------------------------------------|
| `wasm-dbms-api`    | Shared types, traits, validators, sanitizers                |
| `wasm-dbms-memory` | Memory abstraction and page management                      |
| `wasm-dbms`        | Core DBMS engine with transactions, joins, integrity checks |
| `wasm-dbms-macros` | Procedural macros: `Encode`, `Table`, `CustomDataType`      |

## Quick Start

Add the dependencies to your `Cargo.toml`:

```toml
[dependencies]
wasm-dbms = "0.6"
wasm-dbms-api = "0.6"
```

### Define Your Tables

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

Required derives: `Table`, `Clone`

### Define Relationships

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}
```

### Create a Database Schema

Wire your tables together with `#[derive(DatabaseSchema)]`:

```rust
use wasm_dbms::prelude::DatabaseSchema;

#[derive(DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct MySchema;
```

### Use the Database

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;

// Create the context (owns all database state)
let ctx = DbmsContext::new(HeapMemoryProvider::default());
MySchema::register_tables(&ctx).expect("failed to register tables");

// Create a one-shot (non-transactional) database session
let database = WasmDbmsDatabase::oneshot(&ctx, MySchema);

// Insert a record
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
};
database.insert::<User>(user)?;

// Query records
let query = Query::builder()
    .filter(Filter::eq("name", Value::Text("Alice".into())))
    .build();
let users = database.select::<User>(query)?;
```

### Transactions

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;

// Begin a transaction
let tx_id = ctx.begin_transaction(caller_id);
let mut database = WasmDbmsDatabase::from_transaction(&ctx, MySchema, tx_id);

database.insert::<User>(user)?;
database.update::<User>(update_req)?;

// Commit or rollback
database.commit()?;
// database.rollback()?;
```

## Features

- CRUD operations with type-safe insert/update/select/delete
- ACID transactions with commit and rollback
- Foreign key relationships with referential integrity
- Complex queries with filtering, ordering, and pagination
- JOIN operations between tables
- Validation and sanitization on table columns
- Custom data types
- Runtime-agnostic: works on any WASM runtime (Wasmtime, Wasmer, WasmEdge, Internet Computer, etc.)

## Documentation

Read the full documentation at <https://wasm-dbms.cc>

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

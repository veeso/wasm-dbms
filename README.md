# WASM DBMS

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms.svg?logo=rust)](https://crates.io/crates/wasm-dbms)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms.svg?logo=rust)](https://crates.io/crates/wasm-dbms)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms/badge.svg?logo=rust)](https://docs.rs/wasm-dbms)

An embeddable relational database engine written in Rust, designed to run inside WASM runtimes. Internet Computer
canisters are supported through the dedicated [ic-dbms](https://github.com/veeso/ic-dbms) project.

## Overview

This repository contains two crate families:

- **wasi-dbms**: Crates for building a DBMS on any WASM runtime that supports WASI (Wasmtime, Wasmer, WasmEdge)
- **wasm-dbms** - A runtime-agnostic DBMS engine that runs on any WASM runtime (Wasmtime, Wasmer, WasmEdge, IC)

### Crate Architecture

| Crate              | Description                                                 |
| ------------------ | ----------------------------------------------------------- |
| `wasm-dbms-api`    | Shared types, traits, validators, sanitizers                |
| `wasm-dbms-memory` | Memory abstraction and page management                      |
| `wasm-dbms`        | Core DBMS engine with transactions, joins, integrity checks |
| `wasm-dbms-macros` | Procedural macros: `Encode`, `Table`, `CustomDataType`      |
| `wasi-dbms-memory` | Memory provider implementations for WASI runtimes           |

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

## Internet Computer

Internet Computer support lives in the dedicated [ic-dbms](https://github.com/veeso/ic-dbms) repository. It provides the
`ic-dbms-api`, `ic-dbms-canister`, `ic-dbms-macros`, and `ic-dbms-client` crates, which turn a wasm-dbms schema into a
complete database canister with a generated Candid API and client libraries.

Read the ic-dbms documentation at <https://ic.wasm-dbms.cc>.

## Getting Started

See the [Getting Started Guide](https://wasm-dbms.cc/guides/get-started.html) to set up your first wasm-dbms database.

## Features

- [x] Define tables with common attributes
- [x] CRUD operations
- [x] Complex queries with filtering and pagination
- [x] Relationships between tables with foreign keys
- [x] Transactions with commit and rollback
- [x] Aggregation functions (COUNT, SUM, AVG, etc.)
- [x] Validation, Sanitizers and constraints on table columns
- [x] JOIN operations between tables
- [x] Custom data types
- [x] Runtime-agnostic core (wasm-dbms) for any WASM runtime
- [x] Indexes for faster queries
- [x] Migrations to update the database schema on the fly
- [ ] SQL query support

## Documentation

Read the documentation at <https://wasm-dbms.cc>

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

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

## Why wasm-dbms?

A WASM module is a sandbox with a linear memory and little else. Most embedded databases expect a filesystem and a C
toolchain, so using them from WASM means porting a native engine, depending on storage provided by one specific host,
or building tables by hand on a key-value store. wasm-dbms is a relational engine designed for the sandbox instead:

- **Runs wherever WASM runs**: pure Rust, builds for `wasm32-unknown-unknown`, no C toolchain, WASI or JavaScript
  glue required
- **Storage is a trait**: the engine works on 64 KiB pages behind `MemoryProvider`, so the same database runs on the
  heap, on a file, on Internet Computer stable memory, or on your own storage
- **The schema is Rust code**: tables are structs, queries are typed, and mistakes are compile errors
- **Relational, not key-value**: foreign keys, joins, transactions, indexes and migrations are built in
- **Ships inside your module**: no connection, no network round trip, no service to operate

### How It Compares

| Option                    | Engine language | How you query                 | Where it runs                             |
| ------------------------- | --------------- | ----------------------------- | ----------------------------------------- |
| **wasm-dbms**             | Rust            | Typed Rust API, WIT interface | Any WASM runtime                          |
| SQLite compiled to WASM   | C               | SQL                           | Browsers, WASI runtimes                   |
| Turso Database            | Rust            | SQL (SQLite compatible)       | Native, browsers through WASM bindings    |
| GlueSQL                   | Rust            | SQL, query builder            | Native, browsers and Node.js              |
| DuckDB-Wasm               | C++             | SQL (analytics)               | Browsers                                  |
| PGlite                    | C               | SQL (Postgres)                | Browsers, Node.js, Bun                    |
| Embedded key-value stores | Rust            | Get, put, range               | Native targets, custom backends elsewhere |
| Host-provided storage     | Host specific   | Host SQL or key-value API     | That host only                            |

wasm-dbms is not the best fit for every project. The [full comparison](https://wasm-dbms.cc/why-wasm-dbms.html)
explains when to choose each alternative, and when wasm-dbms is the wrong tool.

## Overview

This repository contains two crate families:

- **wasi-dbms**: Crates for building a DBMS on any WASM runtime that supports WASI (Wasmtime, Wasmer, WasmEdge)
- **wasm-dbms** - A runtime-agnostic DBMS engine that runs on any WASM runtime (Wasmtime, Wasmer, WasmEdge)

### Crate Architecture

| Crate                        | Description                                                 |
| ---------------------------- | ----------------------------------------------------------- |
| `wasm-dbms-api`              | Shared types, traits, validators, sanitizers                |
| `wasm-dbms-memory`           | Memory abstraction and page management                      |
| `wasm-dbms`                  | Core DBMS engine with transactions, joins, integrity checks |
| `wasm-dbms-macros`           | Procedural macros: `Encode`, `Table`, `CustomDataType`      |
| `wasm-dbms-sql`              | SQL front-end: parser, planner, and executor                |
| `wasi-dbms-memory`           | Memory provider implementations for WASI runtimes           |
| `wasi-dbms-key-value-memory` | Draft2 key-value `MemoryProvider` with checkpoints          |

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

The `MemoryProvider` trait abstracts storage — use `HeapMemoryProvider` for testing, `WasiMemoryProvider` for
Wasmtime/WASI, or implement your own for any WASM runtime.

## Quick Start (WASI)

For a WASI filesystem, use the file-backed provider. It stores the same 64 KiB
pages in one file and needs a preopened directory:

```rust,no_run
use wasi_dbms_memory::WasiMemoryProvider;
use wasm_dbms::DbmsContext;

let ctx = DbmsContext::new(WasiMemoryProvider::new("./data/example.db")?);
# Ok::<(), Box<dyn std::error::Error>>(())
```

See the [WASI Memory Provider guide](docs/wasi/wasi-memory-provider.md).

## Quick Start (WASI key-value)

When the host provides persistent key-value storage, use the draft2 provider:

```rust,no_run
use wasi_dbms_key_value_memory::WasiKeyValueMemoryProvider;
use wasm_dbms::DbmsContext;

let ctx = DbmsContext::new(WasiKeyValueMemoryProvider::new("default", "example")?);
// Register tables and perform committed operations.
ctx.flush()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The host must implement `wasi:keyvalue/store@0.2.0-draft2` and
`wasi:keyvalue/batch@0.2.0-draft2`; `ctx.flush()` is the explicit checkpoint
boundary. See the [key-value provider guide](docs/wasi/wasi-key-value-memory-provider.md).

### Component Model (WIT)

wasm-dbms can be exposed as a [WebAssembly Component](https://component-model.bytecodealliance.org/) via a WIT
interface (`/wit/dbms.wit`), making it accessible from any Component Model host — Go, Python, JavaScript, or any
language with Component Model tooling. See the [Wasmtime example](https://wasm-dbms.cc/guides/wasmtime-example.html)
and the reference implementation in `crates/wasm-dbms/example/`.

## Internet Computer

Internet Computer support lives in the dedicated [ic-dbms](https://github.com/veeso/ic-dbms) repository.
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
- [x] SQL query support

## Documentation

Read the documentation at <https://wasm-dbms.cc>

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

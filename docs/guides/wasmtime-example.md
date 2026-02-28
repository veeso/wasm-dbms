# Wasmtime Example

- [Overview](#overview)
- [How It Works](#how-it-works)
  - [WIT Interface](#wit-interface)
  - [Guest Component](#guest-component)
  - [Host Binary](#host-binary)
- [FileMemoryProvider](#filememoryprovider)
- [Building and Running](#building-and-running)
  - [Prerequisites](#prerequisites)
  - [Build](#build)
  - [Run](#run)
- [Extending with Custom Tables](#extending-with-custom-tables)
- [Key Concepts](#key-concepts)

This guide explains how to use wasm-dbms with the WebAssembly Component Model (WIT) and Wasmtime. It walks through the example in `crates/wasm-dbms/example/`.

---

## Overview

The [WebAssembly Component Model](https://component-model.bytecodealliance.org/) defines a standard way for WASM modules to expose typed interfaces using WIT (WebAssembly Interface Types). This example shows how to:

1. Define a WIT interface for the wasm-dbms CRUD and transaction API
2. Build a guest WASM component that wraps wasm-dbms behind the WIT interface
3. Run the guest inside a native Wasmtime host

This approach makes wasm-dbms usable from any Component Model host, not just Rust. The WIT contract at `/wit/dbms.wit` can be consumed by hosts written in Go, Python, JavaScript, or any language with Component Model tooling.

---

## How It Works

### WIT Interface

The WIT definition (`/wit/dbms.wit`) exposes a `database` interface with these operations:

- **select** — query rows from a table with optional filter, ordering, limit, and offset
- **insert** — insert a row into a table, optionally within a transaction
- **update** — update rows matching a filter, optionally within a transaction
- **delete** — delete rows matching a filter, optionally within a transaction
- **begin-transaction** — start a new ACID transaction
- **commit** / **rollback** — finalize or abort a transaction

Values are passed as a `value` variant type that covers booleans, integers, floats, strings, blobs, and null. Filters are JSON-serialized strings matching the `wasm_dbms_api::Filter` type.

This raw/dynamic API is intentional: WIT cannot express Rust generics or user-defined table schemas, so type safety is enforced inside the guest by the wasm-dbms engine.

### Guest Component

The guest (`crates/wasm-dbms/example/guest/`) compiles to `wasm32-wasip2` and exports the WIT `database` interface. Internally it:

1. Initializes a `DbmsContext<FileMemoryProvider>` lazily on first call
2. Registers example tables (`users`, `posts`) using `#[derive(Table)]`
3. Converts between WIT variant values and wasm-dbms `Value` types
4. Dispatches operations through a `DatabaseSchema` implementation

The bridge layer in `lib.rs` handles all the type conversions between the WIT boundary and the typed wasm-dbms internals.

### Host Binary

The host (`crates/wasm-dbms/example/host/`) is a native Rust binary using Wasmtime. It:

1. Creates a Wasmtime engine with Component Model enabled
2. Sets up a WASI context with a preopened directory for the database file
3. Loads the guest `.wasm` component and instantiates it
4. Calls the exported `database` functions to demonstrate all operations

---

## FileMemoryProvider

The `FileMemoryProvider` implements the `MemoryProvider` trait using `std::fs` file I/O. It provides persistent, file-backed storage so that data survives across invocations.

```rust
use wasm_dbms_memory::prelude::MemoryProvider;

pub struct FileMemoryProvider {
    file: File,       // open file handle
    size: u64,        // current size in bytes
    pages: u64,       // allocated pages (size / PAGE_SIZE)
}
```

Operations:
- **grow(n)** — extends the file by `n × 65536` bytes
- **read(offset, buf)** — seeks to offset and reads into buffer
- **write(offset, buf)** — seeks to offset, writes buffer, and flushes

The provider is initialized with a file path relative to the WASI preopened directory (defaults to `wasm-dbms.db`).

> **Note:** `FileMemoryProvider` does not handle concurrent access. It assumes single-writer usage.

---

## Building and Running

### Prerequisites

- Rust 1.85.1+
- `wasm32-wasip2` target:

  ```bash
  rustup target add wasm32-wasip2
  ```

- [just](https://github.com/casey/just) command runner

### Build

```bash
# Build guest + host
just build_wasm_dbms_example
```

This compiles the guest to `wasm32-wasip2` (producing a WASM component at `.artifact/wasm-dbms-example-guest.wasm`) and builds the native host binary.

### Run

```bash
just test_wasm_dbms_example
```

Or run manually:

```bash
cargo run --release -p wasm-dbms-example-host -- .artifact/wasm-dbms-example-guest.wasm
```

The demo inserts users and posts, queries them with filters and ordering, demonstrates transaction commit (data persists) and rollback (data discarded), then cleans up.

---

## Extending with Custom Tables

To add your own tables to the example:

1. **Define the table** in `guest/src/schema.rs`:

   ```rust
   #[derive(Debug, Table, Clone, PartialEq, Eq)]
   #[table = "comments"]
   pub struct Comment {
       #[primary_key]
       pub id: Uint32,
       pub body: Text,
       #[foreign_key(entity = "Post", table = "posts", column = "id")]
       pub post_id: Uint32,
   }
   ```

2. **Add dispatch arms** for `"comments"` in every method of `ExampleDatabaseSchema` in `guest/src/schema.rs` (`select`, `insert`, `update`, `delete`, `validate_insert`, `validate_update`, `referenced_tables`).

3. **Register the table** in `register_tables()`:

   ```rust
   ctx.register_table::<Comment>()?;
   ```

4. **Update the column lookup** in `table_columns()` (`guest/src/lib.rs`):

   ```rust
   "comments" => Ok(schema::Comment::columns()),
   ```

5. **Rebuild** with `just build_wasm_dbms_example`.

---

## Key Concepts

| Concept | Description |
|---------|-------------|
| **WIT** | WebAssembly Interface Types — a language for defining typed component interfaces |
| **Component Model** | The standard for composing WASM modules with defined imports/exports |
| **`wasm32-wasip2`** | Rust compilation target that produces WASM components with WASI Preview 2 support |
| **`wit-bindgen`** | Guest-side code generator that creates Rust types from WIT definitions |
| **`wasmtime::component::bindgen!`** | Host-side macro that generates Rust types for calling WIT interfaces |
| **`DatabaseSchema`** | wasm-dbms trait that dispatches generic operations to concrete table types |
| **`FileMemoryProvider`** | File-backed `MemoryProvider` implementation for persistent storage |

---

## Next Steps

- [Getting Started](./get-started.md) — Set up wasm-dbms from scratch with the `Database` trait
- [CRUD Operations](./crud-operations.md) — Detailed guide on all database operations
- [Transactions](./transactions.md) — ACID transactions with commit/rollback
- [Schema Definition](../reference/schema.md) — Complete schema reference

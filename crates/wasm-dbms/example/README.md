# wasm-dbms Wasmtime Example

This example demonstrates using wasm-dbms with the [WebAssembly Component Model](https://component-model.bytecodealliance.org/) (WIT) and [Wasmtime](https://wasmtime.dev/).

- [Architecture](#architecture)
- [WIT Interface](#wit-interface)
- [Prerequisites](#prerequisites)
- [Build](#build)
- [Run](#run)
- [Extending with Custom Tables](#extending-with-custom-tables)

---

## Architecture

The example consists of two components:

- **Guest** (`guest/`): A WASM component compiled to `wasm32-wasip2` that wraps wasm-dbms behind a WIT-exported `database` interface. It includes a `FileMemoryProvider` for persistent, file-backed storage and registers two example tables (`users`, `posts`).

- **Host** (`host/`): A native Rust binary that uses Wasmtime to load the guest component and exercise every exported operation: insert, select, transactions with commit, and transactions with rollback.

```
┌─────────────────────────────────────────────────┐
│                  Host (native)                  │
│   Wasmtime engine + WASI context                │
│     ↕ WIT bindings (database interface)         │
├─────────────────────────────────────────────────┤
│               Guest (wasm32-wasip2)             │
│   WIT exports ← bridge → wasm-dbms engine       │
│   FileMemoryProvider → wasm-dbms.db              │
└─────────────────────────────────────────────────┘
```

---

## WIT Interface

The interface definition lives at `/wit/dbms.wit` (workspace root). It defines a raw/dynamic API where table names are strings and column values are variant types. Type safety is enforced internally by the wasm-dbms engine, not at the WIT boundary.

Key operations:

| Operation | Signature |
|-----------|-----------|
| `select` | `(table, query) → list<row>` |
| `insert` | `(table, row, tx?) → ()` |
| `update` | `(table, row, tx?) → u64` |
| `delete` | `(table, filter?, tx?) → u64` |
| `begin-transaction` | `() → transaction-id` |
| `commit` | `(tx) → ()` |
| `rollback` | `(tx) → ()` |

---

## Prerequisites

- Rust 1.85.1+
- `wasm32-wasip2` target: `rustup target add wasm32-wasip2`
- [just](https://github.com/casey/just) command runner

---

## Build

```bash
# Build both guest and host
just build_wasm_dbms_example

# Or build individually
just build_wasm_dbms_example_guest   # → .artifact/wasm-dbms-example-guest.wasm
just build_wasm_dbms_example_host    # → target/release/wasm-dbms-example
```

---

## Run

```bash
just test_wasm_dbms_example

# Or run manually
cargo run --release -p wasm-dbms-example-host -- .artifact/wasm-dbms-example-guest.wasm
```

Expected output:

```
=== wasm-dbms WIT Component Model Demo ===
--- Inserting users ---
  Inserted user 1: Alice
  ...
--- Select all users (ordered by id ASC) ---
  { id: 1, name: "Alice", email: "alice@example.com" }
  ...
--- Transaction: commit (insert user 4 Diana) ---
  Committed.
  Verified: Diana exists after commit.
--- Transaction: rollback (insert user 5 Eve) ---
  Rolled back.
  Verified: Eve does NOT exist after rollback.
=== Demo complete ===
```

---

## Extending with Custom Tables

1. Define a new table struct in `guest/src/schema.rs`:

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

2. Add match arms for `"comments"` in every method of `ExampleDatabaseSchema` (`guest/src/schema.rs`).

3. Register the table in `register_tables()`:

   ```rust
   ctx.register_table::<Comment>()?;
   ```

4. Add the table to `table_columns()` in `guest/src/lib.rs`:

   ```rust
   "comments" => Ok(schema::Comment::columns()),
   ```

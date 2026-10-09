# Why wasm-dbms?

- [Why wasm-dbms?](#why-wasm-dbms)
  - [The Problem](#the-problem)
  - [What Makes wasm-dbms Different](#what-makes-wasm-dbms-different)
  - [Comparison with Alternatives](#comparison-with-alternatives)
    - [SQLite Compiled to WASM](#sqlite-compiled-to-wasm)
    - [Turso Database](#turso-database)
    - [GlueSQL](#gluesql)
    - [DuckDB-Wasm](#duckdb-wasm)
    - [PGlite](#pglite)
    - [Embedded Key-Value Stores](#embedded-key-value-stores)
    - [Host-Provided Storage](#host-provided-storage)
  - [When wasm-dbms Is Not the Right Choice](#when-wasm-dbms-is-not-the-right-choice)
  - [Benchmarks](#benchmarks)

---

## The Problem

A WebAssembly module is a sandbox. It owns a linear memory and whatever the host decides to import, and nothing else:
no filesystem, no sockets, no threads, no libc, unless the runtime grants them. Most embedded databases were designed
for an operating system instead. They expect files, file locks and memory mapping, and they often need a C toolchain to
build.

That leaves three common ways to keep relational data inside a WASM module:

1. **Port a native database to WASM.** SQLite, Postgres and DuckDB all have WASM builds. They bring a C or C++
   codebase, a storage shim for the missing filesystem, and usually a JavaScript or WASI host to run on.
2. **Use the storage the host provides.** Many runtimes expose a key-value or SQL API to their guests. The data then
   lives outside the module, and the code is tied to that one host.
3. **Build tables on top of a key-value store.** You get persistence, and then write the schema, indexes, foreign keys
   and transactions yourself.

wasm-dbms is a fourth option: a relational engine designed for the sandbox from the start.

---

## What Makes wasm-dbms Different

- **It runs wherever WASM runs.** The engine is pure Rust and builds for `wasm32-unknown-unknown` with plain `cargo`.
  It needs no C toolchain, no WASI, and no JavaScript glue.
- **Storage is a trait, not a filesystem.** The engine reads and writes 64 KiB pages, the same size as a WASM memory
  page, through the [`MemoryProvider`](./technical/memory.md) trait. The same engine runs on the heap for tests, on a
  file through [WASI](./wasi/wasi-memory-provider.md), on Internet Computer stable memory through
  [ic-dbms](https://github.com/veeso/ic-dbms), or on any storage you implement the trait for. WASI hosts with durable
  key-value storage can use the [draft2 key-value provider](./wasi/wasi-key-value-memory-provider.md) without a
  filesystem preopen.
- **The schema is Rust code.** Tables are structs with derive macros. Records, insert requests and update requests are
  generated types, so a wrong column type is a compile error and no query string is parsed at runtime.
- **It is relational, not key-value.** Foreign keys with cascade and restrict behaviors, joins, ACID transactions,
  B+ tree indexes, aggregates and schema migrations are part of the engine.
- **Data rules live next to the schema.** [Validators](./reference/validation.md) and
  [sanitizers](./reference/sanitization.md) are declared on the columns they protect and run on every write.
- **The database ships inside the module.** There is no connection, no network round trip and no service to operate.
  The host only sees pages.
- **It is not limited to Rust hosts.** The [WIT interface](./guides/wasmtime-example.md) exposes the database as a
  WebAssembly Component, so hosts written in Go, Python, JavaScript or any language with Component Model tooling can
  use it.

---

## Comparison with Alternatives

The table below reflects the state of each project in October 2026. If something is out of date, please
[open an issue](https://github.com/veeso/wasm-dbms/issues).

| Option                    | Engine language | How you query                 | Where the data lives                                                    | Where it runs                             |
| ------------------------- | --------------- | ----------------------------- | ----------------------------------------------------------------------- | ----------------------------------------- |
| **wasm-dbms**             | Rust            | Typed Rust API, WIT interface | Any `MemoryProvider`: heap, file, key-value, IC stable memory, your own | Any WASM runtime                          |
| SQLite compiled to WASM   | C               | SQL                           | Memory, OPFS, or files through WASI                                     | Browsers, WASI runtimes                   |
| Turso Database            | Rust            | SQL (SQLite compatible)       | SQLite file format                                                      | Native, browsers through WASM bindings    |
| GlueSQL                   | Rust            | SQL, query builder            | Swappable storages                                                      | Native, browsers and Node.js              |
| DuckDB-Wasm               | C++             | SQL (analytics)               | Browser memory, remote files                                            | Browsers                                  |
| PGlite                    | C               | SQL (Postgres)                | Memory, IndexedDB, filesystem                                           | Browsers, Node.js, Bun                    |
| Embedded key-value stores | Rust            | Get, put, range               | File or custom backend                                                  | Native targets, custom backends elsewhere |
| Host-provided storage     | Host specific   | Host SQL or key-value API     | Outside the module, managed by the host                                 | That host only                            |

### SQLite Compiled to WASM

[SQLite](https://sqlite.org/wasm/doc/trunk/index.md) is the most tested embedded database there is, and it has
official and community WASM builds. In Rust, [rusqlite](https://github.com/rusqlite/rusqlite) uses
[sqlite-wasm-rs](https://github.com/Spxg/sqlite-wasm-rs) on `wasm32-unknown-unknown`, which stores data in memory or
in the browser's OPFS and relies on `wasm-bindgen` or on host functions you provide. Building for WASI requires a C
toolchain such as the WASI SDK.

Choose SQLite when you need SQL compatibility, the SQLite file format, or its long production record. Choose
wasm-dbms when you want a pure Rust build with no C toolchain, storage that is not a filesystem, and a schema checked
by the Rust compiler.

### Turso Database

[Turso Database](https://github.com/tursodatabase/turso) is a rewrite of SQLite in Rust. It is compatible with the
SQLite SQL dialect and file format, has WASM bindings for browsers, and has not reached 1.0 yet.

Choose Turso when you want SQLite compatibility from a Rust codebase. Choose wasm-dbms when you want typed tables
instead of SQL strings and need to plug in your own page storage.

### GlueSQL

[GlueSQL](https://github.com/gluesql/gluesql) is a SQL engine written in Rust with swappable storages and a query
builder. It is the closest project to wasm-dbms in spirit. Its WASM support is delivered as a JavaScript package for
browsers and Node.js, with in-memory and browser storage backends.

Choose GlueSQL when you want SQL over many storage formats, such as JSON, CSV or Parquet files. Choose wasm-dbms when
you want schemas generated from Rust structs, with foreign keys, validators and sanitizers declared on the columns,
and a page-based storage layer made for WASM memory.

### DuckDB-Wasm

[DuckDB-Wasm](https://duckdb.org/docs/current/clients/wasm/overview) brings the DuckDB analytics engine to the
browser. It is built for analytical queries over large datasets and remote files such as Parquet.

Choose DuckDB-Wasm for analytics in the browser. Choose wasm-dbms for transactional workloads: many small reads and
writes on related records, in any WASM runtime.

### PGlite

[PGlite](https://pglite.dev/docs/about) is Postgres compiled to WASM and packaged as a TypeScript library for
browsers, Node.js and Bun.

Choose PGlite when you need real Postgres behavior and extensions in a JavaScript application. Choose wasm-dbms when
the database must live inside a Rust module on a runtime with no JavaScript.

### Embedded Key-Value Stores

Stores such as [redb](https://github.com/cberner/redb) give you ACID key-value storage in pure Rust, and redb accepts
a custom storage backend. They do not give you tables: the schema, secondary indexes, foreign keys, joins and
validation are yours to write and keep consistent.

Choose a key-value store when your data really is keys and values. Choose wasm-dbms when records reference each
other and you want the engine to keep them consistent.

### Host-Provided Storage

Many platforms give guests a database through host functions: [Spin](https://spinframework.dev/sqlite-api-guide)
offers SQLite and key-value stores, the `wasi:keyvalue` interface standardizes key-value access, and Cloudflare
Workers bind to [D1](https://developers.cloudflare.com/d1/). The data lives outside the module, the host manages it, and it can be shared between modules.

Choose host-provided storage when the data must outlive the module or be shared with other services on that
platform. Choose wasm-dbms when the module must stay portable across runtimes, or when the runtime offers nothing
more than memory, as on the Internet Computer.

---

## When wasm-dbms Is Not the Right Choice

wasm-dbms is not the best tool for every job. Pick an alternative when:

- **You need compatibility with an existing database.** If you must open SQLite files, rely on Postgres features or
  reuse existing database tooling, use SQLite, Turso or PGlite.
- **Your workload is analytical.** Scanning and aggregating very large datasets is what DuckDB is built for.
- **You need decades of production hardening.** wasm-dbms is younger than SQLite and has not reached 1.0, so its API
  can still change between releases.
- **The data must be shared across modules or services.** An embedded database belongs to one module. Use
  host-provided storage or an external database.
- **You only need keys and values.** A key-value store is simpler and smaller.

---

## Benchmarks

The repository includes a benchmark suite that compares wasm-dbms with in-memory SQLite and DuckDB across CRUD
operations, bulk inserts, queries and transactions. Results depend on the machine, so none are published here. Run
them yourself from a checkout:

```sh
just bench_compare
```

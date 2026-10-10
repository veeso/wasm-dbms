# Architecture

- [Architecture](#architecture)
  - [Overview](#overview)
  - [Layered Architecture](#layered-architecture)
    - [Layer 1: Memory Layer](#layer-1-memory-layer)
    - [Layer 2: DBMS Layer](#layer-2-dbms-layer)
    - [Layer 3: API Layer](#layer-3-api-layer)
    - [Layer 4: Application Layer](#layer-4-application-layer)
  - [Crate Organization](#crate-organization)
    - [Dependency Graph](#dependency-graph)
    - [Generic Layer (wasm-dbms)](#generic-layer-wasm-dbms)
      - [wasm-dbms-api](#wasm-dbms-api)
      - [wasm-dbms-memory](#wasm-dbms-memory)
      - [wasm-dbms](#wasm-dbms)
      - [wasm-dbms-sql](#wasm-dbms-sql)
      - [wasm-dbms-macros](#wasm-dbms-macros)
  - [Data Flow](#data-flow)
    - [Insert Operation](#insert-operation)
    - [Select Operation](#select-operation)
    - [Select with Join](#select-with-join)
    - [Transaction Flow](#transaction-flow)
  - [Extension Points](#extension-points)
    - [Custom Sanitizers](#custom-sanitizers)
    - [Custom Validators](#custom-validators)
    - [Custom Data Types](#custom-data-types)
    - [Memory Provider](#memory-provider)

---

## Overview

wasm-dbms is built as a layered architecture where each layer has specific responsibilities and builds upon the layer below. The core DBMS engine (layers 1 to 3) is runtime-agnostic and lives in the `wasm-dbms-*` crates. On top of it sits an application layer, which is not part of this repository: it is written by whoever embeds wasm-dbms, such as the Internet Computer adapter [ic-dbms](https://github.com/veeso/ic-dbms).

This design provides:

- **Separation of concerns**: Each layer focuses on one aspect
- **Testability**: Layers can be tested independently
- **Portability**: The generic layer runs on any WASM runtime (Wasmtime, Wasmer, WasmEdge)
- **Flexibility**: Internal implementations can change without affecting APIs

---

## Layered Architecture

```
┌──────────────────────────────────────────────────────────────┐
│               Layer 4: Application Layer                      │
│  Final database interface, permissions, ACL, custom logic     │
│  (user-owned, NOT part of wasm-dbms)                          │
╞══════════════════════════════════════════════════════════════╡
│                     Layer 3: API Layer                        │
│  Database trait, request/response types                       │
│  (Query, Filter, InsertRequest, UpdateRequest, Record)        │
├──────────────────────────────────────────────────────────────┤
│                     Layer 2: DBMS Layer                       │
│  Tables, CRUD operations, transactions, foreign keys          │
│  (TableRegistry, TransactionManager, query execution)         │
├──────────────────────────────────────────────────────────────┤
│                    Layer 1: Memory Layer                      │
│  Memory management, encoding/decoding, page allocation        │
│  (MemoryProvider, MemoryManager, Encode trait)                │
└──────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌──────────────────┐
                    │ Memory Provider  │
                    │ (file, host, or  │
                    │  heap memory)    │
                    └──────────────────┘
```

Layers 1 to 3 are the wasm-dbms engine. Layer 4 belongs to the application.

### Layer 1: Memory Layer

**Crate:** `wasm-dbms-memory`

**Responsibilities:**

- Manage memory allocation (64 KiB pages)
- Encode/decode data to/from binary format
- Track free space and handle fragmentation
- Provide abstraction for testing (heap vs persistent memory)

**Key components:**

| Component             | Purpose                                                                                                  |
| --------------------- | -------------------------------------------------------------------------------------------------------- |
| `MemoryProvider`      | Abstract interface for raw memory I/O                                                                    |
| `MemoryAccess`        | Trait for page-level read/write operations (implemented by `MemoryManager`, interceptable by DBMS layer) |
| `MemoryManager`       | Allocates and manages pages, implements `MemoryAccess`                                                   |
| `Encode` trait        | Binary serialization for all stored types                                                                |
| `PageLedger`          | Tracks which pages belong to which table                                                                 |
| `FreeSegmentsLedger`  | Tracks free space for reuse                                                                              |
| `IndexLedger`         | Manages B+ tree indexes for a table                                                                      |
| `AutoincrementLedger` | Tracks autoincrement counters per column                                                                 |

**Memory layout:**

```
Page 0: Schema Registry (table → page mapping)
Page 1: Unclaimed Pages Ledger (released pages available for reuse)
Page 2+: Table data (Page Ledger, Free Segments, Index Ledger, Autoincrement Ledger, Records, B-tree nodes)
```

See [Memory Documentation](./memory.md) for detailed technical information.

### Layer 2: DBMS Layer

**Crate:** `wasm-dbms`

**Responsibilities:**

- Implement CRUD operations
- Manage transactions with ACID properties
- Enforce foreign key constraints
- Handle sanitization and validation
- Execute queries with filters

**Key components:**

| Component            | Purpose                                                                          |
| -------------------- | -------------------------------------------------------------------------------- |
| `DbmsContext<M>`     | Owns all DBMS state (memory, schema, transactions, journal)                      |
| `WasmDbmsDatabase`   | Session-scoped DBMS operations                                                   |
| `TableRegistry`      | Manages records for a single table                                               |
| `TransactionSession` | Handles transaction lifecycle                                                    |
| `Transaction`        | Overlay for uncommitted changes                                                  |
| `IndexOverlay`       | Tracks uncommitted index changes within a transaction                            |
| `Journal`            | Write-ahead journal recording original bytes for rollback                        |
| `JournaledWriter`    | Wraps `MemoryManager` + `Journal`, implements `MemoryAccess` to intercept writes |
| `FilterAnalyzer`     | Extracts index plans from query filters                                          |
| `IndexReader`        | Unified view over base index and transaction overlay                             |
| `JoinEngine`         | Executes cross-table join queries                                                |

**Transaction model:**

```
┌──────────────────────────────────────────┐
│           Active Transactions             │
│  ┌─────────┐  ┌─────────┐  ┌─────────┐   │
│  │  Tx 1   │  │  Tx 2   │  │  Tx 3   │   │
│  │ (overlay)│  │(overlay)│  │(overlay)│   │
│  └────┬────┘  └────┬────┘  └────┬────┘   │
│       │            │            │         │
│       └────────────┼────────────┘         │
│                    │                      │
│                    ▼                      │
│         ┌─────────────────┐              │
│         │  Committed Data │              │
│         │   (in memory)   │              │
│         └─────────────────┘              │
└──────────────────────────────────────────┘
```

Transactions use an overlay pattern:

- Changes are written to an overlay (in-memory)
- Reading checks overlay first, then committed data
- Index changes are tracked in a separate `IndexOverlay` per table
- `IndexReader` merges base index results with overlay additions/removals
- Commit merges overlay to committed data and flushes index changes to B-trees
- Rollback discards the overlay — on-disk B-trees remain untouched

### Layer 3: API Layer

**Crate:** `wasm-dbms-api`

**Responsibilities:**

- Define the `Database` trait, the entry point for all operations
- Define request and response types
- Define shared data types, errors, sanitizers, and validators

**Key components:**

| Component      | Purpose                                               |
| -------------- | ----------------------------------------------------- |
| `Database`     | Trait for CRUD, queries, transactions, and migrations |
| Request types  | `InsertRequest`, `UpdateRequest`, `Query`, `Filter`   |
| Response types | `Record`, `DbmsError`, `DbmsResult`                   |

### Layer 4: Application Layer

**Crate:** none. This layer is written by the user of wasm-dbms.

The engine does not authenticate callers, check permissions, or decide how the database is exposed. The application layer wraps the `Database` trait and provides the interface that the final database presents to its clients.

**Typical responsibilities:**

- Expose the database to the outside world (host functions, WIT interface, RPC endpoints, CLI, and so on)
- Identify callers and enforce permissions or access control lists (ACL) before calling the engine
- Decide who may use a transaction id. The engine addresses transactions by id only; an application layer that serves several identities keeps its own ledger from id to identity (see [Embedding wasm-dbms](../guides/embedding.md))
- Add business rules that go beyond sanitizers and validators
- Provide the `MemoryProvider` for the target runtime and own the `DbmsContext`

For example, [ic-dbms](https://github.com/veeso/ic-dbms) is an application layer for the Internet Computer. It exposes canister endpoints, checks access control, and ships client libraries. Read its documentation at <https://ic.wasm-dbms.cc>.

---

## Crate Organization

```
wasm-dbms/
├── crates/
│   ├── wasm-dbms/                  # Generic WASM DBMS crates
│   │   ├── wasm-dbms-api/          # Shared types and traits
│   │   ├── wasm-dbms-memory/       # Memory abstraction and page management
│   │   ├── wasm-dbms/              # Core DBMS engine
│   │   ├── wasm-dbms-sql/          # SQL front-end (parser, planner, executor)
│   │   └── wasm-dbms-macros/       # Procedural macros (Encode, Table, CustomDataType, DatabaseSchema)
│   │
│   └── wasi-dbms/                  # WASI-specific crates
│       ├── wasi-dbms-memory/       # File-backed memory provider for WASI runtimes
│       └── wasi-dbms-key-value-memory/ # Draft2 key-value provider with checkpoints
│
└── .artifact/                      # Build outputs (.wasm)
```

### Dependency Graph

```
wasm-dbms-macros <── wasm-dbms-api <── wasm-dbms-memory <── wasm-dbms <── wasm-dbms-sql
```

`wasm-dbms-sql` is optional: nothing depends on it, so programs that do not use
SQL do not link it.

Application layers, such as [ic-dbms](https://github.com/veeso/ic-dbms), depend on these crates from separate repositories.

### Generic Layer (wasm-dbms)

#### wasm-dbms-api

**Purpose:** Runtime-agnostic shared types and traits

**Contents:**

- Data types (`Uint32`, `Text`, `DateTime`, etc.)
- `Value` enum for runtime values
- Filter, Query, and Join types
- `Database` trait
- Sanitizer and Validator traits
- `CustomDataType` trait and `CustomValue`
- Error types (`DbmsError`, `DbmsResult`)

**Dependencies:** Minimal (serde, thiserror).

#### wasm-dbms-memory

**Purpose:** Memory abstraction and page management

**Contents:**

- `MemoryProvider` trait
- `HeapMemoryProvider` (testing)
- `MemoryManager` (page-level operations)
- `SchemaRegistry` (table-to-page mapping)
- `TableRegistry` (record-level operations)

#### wasm-dbms

**Purpose:** Core DBMS engine (runtime-agnostic)

**Contents:**

- `DbmsContext<M>` (owns all mutable state)
- `WasmDbmsDatabase<'ctx, M>` (session-scoped operations)
- Transaction management (overlay pattern)
- Foreign key integrity checks
- JOIN execution engine
- `DatabaseSchema` trait for dynamic dispatch

#### wasm-dbms-sql

**Purpose:** SQL front-end for the DBMS engine

**Contents:**

- Lexer and recursive-descent parser for the [SQL dialect](../reference/sql.md)
- Syntax tree types (`wasm_dbms_sql::ast`)
- Planner that resolves names against the schema, converts literals and
  parameters to column types, and builds `Query` and `Filter` values
- `SqlEngine`, which runs statements through `WasmDbmsDatabase` and
  `DatabaseSchema`, outside a transaction or inside the one whose id is
  passed to `execute`

The result and error types (`SqlResult`, `SqlError`) live in `wasm-dbms-api`
behind its `sql` feature, so that adapters can expose SQL without depending on
the engine.

#### wasm-dbms-macros

**Purpose:** Generic procedural macros

**Macros:**

- `#[derive(Encode)]` - Binary serialization
- `#[derive(Table)]` - Table schema and related types
- `#[derive(CustomDataType)]` - Custom data type bridge
- `#[derive(DatabaseSchema)]` - Generates `DatabaseSchema<M>` trait implementation for schema dispatch

---

## Data Flow

### Insert Operation

```
1. Caller invokes Database::insert::<User>(request)
              │
2. Database session receives the request
              │
3. DBMS layer:
   a. Apply sanitizers to values
   b. Apply validators to values
   c. Check primary key uniqueness
   d. Validate foreign key references
   e. If tx_id: write to transaction overlay
      (index overlay tracks added keys)
      Else: write directly
              │
4. Memory layer:
   a. Encode record to bytes
   b. Find space (free segment or new page)
   c. Write to memory
   d. Update all indexes with the new
      key → RecordAddress mapping
              │
5. Return DbmsResult<()>
```

### Select Operation

```
1. Caller invokes Database::select::<User>(query)
              │
2. Database session receives the query
              │
3. DBMS layer:
   a. Parse filters
   b. Analyze filter for index plan
      (equality, range, or IN on indexed column)
   c. If index plan found:
      - Use IndexReader to get RecordAddresses
      - Load only matching records
      - Apply remaining filter as residual
   d. If no index plan (fallback):
      - Full table scan across all pages
   e. If tx_id: merge with overlay
   f. Apply ordering
   g. Apply limit/offset
   h. Select requested columns
   i. Handle eager loading
              │
4. Memory layer:
   a. Read pages
   b. Decode records
              │
5. Return DbmsResult<Vec<Record>>
```

### Select with Join

```text
1. Caller invokes Database::select_join(table, query_with_joins)
              │
2. Database session checks query.has_joins()
              │ (true)
3. JoinEngine:
   a. Read all rows from FROM table
   b. For each JOIN clause:
      - Read rows from joined table (left keys pushed down when the join column
        is indexed)
      - Resolve column references
      - Execute hash join over row indices
   c. Apply filter on combined rows
   d. Apply ordering
   e. Apply offset/limit
   f. Build the column list once and materialize the rows
              │
4. Return DbmsResult<JoinResultSet>
```

See [Join Engine](./join-engine.md) for implementation details.

### Transaction Flow

```
DbmsContext::begin_transaction():
  1. Generate transaction ID
  2. Create empty overlay
  3. Return transaction ID

Operation within a transaction (WasmDbmsDatabase::from_transaction):
  1. Read from: overlay first, then committed
  2. Write to: overlay only

commit():
  1. For each change in overlay:
     - Write to committed data
  2. Delete overlay
  3. Transaction ID becomes invalid

rollback():
  1. Delete overlay (discard all changes)
  2. Transaction ID becomes invalid
```

---

## Extension Points

wasm-dbms provides several extension points for customization:

### Custom Sanitizers

Implement the `Sanitize` trait:

```rust
pub trait Sanitize {
    fn sanitize(&self, value: Value) -> DbmsResult<Value>;
}
```

### Custom Validators

Implement the `Validate` trait:

```rust
pub trait Validate {
    fn validate(&self, value: &Value) -> DbmsResult<()>;
}
```

### Custom Data Types

Define custom data types with the `CustomDataType` derive macro:

```rust
#[derive(Encode, CustomDataType, Clone, Debug, PartialEq, Eq)]
#[type_tag = "status"]
pub enum Status {
    Active,
    Inactive,
}
```

### Memory Provider

Implement `MemoryProvider` for custom memory backends:

```rust
pub trait MemoryProvider {
    const PAGE_SIZE: u64;
    fn size(&self) -> u64;
    fn pages(&self) -> u64;
    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64>;
    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()>;
    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()>;
    fn flush(&mut self) -> MemoryResult<()>;
}
```

Built-in providers:

- `WasiMemoryProvider` - Uses a single flat file (WASI production)
- `WasiKeyValueMemoryProvider` - Uses a draft2 key-value bucket and explicit checkpoints
- `HeapMemoryProvider` - Uses heap memory (testing)

Other runtimes ship their own provider. For the Internet Computer, see [ic-dbms](https://ic.wasm-dbms.cc).

# Architecture

- [Overview](#overview)
- [Three-Layer Architecture](#three-layer-architecture)
  - [Layer 1: Memory Layer](#layer-1-memory-layer)
  - [Layer 2: DBMS Layer](#layer-2-dbms-layer)
  - [Layer 3: API Layer](#layer-3-api-layer)
- [Crate Organization](#crate-organization)
  - [Generic Layer (wasm-dbms)](#generic-layer-wasm-dbms)
  - [IC Layer (ic-dbms)](#ic-layer-ic-dbms)
- [Data Flow](#data-flow)
  - [Insert Operation](#insert-operation)
  - [Select Operation](#select-operation)
  - [Select with Join](#select-with-join)
  - [Transaction Flow](#transaction-flow)
- [Extension Points](#extension-points)

---

## Overview

ic-dbms is built as a layered architecture where each layer has specific responsibilities and builds upon the layer below. The core DBMS engine is runtime-agnostic (`wasm-dbms-*` crates), while the IC-specific adapter layer (`ic-dbms-*` crates) provides Internet Computer integration.

This design provides:

- **Separation of concerns**: Each layer focuses on one aspect
- **Testability**: Layers can be tested independently
- **Portability**: The generic layer runs on any WASM runtime (Wasmtime, Wasmer, WasmEdge)
- **Flexibility**: Internal implementations can change without affecting APIs

---

## Three-Layer Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Layer 3: API Layer                       │
│  Canister endpoints, Candid interface, access control        │
│  (DbmsCanister macro, ACL guards, request/response types)    │
├─────────────────────────────────────────────────────────────┤
│                     Layer 2: DBMS Layer                      │
│  Tables, CRUD operations, transactions, foreign keys         │
│  (TableRegistry, TransactionManager, query execution)        │
├─────────────────────────────────────────────────────────────┤
│                    Layer 1: Memory Layer                     │
│  Stable memory management, encoding/decoding, page allocation│
│  (MemoryProvider, MemoryManager, Encode trait)               │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │  IC Stable      │
                    │    Memory       │
                    │   (or Heap)     │
                    └─────────────────┘
```

### Layer 1: Memory Layer

**Crate:** `wasm-dbms-memory`

**Responsibilities:**
- Manage stable memory allocation (64 KiB pages)
- Encode/decode data to/from binary format
- Track free space and handle fragmentation
- Provide abstraction for testing (heap vs stable memory)

**Key components:**

| Component | Purpose |
|-----------|---------|
| `MemoryProvider` | Abstract interface for memory access |
| `MemoryManager` | Allocates and manages pages |
| `Encode` trait | Binary serialization for all stored types |
| `PageLedger` | Tracks which pages belong to which table |
| `FreeSegmentsLedger` | Tracks free space for reuse |

**Memory layout:**

```
Page 0: Schema Registry (table → page mapping)
Page 1: ACL (allowed principals)
Page 2+: Table data (Page Ledger, Free Segments, Records)
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

| Component | Purpose |
|-----------|---------|
| `DbmsContext<M>` | Owns all DBMS state (memory, schema, ACL, transactions) |
| `WasmDbmsDatabase` | Session-scoped DBMS operations |
| `TableRegistry` | Manages records for a single table |
| `TransactionSession` | Handles transaction lifecycle |
| `Transaction` | Overlay for uncommitted changes |
| `JoinEngine` | Executes cross-table join queries |

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
- Commit merges overlay to committed data
- Rollback discards the overlay

### Layer 3: API Layer

**Crate:** `ic-dbms-canister` (IC-specific)

**Responsibilities:**
- Expose Candid interface
- Handle request/response encoding
- Enforce access control (ACL)
- Route requests to DBMS layer
- Generate table-specific endpoints

**Key components:**

| Component | Purpose |
|-----------|---------|
| `DbmsCanister` macro | Generates canister API from schema |
| ACL guard | Checks caller authorization |
| Request types | `InsertRequest`, `UpdateRequest`, `Query` |
| Response types | `Record`, error handling |

**Generated API structure:**

```rust
// For each table "users":
insert_users(UserInsertRequest, Option<TxId>) -> Result<()>
select_users(Query, Option<TxId>) -> Result<Vec<UserRecord>>
update_users(UserUpdateRequest, Option<TxId>) -> Result<u64>
delete_users(DeleteBehavior, Option<Filter>, Option<TxId>) -> Result<u64>

// Untyped select (supports joins):
select(table: String, Query, Option<TxId>) -> Result<Vec<Vec<(CandidColumnDef, Value)>>>

// Global operations:
begin_transaction() -> TxId
commit(TxId) -> Result<()>
rollback(TxId) -> Result<()>
acl_add_principal(Principal) -> Result<()>
acl_remove_principal(Principal) -> Result<()>
acl_allowed_principals() -> Vec<Principal>
```

---

## Crate Organization

```
ic-dbms/
├── crates/
│   ├── wasm-dbms/                  # Generic WASM DBMS crates
│   │   ├── wasm-dbms-api/          # Shared types and traits
│   │   ├── wasm-dbms-memory/       # Memory abstraction and page management
│   │   ├── wasm-dbms/              # Core DBMS engine
│   │   └── wasm-dbms-macros/       # Procedural macros (Encode, Table, CustomDataType, DatabaseSchema)
│   │
│   └── ic-dbms/                    # IC-specific crates
│       ├── ic-dbms-api/            # IC-specific types (re-exports wasm-dbms-api)
│       ├── ic-dbms-canister/       # Core IC canister implementation
│       ├── ic-dbms-macros/         # IC-specific macros (DbmsCanister)
│       ├── ic-dbms-client/         # Client libraries
│       ├── example/                # Reference implementation
│       └── integration-tests/      # PocketIC integration tests
│
└── .artifact/                      # Build outputs (.wasm, .did, .wasm.gz)
```

### Dependency Graph

```
wasm-dbms-macros <── wasm-dbms-api <── wasm-dbms-memory <── wasm-dbms
                                                                 ^
ic-dbms-macros <── ic-dbms-canister ─────────────────────────────┘
                        ^
                   ic-dbms-client
```

### Generic Layer (wasm-dbms)

#### wasm-dbms-api

**Purpose:** Runtime-agnostic shared types and traits

**Contents:**
- Data types (`Uint32`, `Text`, `DateTime`, etc.)
- `Value` enum for runtime values
- Filter, Query, and Join types
- `Database` trait
- `CallerContext` trait for identity abstraction
- Sanitizer and Validator traits
- `CustomDataType` trait and `CustomValue`
- Error types (`DbmsError`, `DbmsResult`)

**Dependencies:** Minimal (serde, thiserror). Candid support via optional `candid` feature.

#### wasm-dbms-memory

**Purpose:** Memory abstraction and page management

**Contents:**
- `MemoryProvider` trait
- `HeapMemoryProvider` (testing)
- `MemoryManager` (page-level operations)
- `SchemaRegistry` (table-to-page mapping)
- `AccessControl` trait (identity-based ACL abstraction)
- `AccessControlList` (default `AccessControl` impl with `Vec<u8>` identity)
- `NoAccessControl` (no-op ACL for runtimes that don't need access control)
- `TableRegistry` (record-level operations)

#### wasm-dbms

**Purpose:** Core DBMS engine (runtime-agnostic)

**Contents:**
- `DbmsContext<M, A = AccessControlList>` (owns all mutable state)
- `WasmDbmsDatabase<'ctx, M, A>` (session-scoped operations)
- Transaction management (overlay pattern)
- Foreign key integrity checks
- JOIN execution engine
- `DatabaseSchema` trait for dynamic dispatch

#### wasm-dbms-macros

**Purpose:** Generic procedural macros

**Macros:**
- `#[derive(Encode)]` - Binary serialization
- `#[derive(Table)]` - Table schema and related types
- `#[derive(CustomDataType)]` - Custom data type bridge
- `#[derive(DatabaseSchema)]` - Generates `DatabaseSchema<M>` trait implementation for schema dispatch

### IC Layer (ic-dbms)

#### ic-dbms-api

**Purpose:** IC-specific types, re-exports generic API

**Contents:**
- Re-exports all types from `wasm-dbms-api`
- `Principal` custom data type (wraps `candid::Principal`)
- `IcDbmsCanisterArgs` init/upgrade arguments
- `IcDbmsError` / `IcDbmsResult` type aliases

#### ic-dbms-canister

**Purpose:** Thin IC adapter over `wasm-dbms`

**Contents:**
- `IcMemoryProvider` (IC stable memory)
- `DBMS_CONTEXT` thread-local wrapping `DbmsContext<IcMemoryProvider>`
- Canister API layer with ACL guards

**Dependencies:** ic-dbms-api, ic-dbms-macros, wasm-dbms, wasm-dbms-memory, ic-cdk

#### ic-dbms-macros

**Purpose:** IC-specific code generation

**Macros:**
- `#[derive(DatabaseSchema)]` - Generates `DatabaseSchema<M>` trait implementation (IC-specific paths)
- `#[derive(DbmsCanister)]` - Generates complete canister API

#### ic-dbms-client

**Purpose:** Client libraries for canister interaction

**Implementations:**
- `IcDbmsCanisterClient` - Inter-canister calls
- `IcDbmsAgentClient` - External via ic-agent (feature-gated)
- `IcDbmsPocketIcClient` - Testing with PocketIC (feature-gated)

---

## Data Flow

### Insert Operation

```
1. Client calls insert_users(request, tx_id)
              │
2. ACL guard checks caller authorization
              │
3. API layer deserializes request
              │
4. DBMS layer:
   a. Apply sanitizers to values
   b. Apply validators to values
   c. Check primary key uniqueness
   d. Validate foreign key references
   e. If tx_id: write to transaction overlay
      Else: write directly
              │
5. Memory layer:
   a. Encode record to bytes
   b. Find space (free segment or new page)
   c. Write to stable memory
              │
6. Return Result<()>
```

### Select Operation

```
1. Client calls select_users(query, tx_id)
              │
2. ACL guard checks caller authorization
              │
3. API layer deserializes query
              │
4. DBMS layer:
   a. Parse filters
   b. Determine pages to scan
   c. For each page:
      - Read records from memory
      - If tx_id: merge with overlay
      - Apply filters
      - Apply ordering
   d. Apply limit/offset
   e. Select requested columns
   f. Handle eager loading
              │
5. Memory layer:
   a. Read pages
   b. Decode records
              │
6. Return Result<Vec<Record>>
```

### Select with Join

```
1. Client calls select(table, query_with_joins, tx_id)
              │
2. ACL guard checks caller authorization
              │
3. API layer checks query.has_joins()
              │ (true)
4. JoinEngine:
   a. Read all rows from FROM table
   b. For each JOIN clause:
      - Read all rows from joined table
      - Resolve column references
      - Execute nested-loop join
   c. Apply filter on combined rows
   d. Apply ordering
   e. Apply offset/limit
   f. Flatten to output with CandidColumnDef
              │
5. Return Result<Vec<Vec<(CandidColumnDef, Value)>>>
```

See [Join Engine](./join-engine.md) for implementation details.

### Transaction Flow

```
begin_transaction():
  1. Generate transaction ID
  2. Create empty overlay
  3. Record owner (caller identity)
  4. Return transaction ID

Operation with tx_id:
  1. Verify caller owns transaction
  2. Read from: overlay first, then committed
  3. Write to: overlay only

commit(tx_id):
  1. Verify caller owns transaction
  2. For each change in overlay:
     - Write to committed data (stable memory)
  3. Delete overlay
  4. Transaction ID becomes invalid

rollback(tx_id):
  1. Verify caller owns transaction
  2. Delete overlay (discard all changes)
  3. Transaction ID becomes invalid
```

---

## Extension Points

ic-dbms provides several extension points for customization:

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
    fn read(&self, offset: u64, buf: &mut [u8]) -> MemoryResult<()>;
    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()>;
}
```

Built-in providers:
- `IcMemoryProvider` - Uses IC stable memory (production)
- `HeapMemoryProvider` - Uses heap memory (testing)

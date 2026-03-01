# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Context

When working on this project, prioritize code quality, maintainability, test coverage, and adherence to Rust best
practices.

The project is organized as a two-layer architecture:

- **wasm-dbms** (generic layer): Runtime-agnostic DBMS engine that runs on any WASM runtime
- **ic-dbms** (IC layer): Thin adapter providing Internet Computer canister integration on top of wasm-dbms

Each layer internally follows a 3-layer system:

1. Memory layer: stable memory management and low-level data encoding/decoding.
2. DBMS layer: core database functionality (tables, CRUD operations, transactions).
3. API layer: exposes operations (canister API for IC, `Database` trait for generic).

## Project Overview

wasm-dbms is a Rust framework for building relational databases on any WASM runtime.
Developers define database schemas using Rust structs with derive macros, and the framework provides
CRUD operations, ACID transactions, foreign key integrity, and validation/sanitization.

The IC adapter (ic-dbms) adds Candid serialization, ACL-based access control, canister lifecycle
management, and client libraries for Internet Computer deployment.

## Common Commands

```bash
# Build all canisters (compiles to wasm32, shrinks WASM, extracts Candid)
just build_all

# Run unit tests
just test                    # All unit tests
just test <test_name>        # Specific test

# Run integration tests (uses PocketIC)
just integration_test                # All integration tests
just integration_test <pattern>      # Specific tests

# Run all tests
just test_all

# Code quality (CI uses these)
just check_code              # Format check + clippy with -D warnings
just fmt_nightly             # Format with nightly rustfmt
just clippy                  # Run clippy

# Clean build artifacts
just clean
```

## Architecture

### Workspace Structure

```
crates/
├── wasm-dbms/                  # Generic WASM DBMS crates
│   ├── wasm-dbms-api/          # Shared types, traits, validators, sanitizers
│   ├── wasm-dbms-memory/       # Memory abstraction and page management
│   ├── wasm-dbms/              # Core DBMS engine (transactions, joins, integrity)
│   └── wasm-dbms-macros/       # Procedural macros: Encode, Table, CustomDataType, DatabaseSchema
│
└── ic-dbms/                    # IC-specific crates
    ├── ic-dbms-api/            # IC types (re-exports wasm-dbms-api)
    ├── ic-dbms-canister/       # IC canister DBMS implementation
    ├── ic-dbms-macros/         # IC-specific macros: DatabaseSchema, DbmsCanister
    ├── ic-dbms-client/         # Client libraries for canister interaction
    ├── example/                # Reference implementation
    └── integration-tests/      # PocketIC integration tests
```

### Dependency Graph

```
wasm-dbms-macros <── wasm-dbms-api <── wasm-dbms-memory <── wasm-dbms
                                                                 ^
ic-dbms-macros <── ic-dbms-canister ─────────────────────────────┘
                        ^
                   ic-dbms-client
```

### Macro System (Four-Tier)

1. **`#[derive(Encode)]`** (wasm-dbms-macros): Binary serialization for memory storage
2. **`#[derive(Table)]`** (wasm-dbms-macros): Generates `TableSchema`, `*Record`, `*InsertRequest`, `*UpdateRequest`,
   `*ForeignFetcher`
3. **`#[derive(DatabaseSchema)]`** (wasm-dbms-macros / ic-dbms-macros): Generates `DatabaseSchema<M, A>` trait
   implementation for schema dispatch. Two variants exist: the generic one in wasm-dbms-macros (uses `::wasm_dbms::`
   paths) and the IC-specific one in ic-dbms-macros (uses `::ic_dbms_canister::prelude::` paths).
4. **`#[derive(DbmsCanister)]`** (ic-dbms-macros): Generates complete IC canister API with all CRUD operations

### Memory Model

Uses 64 KiB pages in stable memory:

- Schema Registry (1 page)
- ACL Table (1 page)
- Per-table: Page Ledger + Free Segments Ledger + Record Pages

The `MemoryProvider` trait abstracts memory access for testability (heap-based in tests, stable memory in production).

### Transaction Model

- ACID transactions with commit/rollback via overlay pattern
- Per-caller transaction ownership
- Optional transaction ID parameter on all CRUD operations

## Key Patterns

### Generic (wasm-dbms) Table

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(20))]
    pub name: Text,
}
```

Required derives: `Table`, `Clone`

### IC Canister Table

```rust
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}
```

Required derives: `Table`, `CandidType`, `Deserialize`, `Clone`

### Creating an IC Canister

```rust
#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct IcDbmsCanisterGenerator;

ic_cdk::export_candid!();
```

## Documentation Structure

```
docs/
├── index.md                   # wasm-dbms landing page
├── guides/                    # Generic wasm-dbms guides
│   ├── get-started.md         # Generic getting started (Database trait)
│   ├── crud-operations.md     # Generic CRUD
│   ├── querying.md            # Filters, ordering, pagination, joins
│   ├── transactions.md        # ACID transactions
│   ├── relationships.md       # Foreign keys and eager loading
│   └── custom-data-types.md   # Custom data types
├── reference/                 # Generic reference
│   ├── data-types.md, schema.md, validation.md, sanitization.md, json.md, errors.md
├── technical/                 # Architecture and internals
│   ├── architecture.md, memory.md, join-engine.md
└── ic/                        # IC-specific docs
    ├── index.md               # IC integration overview
    ├── guides/                # IC-specific guides
    │   ├── get-started.md     # IC canister setup/deploy
    │   ├── crud-operations.md # CRUD via ic-dbms-client
    │   ├── access-control.md  # ACL management
    │   └── client-api.md      # Client library usage
    └── reference/             # IC-specific reference
        ├── schema.md          # DbmsCanister macro, Candid API
        ├── data-types.md      # Principal type, Candid mappings
        └── errors.md          # IcDbmsError, double-Result pattern
```

## Build Requirements

- Rust 1.91.1+ (Edition 2024)
- `wasm32-unknown-unknown` target
- Tools: `ic-wasm`, `candid-extractor`, `just`
- CI uses nightly for formatting

## Conventions

- Uses Conventional Commits
- Output artifacts go to `.artifact/` (`.wasm`, `.did`, `.wasm.gz`)
- Always update the CHANGELOG.md with significant changes
- Design docs and plans go in `.claude/plans/`, never in `docs/plans/`

# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Context

When working on this project, prioritize code quality, maintainability, test coverage, and adherence to Rust best
practices.

Keep in mind that the ic-dbms-canister is divided into a 3-layer system, where each layer builds upon the previous one:

1. Memory layer: takes care of stable memory management and low-level data encoding/decoding.
2. DBMS layer: implements the core database functionality (tables, CRUD operations, transactions).
3. API layer: exposes the canister API with all operations.

## Project Overview

IC DBMS is a Rust framework for building database canisters on the Internet Computer (IC).
Developers define database schemas using Rust structs with derive macros, and the framework generates a complete
canister with CRUD operations, transactions, and ACL-based access control.

It provides all the operations needed for a relational database, including:

- Create, Read, Update, Delete (CRUD) operations
- ACID transactions with commit/rollback
- Access control lists (ACLs) for table-level permissions
- Memory management optimized for IC stable memory

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

- **ic-dbms-api**: Shared types (data types, traits, validators, sanitizers, memory encoding)
- **ic-dbms-canister**: Core DBMS engine (table management, transactions, memory management)
- **ic-dbms-macros**: Procedural macros (`#[derive(Encode)]`, `#[derive(Table)]`, `#[derive(DbmsCanister)]`)
- **ic-dbms-client**: Client library for canister interaction
- **example/**: Reference implementation showing how to define a schema
- **integration-tests/pocket-ic-tests/**: Integration tests using PocketIC

### Macro System (Three-Tier)

1. **`#[derive(Encode)]`**: Auto-implements binary serialization for memory storage
2. **`#[derive(Table)]`**: Generates `TableSchema`, `*Record`, `*InsertRequest`, `*UpdateRequest`, `*ForeignFetcher`
   types
3. **`#[derive(DbmsCanister)]`**: Generates complete canister API with all CRUD operations

### Memory Model

Uses 64 KiB pages in IC stable memory:

- Schema Registry (1 page)
- ACL Table (1 page)
- Per-table: Page Ledger + Free Segments Ledger + Record Pages

The `MemoryProvider` trait abstracts memory access for testability (heap-based in tests, stable memory in production).

### Transaction Model

- ACID transactions with commit/rollback via overlay pattern
- Per-principal transaction ownership
- Optional transaction ID parameter on all CRUD operations

## Key Patterns

### Defining a Table

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(20))]
    pub name: Text,
    #[foreign_key(entity = "Post", table = "posts", column = "user")]
    pub id: Uint32,
}
```

Required derives: `Table`, `CandidType`, `Deserialize`, `Clone`

### Creating a Canister

```rust
#[derive(DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct IcDbmsCanisterGenerator;

ic_cdk::export_candid!();
```

## Build Requirements

- Rust 1.85.1+ (Edition 2024)
- `wasm32-unknown-unknown` target
- Tools: `ic-wasm`, `candid-extractor`, `just`
- CI uses nightly for formatting

## Conventions

- Uses Conventional Commits
- Output artifacts go to `.artifact/` (`.wasm`, `.did`, `.wasm.gz`)
- Always update the CHANGELOG.md with significant changes
- Design docs and plans go in `.claude/plans/`, never in `docs/plans/`

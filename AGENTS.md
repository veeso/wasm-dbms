# AGENTS.md

This file is the shared repository contract for contributors and coding agents.
Prioritize correctness, maintainability, test coverage, and idiomatic Rust.

## Project Context

The project has two layers:

- **wasm-dbms**: a runtime-agnostic DBMS engine that runs on any WASM runtime.
- **ic-dbms**: a thin Internet Computer adapter built on wasm-dbms.

Each layer follows three internal layers:

1. Memory: stable-memory management and low-level encoding/decoding.
2. DBMS: tables, CRUD operations, transactions, and integrity checks.
3. API: the generic `Database` trait or the IC canister API.

## Project Overview

wasm-dbms is a Rust framework for building relational databases on WASM
runtimes. Developers define schemas with Rust structs and derive macros; the
framework provides CRUD operations, ACID transactions, foreign-key integrity,
and validation/sanitization.

The IC adapter adds Candid serialization, ACL-based access control, canister
lifecycle management, and client libraries for Internet Computer deployment.

## Common Commands

Use `just --list` for the complete interface. The main quality gate is:

```sh
just check
```

Useful focused commands are:

```sh
# Build all workspace targets, including canisters and the WASI example.
just build_all

# Run unit, doc, integration, and WIT example tests.
just test_all

# Run the generic engine's fast test and build paths.
just test_wasm_dbms
just build_wasm_dbms
just test_wasm_dbms_example

# Format and inspect the repository.
just fmt
just fmt_check
just clippy "-- -D warnings"
just doc
just deny
just zizmor

# Security and release helpers.
just scan_secrets .
just changelog_preview 0.10.0
just package_list "--help"
```

`just fmt` uses dprint for Rust, Markdown, TOML, and YAML. Rust files are
delegated to the pinned nightly rustfmt command configured in `dprint.json`.
Run `just fmt` after changing Rust code and `just fmt_check` before submitting
changes.

## Architecture

### Workspace Structure

```text
crates/
├── wasm-dbms/                  # Generic WASM DBMS crates
│   ├── wasm-dbms-api/          # Shared types, traits, validators, sanitizers
│   ├── wasm-dbms-memory/       # Memory abstraction and page management
│   ├── wasm-dbms/              # Core DBMS engine
│   └── wasm-dbms-macros/       # Encode, Table, CustomDataType, DatabaseSchema
│
└── ic-dbms/                    # IC-specific crates
    ├── ic-dbms-api/            # IC types and wasm-dbms-api re-exports
    ├── ic-dbms-canister/       # IC canister DBMS implementation
    ├── ic-dbms-macros/         # DbmsCanister procedural macro
    ├── ic-dbms-client/         # Canister client libraries
    ├── example/                # Reference implementation
    └── integration-tests/      # PocketIC integration tests
```

### Dependency Graph

```text
wasm-dbms-macros <── wasm-dbms-api <── wasm-dbms-memory <── wasm-dbms
                                                                 ^
ic-dbms-macros <── ic-dbms-canister ─────────────────────────────┘
                        ^
                   ic-dbms-client
```

### Macro System

1. `#[derive(Encode)]` provides binary serialization for memory storage.
2. `#[derive(Table)]` generates `TableSchema`, record, request, and foreign
   fetcher types.
3. `#[derive(DatabaseSchema)]` generates the `DatabaseSchema<M, A>` dispatch
   implementation and is re-exported through `ic-dbms-canister::prelude`.
4. `#[derive(DbmsCanister)]` generates the complete IC canister API and CRUD
   endpoints.

### Memory Model

Stable memory uses 64 KiB pages:

- Schema Registry: one page.
- ACL Table: one page.
- Each table: a page ledger, free-segments ledger, and record pages.

The `MemoryProvider` trait abstracts memory access for testability. Tests use a
heap-backed provider; production uses stable memory.

### Transaction Model

- ACID transactions use an overlay pattern for commit and rollback.
- Transactions are owned by the caller.
- CRUD operations accept an optional transaction ID.

### Key Patterns

Generic tables use the `Table` derive and can add sanitizers and validators:

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

IC tables additionally derive `CandidType` and `Deserialize`; the `#[candid]`
attribute adds Candid/Serde derives to generated types.

An IC canister schema combines `DatabaseSchema` and `DbmsCanister`, then exports
its Candid interface with `ic_cdk::export_candid!()`.

## Documentation Structure

```text
docs/
├── index.md                   # wasm-dbms landing page
├── guides/                    # Generic guides
│   ├── get-started.md         # Database trait setup
│   ├── crud-operations.md     # Generic CRUD
│   ├── querying.md            # Filters, ordering, pagination, joins
│   ├── transactions.md        # ACID transactions
│   ├── relationships.md       # Foreign keys and eager loading
│   └── custom-data-types.md   # Custom data types
├── reference/                 # Generic data types, schema, validation, errors
├── technical/                 # Architecture and internals
└── ic/                        # IC-specific guides and reference
```

## Build and Tooling Requirements

- The development and CI toolchain is Rust `1.99.0`, pinned by
  [`rust-toolchain.toml`](./rust-toolchain.toml), with `clippy`, `rustfmt`,
  `wasm32-unknown-unknown`, and `wasm32-wasip2`.
- Crate manifests continue to advertise Rust `1.91.1` as the published MSRV.
- `just` is the stable command interface.
- dprint formats Rust, Markdown, TOML, and YAML; the configured nightly
  rustfmt formats Rust source files.
- `cargo-deny` rejects vulnerabilities, unsoundness, yanked crates,
  unapproved licenses, wildcard dependencies, and unapproved sources. An
  unavoidable unmaintained transitive crate may remain a warning.
- git-cliff generates release notes from Conventional Commit history. Use
  `just changelog_preview <version>` to inspect them and `just changelog
  <version>` to generate a versioned entry.
- GitHub Actions use least-privilege permissions, disabled persisted checkout
  credentials, and verified full-SHA action pins. Run `just zizmor` after
  workflow changes.
- Keep dependency versions and manifest fields compatible with the repository's
  Cargo conventions; prefer workspace dependencies where they are defined.

## Conventions

- Use Conventional Commits. Do not add agent-attribution lines to commits.
- Put generated build artifacts in `.artifact/` and do not commit them.
- Keep plans and progress files under `.superpowers/`; these files are local
  working material and must not be committed.
- When adding documentation under `docs/`, register it in
  [`docs/SUMMARY.md`](./docs/SUMMARY.md) under the matching section.
- Read [`CONTRIBUTING.md`](./CONTRIBUTING.md) and [`AI_POLICY.md`](./AI_POLICY.md)
  before contributing through an AI-assisted workflow.

## Database API Surface (Keep in Sync)

When changing the `Database` trait
(`crates/wasm-dbms/wasm-dbms-api/src/dbms/database.rs`) or the
`DatabaseSchema` dispatch trait (`crates/wasm-dbms/wasm-dbms/src/schema.rs`) —
adding, removing, or renaming methods; changing signatures; adding error
variants; or extending `Query`, `Filter`, or `Value` — propagate the change to
every consumer below in the same PR. These surfaces have silently drifted in
the past; treat this as a checklist:

- `wit/dbms.wit`: update the WIT records, variants, and database interface,
  rebuild `wasm-dbms-example-guest` and `wasm-dbms-example-host`, and run the
  host demo end to end.
- `crates/ic-dbms/ic-dbms-canister/src/api.rs`: generic per-operation helpers
  consumed by macro-generated canister endpoints.
- `crates/ic-dbms/ic-dbms-macros/src/dbms_canister.rs`: the
  `#[derive(DbmsCanister)]` macro, including per-table endpoints such as
  `select_<table>`, `aggregate_<table>`, `insert_<table>`, shared select,
  transaction, and ACL endpoints.
- `crates/ic-dbms/ic-dbms-client/src/client.rs`: the `Client` trait and all
  implementations:
  - `client/ic.rs`: canister-to-canister calls via `ic-cdk`.
  - `client/agent.rs`: external calls via `ic-agent`, behind the `ic-agent`
    feature.
  - `client/pocket_ic.rs`: integration tests, behind the `pocket-ic` feature.
- `crates/ic-dbms/integration-tests/dbms-canister-client-integration/src/lib.rs`:
  the wrapper canister exposing the client method end to end.
- `crates/ic-dbms/integration-tests/pocket-ic-tests/tests/`: coverage through
  both the direct client and wrapper canister; register each test file in
  `tests/integration_tests.rs`.

Documentation that must follow the same change:

- `docs/reference/query.md` and `docs/reference/errors.md` for builder methods
  or error variants.
- `docs/ic/reference/schema.md` for generated Candid endpoints.
- `docs/ic/guides/client-api.md` for `Client` trait methods.
- `docs/guides/querying.md` and `docs/guides/crud-operations.md` for
  caller-facing behavior changes.

When in doubt, search for the old method name across the workspace before
finishing; every match needs an update or an explicit deletion.

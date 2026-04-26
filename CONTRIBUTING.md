# Contributing to wasm-dbms

Thank you for your interest in contributing to **wasm-dbms**! This document describes how to set up a local development
environment, the workflow used for changes, and the conventions every contribution must follow.

By participating in this project you agree to abide by the [Code of Conduct](./CODE_OF_CONDUCT.md).

---

- [Contributing to wasm-dbms](#contributing-to-wasm-dbms)
  - [Ways to Contribute](#ways-to-contribute)
  - [Repository Layout](#repository-layout)
  - [Development Environment](#development-environment)
    - [Required Tooling](#required-tooling)
    - [Optional Tooling (IC work only)](#optional-tooling-ic-work-only)
    - [First-Time Setup](#first-time-setup)
  - [Working on `wasm-dbms` Only (no IC toolchain)](#working-on-wasm-dbms-only-no-ic-toolchain)
  - [Working on `ic-dbms`](#working-on-ic-dbms)
  - [Common Commands](#common-commands)
  - [Workflow](#workflow)
  - [Conventions](#conventions)
    - [Code Style](#code-style)
    - [Commit Messages](#commit-messages)
    - [Branches](#branches)
    - [Pull Requests](#pull-requests)
    - [Documentation](#documentation)
    - [Changelog](#changelog)
    - [Database API Surface](#database-api-surface)
  - [Testing Guidelines](#testing-guidelines)
  - [Reporting Bugs and Requesting Features](#reporting-bugs-and-requesting-features)
  - [Security Issues](#security-issues)
  - [License](#license)

---

## Ways to Contribute

- Fixing bugs and reporting reproducible issues.
- Improving documentation under `docs/` (rendered at <https://wasm-dbms.cc>).
- Adding test coverage to existing crates.
- Implementing new features (please open an issue first to discuss the design).
- Reviewing open pull requests.

If you are unsure whether a change is welcome, open a GitHub issue and ask before investing time in a large patch.

## Repository Layout

The workspace is split into two crate families. Detailed architecture lives in
[`docs/technical/architecture.md`](./docs/technical/architecture.md); the short version:

| Path                    | Purpose                                                                |
|-------------------------|------------------------------------------------------------------------|
| `crates/wasm-dbms/`     | Runtime-agnostic DBMS engine and procedural macros (no IC dependency). |
| `crates/wasi-dbms/`     | Memory providers and examples for WASI runtimes (Wasmtime, etc.).      |
| `crates/ic-dbms/`       | Internet Computer adapter, client libraries, and integration tests.    |
| `wit/`                  | WIT interface definitions for the WASI Component Model bindings.       |
| `docs/`                 | mdBook-style documentation.                                            |
| `just/`                 | Modular `Justfile` recipes (`build`, `test`, `code_check`, `bench`).   |

## Development Environment

### Required Tooling

- **Rust toolchain** pinned by [`rust-toolchain.toml`](./rust-toolchain.toml) (currently `1.94.1`) with the
  `wasm32-unknown-unknown` and `wasm32-wasip2` targets. `rustup` will install both automatically the first time
  you run a `cargo` command in the repository.
- **Nightly Rust** with `rustfmt` (used for formatting only):

  ```sh
  rustup toolchain install nightly --component rustfmt
  ```

- [`just`](https://github.com/casey/just) — task runner used by every workflow in CI.

### Optional Tooling (IC work only)

You only need these when changing crates under `crates/ic-dbms/` or running the IC integration tests locally. If you
work exclusively on the generic `wasm-dbms-*` or `wasi-dbms-*` crates, you can skip them and rely on CI to validate
the IC side.

- [`ic-wasm`](https://github.com/dfinity/ic-wasm) — shrinks canister WASM artifacts.
- [`candid-extractor`](https://crates.io/crates/candid-extractor) — extracts `.did` files from built canisters.
- [`pocket-ic`](https://github.com/dfinity/pocketic) — local IC replica used by the integration test suite. The
  binary is downloaded automatically by the test harness on first run; ensure your platform is supported.

Install the Cargo-based tools with:

```sh
cargo install ic-wasm candid-extractor
```

### First-Time Setup

```sh
git clone https://github.com/veeso/wasm-dbms.git
cd wasm-dbms

# Verify the toolchain installs correctly and the workspace builds
cargo build --workspace
```

## Working on `wasm-dbms` Only (no IC toolchain)

If your change is confined to the generic engine, macros, or WASI bits, run the focused recipes:

```sh
just test_wasm_dbms              # fast unit tests for wasm-dbms-{api,memory} and wasm-dbms
just build_wasm_dbms             # build generic crates for wasm32-unknown-unknown
just test_wasm_dbms_example      # end-to-end Component Model example (Wasmtime host + guest)
just check_code                  # nightly fmt --check + clippy -D warnings
```

Open the PR once these pass locally; the CI job covers the IC build and integration tests for you.

## Working on `ic-dbms`

When touching IC crates, run the full suite locally before opening a PR:

```sh
just build_all                   # generic + IC canisters + WASI example
just test_all                    # unit tests + PocketIC integration tests + WIT example
just check_code                  # format + clippy
```

## Common Commands

A non-exhaustive cheat sheet (run `just --list` for everything):

| Command                          | Description                                                        |
|----------------------------------|--------------------------------------------------------------------|
| `just build_all`                 | Builds every crate, canister, and the WASI example.                |
| `just test`                      | Runs all unit + doc tests across the workspace.                    |
| `just test <name>`               | Filters unit tests by substring.                                   |
| `just integration_test [name]`   | Runs the PocketIC integration tests.                               |
| `just test_all`                  | All of the above, plus the WIT host/guest example.                 |
| `just clippy`                    | `cargo clippy --all-features`.                                     |
| `just fmt_nightly`               | Formats the whole workspace with nightly `rustfmt`.                |
| `just check_code`                | What CI runs: `fmt_nightly --check` + `clippy -- -D warnings`.     |
| `just clean`                     | Removes `.artifact/` and `target/` (asks for confirmation).        |

## Workflow

1. **Open an issue first** for non-trivial changes so the design can be discussed.
2. Fork the repository and create a topic branch from `main`.
3. Implement your change with tests.
4. Run `just check_code` and the relevant test recipes locally.
5. Update documentation under `docs/` and the `CHANGELOG.md` entry if user-visible behaviour changes (see
   [Changelog](#changelog)).
6. Open a pull request against `main` describing the motivation, the approach, and any follow-ups.

## Conventions

### Code Style

- Format with **nightly** `rustfmt`: `just fmt_nightly`. CI fails on any diff.
- Lint clean under `cargo clippy --all-features -- -D warnings`.
- Prefer `where` clauses over inline trait bounds on generic parameters.
- No `unsafe` without an accompanying `// SAFETY:` comment that justifies the invariants.
- Keep public items documented; the project relies on `docs.rs` for the API reference.
- `Cargo.toml` files follow the in-repo [`cargo-toml`](./Cargo.toml) conventions: alphabetically sorted dependencies,
  workspace-inherited versions where possible.

### Commit Messages

This project uses [Conventional Commits](https://www.conventionalcommits.org/). Examples:

```text
feat(query): add HAVING clause to aggregate queries
fix(memory): handle page boundary in free segment ledger
docs(ic): clarify ACL bootstrap flow
chore(ci): cache cargo registry between jobs
```

The release notes are generated from these prefixes by `git-cliff` (see [`cliff.toml`](./cliff.toml)).

### Branches

- Feature branches: `feat/<issue>-<slug>` (e.g. `feat/34-schema-migrations`).
- Bug-fix branches: `fix/<issue>-<slug>`.
- Documentation-only: `docs/<slug>`.

### Pull Requests

- Keep PRs focused; split unrelated changes into separate PRs.
- Reference the GitHub issue in the PR description (`Closes #N`).
- The PR title should also follow Conventional Commits — it becomes the squash-merge commit message.
- All CI jobs (`lint`, `unit-test`, integration tests, doc tests, bench-build) must be green before review.

### Documentation

User-facing changes must update the relevant pages under `docs/`:

- Generic engine behaviour → `docs/guides/` and `docs/reference/`.
- IC-specific behaviour → `docs/ic/guides/` and `docs/ic/reference/`.
- Architecture and internals → `docs/technical/`.

Design notes and implementation plans live in `.claude/plans/` — never under `docs/plans/`.

### Changelog

Add a bullet under the **Unreleased** section of [`CHANGELOG.md`](./CHANGELOG.md) for any user-visible change
(new feature, bug fix, breaking change, deprecation). Internal refactors that do not change behaviour can be
omitted.

### Database API Surface

The `Database` trait and `DatabaseSchema` dispatch trait are mirrored across several consumers. When you
change either of them — adding/removing methods, changing signatures, adding error variants, or extending
`Query`/`Filter`/`Value` — update **every** surface below in the same PR:

- `wit/dbms.wit` — WIT interface for the WASI guest.
- `crates/ic-dbms/ic-dbms-canister/src/api.rs` — generic helpers used by the canister macro.
- `crates/ic-dbms/ic-dbms-macros/src/dbms_canister.rs` — the `#[derive(DbmsCanister)]` endpoint generator.
- `crates/ic-dbms/ic-dbms-client/src/client.rs` and the three implementations under `client/`
  (`ic.rs`, `agent.rs`, `pocket_ic.rs`).
- `crates/ic-dbms/integration-tests/dbms-canister-client-integration/src/lib.rs` — wrapper canister.
- `crates/ic-dbms/integration-tests/pocket-ic-tests/tests/` — coverage for both the direct client and the
  wrapper canister; remember to register the new test in `tests/integration_tests.rs`.

Documentation that must follow the same change: `docs/reference/query.md`, `docs/reference/errors.md`,
`docs/ic/reference/schema.md`, `docs/ic/guides/client-api.md`, `docs/guides/querying.md`, and
`docs/guides/crud-operations.md`. When in doubt, grep for the old method name across the workspace before
finishing the change.

## Testing Guidelines

- Every public function should have at least one unit test exercising the happy path and the most relevant
  failure modes.
- Use the in-memory `MemoryProvider` for fast unit tests; reach for PocketIC only when you need true canister
  semantics (cycles, inter-canister calls, upgrades).
- Doc tests are part of CI (`cargo test --doc`); keep code samples in `///` blocks compiling.
- Benchmarks live under `crates/wasm-dbms/wasm-dbms/benches/`; CI builds them but does not measure performance.
  Run them locally with `just bench`.

## Reporting Bugs and Requesting Features

Use the issue templates under [`.github/ISSUE_TEMPLATE/`](./.github/ISSUE_TEMPLATE/). For bug reports include:

- Crate version (or commit SHA).
- Minimal reproduction (preferably a failing test).
- Expected vs. actual behaviour.
- Host platform and runtime (native, Wasmtime, IC replica, mainnet …).

## Security Issues

Do **not** open public issues for security vulnerabilities. Email <christian.visintin@veeso.dev> with the
details and a way to reproduce the problem. You will receive an acknowledgement within a few business days.

## License

By contributing you agree that your contributions will be licensed under the [MIT License](./LICENSE) that
covers the project.

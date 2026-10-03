# Contributing to wasm-dbms

Thank you for your interest in contributing to **wasm-dbms**! This document describes how to set up a local development
environment, the workflow used for changes, and the conventions every contribution must follow. AI-assisted
contributions must also follow the [AI policy](./AI_POLICY.md).

By participating in this project you agree to abide by the [Code of Conduct](./CODE_OF_CONDUCT.md).

---

- [Contributing to wasm-dbms](#contributing-to-wasm-dbms)
  - [Ways to Contribute](#ways-to-contribute)
  - [Repository Layout](#repository-layout)
  - [Development Environment](#development-environment)
    - [Required Tooling](#required-tooling)
    - [First-Time Setup](#first-time-setup)
  - [Local Checks](#local-checks)
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

The workspace is split into two crate families. The Internet Computer adapter lives in the separate
[ic-dbms](https://github.com/veeso/ic-dbms) repository. Detailed architecture lives in
[`docs/technical/architecture.md`](./docs/technical/architecture.md); the short version:

| Path                | Purpose                                                                |
| ------------------- | ---------------------------------------------------------------------- |
| `crates/wasm-dbms/` | Runtime-agnostic DBMS engine and procedural macros (no IC dependency). |
| `crates/wasi-dbms/` | Memory providers and examples for WASI runtimes (Wasmtime, etc.).      |
| `wit/`              | WIT interface definitions for the WASI Component Model bindings.       |
| `docs/`             | mdBook-style documentation.                                            |
| `just/`             | Modular `Justfile` recipes for build, test, quality, and release work. |

## Development Environment

### Required Tooling

- **Rust toolchain** pinned by [`rust-toolchain.toml`](./rust-toolchain.toml) (currently `1.99.0`) with the
  `wasm32-unknown-unknown` and `wasm32-wasip2` targets. `rustup` will install both automatically the first time
  you run a `cargo` command in the repository.
- **Nightly Rust** with `rustfmt` (used by dprint for Rust formatting only):

  ```sh
  rustup toolchain install nightly --component rustfmt
  ```

- [`just`](https://github.com/casey/just) — task runner used by every workflow in CI.
- [`dprint`](https://dprint.dev/) — formatter for Rust, Markdown, TOML, and YAML.
- [`cargo-deny`](https://embarkstudios.github.io/cargo-deny/) — dependency policy checks.
- [`git-cliff`](https://git-cliff.org/) — generated changelog tooling.
- [`zizmor`](https://woodruffw.github.io/zizmor/) — GitHub Actions security checks.
- [`TruffleHog`](https://github.com/trufflesecurity/trufflehog) — secret scanning.

### First-Time Setup

```sh
git clone https://github.com/veeso/wasm-dbms.git
cd wasm-dbms

# Verify the toolchain installs correctly and the workspace builds
cargo check --workspace

# Run the repository's normal quality gate
just check
```

## Local Checks

Run the focused recipes while iterating, then the full suite before opening a pull request:

```sh
just test_wasm_dbms              # fast unit tests for wasm-dbms-{api,memory} and wasm-dbms
just build_wasm_dbms             # build generic crates for wasm32-unknown-unknown
just test_wasm_dbms_example      # end-to-end Component Model example (Wasmtime host + guest)
just build_all                   # generic crates + WASI example
just test_all                    # unit tests + WIT example
just check                       # formatting, clippy, docs, dependency policy, and tests
```

## Common Commands

A non-exhaustive cheat sheet (run `just --list` for everything):

| Command                         | Description                                                 |
| ------------------------------- | ----------------------------------------------------------- |
| `just build_all`                | Builds every crate and the WASI example.                    |
| `just test`                     | Runs unit and doc tests across the workspace.               |
| `just test <name>`              | Filters unit tests by substring.                            |
| `just test_all`                 | Runs unit, doc, and WIT example tests.                      |
| `just fmt`                      | Formats supported files with dprint.                        |
| `just fmt_check`                | Checks Rust, Markdown, TOML, and YAML formatting.           |
| `just clippy`                   | Runs workspace Clippy checks.                               |
| `just doc`                      | Builds warning-free workspace documentation.                |
| `just deny`                     | Runs cargo-deny policy checks.                              |
| `just zizmor`                   | Audits GitHub Actions workflows.                            |
| `just scan_secrets .`           | Scans the repository for secrets with TruffleHog.           |
| `just check`                    | Runs the normal local quality gate.                         |
| `just coverage`                 | Runs the workspace coverage command used by CI.             |
| `just changelog_preview 0.10.0` | Previews generated release notes.                           |
| `just package_list "--help"`    | Shows the package-file inspection command.                  |
| `just clean`                    | Removes `.artifact/` and `target/` (asks for confirmation). |

## Workflow

1. Discuss non-trivial design changes before implementation and link the relevant issue in the pull request when one exists.
2. Fork the repository and create a topic branch from `main`.
3. Implement your change with tests.
4. Run `just check` and the relevant build and test recipes locally.
5. Update documentation under `docs/` and preview generated release notes if user-visible behaviour changes (see
   [Changelog](#changelog)).
6. Open a pull request against `main` describing the motivation, the approach, and any follow-ups.

## Conventions

### Code Style

- Format supported files with dprint: `just fmt`; CI fails on any diff from `just fmt_check`.
- Rust formatting is delegated to the configured nightly `rustfmt` command.
- Lint clean under `just clippy "-- -D warnings"`.
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
- All CI jobs (`lint`, `unit-test`, doc tests, bench-build, WIT example) must be green before review.

### Documentation

User-facing changes must update the relevant pages under `docs/`:

- Generic engine behaviour → `docs/guides/` and `docs/reference/`.
- IC-specific behaviour → the [ic-dbms](https://github.com/veeso/ic-dbms) repository.
- Architecture and internals → `docs/technical/`.

Design notes and implementation plans live in `.superpowers/` — never under `docs/plans/`.

### Changelog

User-visible changes are generated from Conventional Commit history. Preview
the next notes with `just changelog_preview <version>` and generate the entry
with `just changelog <version>`. Internal refactors that do not change
behaviour can be omitted from release-note-oriented commit messages.

### Database API Surface

The `Database` trait and `DatabaseSchema` dispatch trait are mirrored across several consumers. When you
change either of them — adding/removing methods, changing signatures, adding error variants, or extending
`Query`/`Filter`/`Value` — update **every** surface below in the same PR:

- `wit/dbms.wit` — WIT interface for the WASI guest.
- The [ic-dbms](https://github.com/veeso/ic-dbms) repository — it consumes these traits through the published crates
  (canister API helpers, the `#[derive(DbmsCanister)]` macro, the `Client` trait and its implementations, and the
  PocketIC integration tests). Mention the required follow-up there in your pull request.

Documentation that must follow the same change: `docs/reference/query.md`, `docs/reference/errors.md`,
`docs/guides/querying.md`, and `docs/guides/crud-operations.md`. When in doubt, grep for the old method name across the workspace before
finishing the change.

## Testing Guidelines

- Every public function should have at least one unit test exercising the happy path and the most relevant
  failure modes.
- Use the in-memory `MemoryProvider` for fast unit tests.
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

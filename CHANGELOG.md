# Changelog

- [Changelog](#changelog)
  - [0.6.0](#060)
  - [0.5.0](#050)
  - [0.4.0](#040)
  - [0.3.0](#030)
  - [0.2.1](#021)
  - [0.2.0](#020)
  - [0.1.0](#010)

## 0.6.0

Released on 2026-03-02

### ⚠ Breaking Changes

- migrate Principal from built-in to CustomDataType
  > Value::Principal and DataTypeKind::Principal removed.
  Principal fields in tables must now use #[custom_type] annotation.
  Existing stable memory schemas are incompatible (fingerprint change).
- restructure workspace into wasm-dbms and ic-dbms layers
  > restructure workspace into wasm-dbms and ic-dbms layers

### Added

- **ic-dbms-api:** add CustomValue struct with comparison and hashing
- **ic-dbms-api:** add CustomDataType trait
- **ic-dbms-api:** add Value::Custom variant and accessors
- **ic-dbms-api:** add DataTypeKind::Custom variant and CandidDataTypeKind
  > Add Custom(&'static str) variant to DataTypeKind for user-defined types.
  > Remove CandidType/Serialize/Deserialize derives from DataTypeKind since
  > it no longer needs to cross API boundaries directly. Introduce
  > CandidDataTypeKind as the Candid-serializable mirror with Custom(String)
  > for the canister API layer. Update CandidColumnDef to use the new type.
- **ic-dbms-macros:** add #[derive(CustomDataType)] macro
  > Add a proc-macro derive that generates `impl CustomDataType` (with
  > TYPE_TAG constant) and `impl From<T> for Value` for user-defined types.
  > The attribute `#[type_tag = "..."]` is required and uses the same
  > NameValue parsing pattern as the existing `#[table = "..."]` attribute.
- **ic-dbms-macros:** add #[custom_type] support to Table derive macro
  > When a field is annotated with #[custom_type], the generated code uses
  > Value::Custom(CustomValue { ... }) instead of Value::FieldType(field)
  > for to_values/from_values in TableSchema, Record, InsertRequest, and
  > UpdateRequest. This allows user-defined types implementing CustomDataType
  > to be used as table columns.
- 💥 migrate Principal from built-in to CustomDataType
- add WIT interface definition for wasm-dbms Component Model API
- add WIT guest crate with FileMemoryProvider and example schemas
  > Create the wasm-dbms-example-guest crate scaffolding with:
  > - FileMemoryProvider: file-backed MemoryProvider implementation with
      >   persistence across process restarts and full test coverage
  > - Example table schemas (User, Post) with ExampleDatabaseSchema
      >   implementing the generic DatabaseSchema<M> trait
  > - register_tables helper for DBMS context initialization
  >
  > Fix wasm-dbms-macros to use DbmsError/DbmsResult instead of
  > IcDbmsError/IcDbmsResult and remove IC-specific candid/serde derives
  > from generated insert, update, and record structs, making the
  > generic macro layer truly runtime-agnostic.
- implement WIT guest bridge layer for Component Model exports
- add Wasmtime host binary for WIT Component Model example
  > Create the host-side binary that loads the guest WASM component via
  > Wasmtime, provides WASI filesystem access, and exercises every exported
  > database operation: insert, select, transactional commit, and rollback.
- add wasm-dbms dependency to ic-dbms-canister
- add #[derive(DatabaseSchema)] macro for automatic schema dispatch
  > Add a DatabaseSchema derive macro that auto-generates the
  > DatabaseSchema<M> trait implementation from a #[tables(...)] attribute,
  > eliminating ~130+ lines of boilerplate per schema. Two variants exist:
  > a generic one in wasm-dbms-macros and an IC-specific one in
  > ic-dbms-macros with IC crate paths. Update examples, tests, and docs.
- add AccessControl trait with associated Id type for runtime-agnostic ACL
  > Introduce the AccessControl trait in wasm-dbms-memory to abstract access
  > control behind a generic interface. Different runtimes can use different
  > identity types: Vec<u8> (AccessControlList), Principal (IcAccessControlList),
  > or () (NoAccessControl). The A: AccessControl generic parameter is propagated
  > through DbmsContext, WasmDbmsDatabase, DatabaseSchema, integrity validators,
  > join engine, and both derive macros. Default type parameters preserve backward
  > compatibility.
- add journaling-based atomicity to MemoryManager
  > Replace panic-based rollback in atomic() with a write-ahead journal in
  > MemoryManager. All writes via write_at and zero are recorded when a
  > journal is active, enabling byte-level rollback on error. This makes
  > atomicity runtime-agnostic, removing the dependency on IC's
  > trap-reverts-stable-memory semantics.
  >
  > Key changes:
  > - Add JournalEntry, begin/commit/rollback_journal to MemoryManager
  > - Refactor atomic() to use journal with nested-call awareness
  > - Refactor commit() to use a single journal spanning all operations
  > - Fix self vs db inconsistency in delete closure
  > - Fix pre-existing clippy is_multiple_of lint
  > - Add 14 journal unit tests and 1 commit-rollback integration test
  > - Add docs/technical/atomicity.md

### Changed

- 💥 restructure workspace into wasm-dbms and ic-dbms layers
  > Split the monolithic ic-dbms crates into a two-layer architecture:
  > - wasm-dbms (generic layer): runtime-agnostic DBMS engine (wasm-dbms-api,
      >   wasm-dbms-memory, wasm-dbms, wasm-dbms-macros)
  > - ic-dbms (IC layer): thin adapter for Internet Computer canister
      >   integration (ic-dbms-api, ic-dbms-canister, ic-dbms-macros,
      >   ic-dbms-client, example, integration-tests)
  >
  > Also fixes integration test wasm paths to account for the new directory
  > depth and updates CI, docs, and build scripts accordingly.
- consolidate IC thread-locals into DbmsContext
- remove duplicated IC database engine module
- update ic-dbms-canister prelude to re-export from wasm-dbms
- update IC API layer to use wasm-dbms database engine
- slim down DbmsCanister macro to IC API only
- update IC canister tests to use wasm-dbms engine
- update CHANGELOG, docs, and API for custom data types and AccessControl trait
  > Update CHANGELOG with custom data types, AccessControl, and DatabaseSchema entries.
  > Remove CallerContext in favor of AccessControl trait. Update IC macros to use
  > generic-layer AccessControl. Update example guest, Cargo.toml dependencies, and
  > documentation across wasm-dbms and ic-dbms crates.
- remove IC-specific documentation from wasm-dbms crates
  > The generic wasm-dbms layer should not reference IC-specific concepts.
  > Remove all doc comments mentioning IC, canister, Principal, Candid,
  > IcDbmsError, and IcDbmsResult from the wasm-dbms crates.
- make error types runtime-agnostic and replace ACL panic with error
  > Rename IC-specific error variants to runtime-agnostic names
  > (StableMemoryError → ProviderError, PrincipalError → IdentityDecodeError),
  > add ConstraintViolation variant, replace panic in ACL last-identity removal
  > with a proper error, simplify get_referenced_tables by removing thread-local
  > cache, and add DbmsContext threading documentation.
- move journal from MemoryManager to transaction module
  > Extract the write-ahead journal from the memory layer into the DBMS
  > layer where it belongs as a transaction concern. Introduce MemoryAccess
  > trait so memory-crate functions are generic over the writer, allowing
  > JournaledWriter to intercept writes for rollback support.

### Documentation

- add custom data types design document
  > Design for issue #35: type-erased CustomValue approach with
  > CustomDataType trait, no generic propagation through core API.
  > Principal becomes a CustomDataType impl in 0.6, prerequisite
  > for wasm-dbms extraction (#48) in 0.7.
- update CHANGELOG for 0.6.0 custom data types
- add custom data types guide and update references
- New website for wasm-dbms
- add Wasmtime WIT Component Model example documentation
- update architecture docs after IC deduplication

### Fixed

- move design doc to .claude/plans, add convention to CLAUDE.md
  > Design docs and plans belong in .claude/plans/ (gitignored),
  > not in docs/plans/. Added this convention to CLAUDE.md.
- **ic-dbms-macros:** fix nullable custom type codegen using inner type
  > When a custom type field is declared as Nullable<T>, the macro now
  > correctly uses the inner type T (not Nullable<T>) for trait lookups
  > like CustomDataType::TYPE_TAG and Encode::decode in all codegen paths.
- address code review findings
  > - Replace String::leak() with OnceLock-based static cache in
      >   Value::type_name() for Custom variants to prevent unbounded leaks
  > - Add compile-time error when #[custom_type] and #[foreign_key] are
      >   combined on the same field
- harden custom data types and add CustomValue constructor
  > - Add cache size guard (max 64 entries) to Value::type_name() to
      >   prevent unbounded memory leaks on IC
  > - Replace panicking .expect() with non-panicking if-let-Ok decode
      >   in macro codegen for custom types (record, insert, update)
  > - Add CustomValue::new<T>() constructor enforcing consistency between
      >   type_tag, encoded bytes, and display string
  > - Add Project table with #[custom_type] owner field to example canister
  > - Add PocketIC integration tests for custom type CRUD and filtering
- exclude guest crate from native tests and fix clippy warning
  > The guest crate targets wasm32-wasip2 and cannot link on native targets.
  > Exclude it from `just test` using --workspace --exclude. Also fix a
  > redundant_closure clippy warning in the host binary.
- update MSRV to 1.91.1, fix ACL persist-before-panic, fix clippy warnings
  > - Set rust-version to 1.91.1 (actual MSRV per cargo msrv) across
      >   workspace Cargo.toml, CLAUDE.md, and all docs
  > - Replace is_multiple_of (Rust 1.87+) with modulo check for MSRV compat
  > - Fix ACL remove_identity to check emptiness before persisting, preventing
      >   corrupted state on non-IC runtimes
  > - Add #[allow(clippy::approx_constant)] to JSON test module
  > - Remove unused _name binding in DatabaseSchema metadata parsing
- remove redundant drop and unnecessary pub visibility in journal refactor
  > Remove the explicit `drop(self)` in `Journal::commit` since the method
  > already takes ownership, and revert test-only struct fields in
  > `memory_manager` back to private visibility since they are unused
  > outside their module.
- add wasm32-wasip2 target to CI and rust-toolchain

### Miscellaneous

- funding
- add justfile recipes for WIT example build and test
  > Adds build_wasm_dbms_example (guest + host) and test_wasm_dbms_example
  > recipes, integrated into build_all and test_all for CI coverage.
- update benchmarks and remove unused dependencies after IC deduplication
- sort deps

## 0.5.0

Released on 2026-02-27

### ⚠ Breaking Changes

- Remove generic T from Query, since it's unnecessary
  > Remove `T` from `Query` and `QueryBuilder`

### Added

- 💥 Remove generic T from Query, since it's unnecessary
  > The `T: TableSchema` argument from `Query` and `QueryBuilder` was actually unnecessary, because it didn't provide
  any meaningful information. The T argument has just been moved to the dbms `select` method, in order to bring
  information to the selected entity.
- add generic select endpoint for untyped table queries (#10)
  > Add a `select_raw` method to the Database trait and a `select` canister
  > endpoint that returns `Vec<Vec<(CandidColumnDef, Value)>>`, enabling
  > table queries by name without compile-time type information. This lays
  > the groundwork for future SQL and JOIN support.
- **ic-dbms-client:** add `select_raw` method to allow selecting untyped columns
- implement JOIN support (INNER, LEFT, RIGHT, FULL) (#47)
  > Add user-facing join guide content to the querying and relationships
  > docs, create a technical deep-dive for the join engine, and update the
  > architecture overview and index with join-related entries.
  > Add cross-table join queries with nested-loop join engine, qualified
  > column resolution, NULL padding for outer joins, and filter support
  > on joined rows. Joins are available through the untyped select_raw
  > path and the generated select canister endpoint.

### Performance

- batch fetch foreign keys in eager relation loading (#41)
  > Replace per-record N+1 foreign key fetching with a batched approach
  > using Filter::In queries. Adds ForeignFetcher::fetch_batch trait method,
  > HashSet-based FK deduplication, benchmarks, and uses the existing
  > TableColumns type alias throughout.

## 0.4.0

Released on 2026-02-06

- New features:
  - [Issue 10](https://github.com/veeso/ic-dbms/issues/10): Added a generic (untyped) `select` canister endpoint that
    returns `Vec<Vec<(CandidColumnDef, Value)>>` instead of typed records.
    - Enables querying tables by name without requiring compile-time type information.
    - Extracted shared `select_columns` core from the typed `select` path.
    - Added `select_raw` method to the `Database` trait and `flatten_table_columns` utility.
    - Extended `DatabaseSchema` trait and `#[derive(DbmsCanister)]` macro to generate dispatch logic and the new
      `select` endpoint.
    - Added to `Client` a new method `select_raw` that returns untyped results, and updated the typed `select` to be a
      thin wrapper around it.
  - [Issue 13](https://github.com/veeso/ic-dbms/issues/13): Added JSON filtering capabilities for querying JSON columns.
    - `JsonFilter::Contains` for PostgreSQL `@>` style structural containment checks
    - `JsonFilter::Extract` for extracting values at JSON paths with comparison operations
    - `JsonFilter::HasKey` for checking path existence in JSON structures
    - Path syntax supports dot notation with bracket array indices (e.g., `user.items[0].name`)
  - [Issue 22](https://github.com/veeso/ic-dbms/issues/22): Added `AgentClient` for the ic-dbms-canister to interact
    with
    the IC from an IC Agent.
- Performance improvements:
  - [Issue 11](https://github.com/veeso/ic-dbms/issues/11): Implemented in-place update instead of delete+insert
    strategy
    ([#37](https://github.com/veeso/ic-dbms/pull/37)).
    - Records whose size is unchanged are now overwritten directly in stable memory, avoiding unnecessary reallocation.
    - Records whose size changes still fall back to delete+reinsert.
    - Added `UpdateIntegrityValidator` that allows keeping the same primary key during updates.
    - Cascade primary key changes to referencing tables via `update_pk_referencing_updated_table`.
    - Extracted shared validation logic into `integrity::common` module.
  - Replaced the external `like` crate with a custom SQL LIKE pattern
    engine ([#42](https://github.com/veeso/ic-dbms/pull/42)).
    - The new iterative two-pointer algorithm runs in O(n*m) worst-case with O(1) space and zero heap allocation,
      replacing the previous recursive approach that had exponential worst-case complexity.
- Bug fixes:
  - Fixed an issue with the IcCanisterClient which called `update` with the wrong amount of arguments.
  - Fixed multi-column `order_by` applying sorts in the wrong order, causing only the last column's sort to survive
    ([#39](https://github.com/veeso/ic-dbms/pull/39)).
- Refactoring:
  - Moved workspace crates into `crates/` directory for better project organization
    ([#38](https://github.com/veeso/ic-dbms/pull/38)).
  - Cleaned up `dbms.rs` with extracted helpers, immutable borrow fixes, and moved tests to a separate file
    ([#40](https://github.com/veeso/ic-dbms/pull/40)).
  - Reorganized and expanded project documentation ([#31](https://github.com/veeso/ic-dbms/pull/31)).
  - Increased test coverage for ic-dbms-api, ic-dbms-canister, and ic-dbms-client.
- Dependencies:
  - [Issue 12](https://github.com/veeso/ic-dbms/issues/12): Bump pocket-ic to 12.0.0.

## 0.3.0

Released on 2025-12-24

- [Field Sanitizers](https://github.com/veeso/ic-dbms/pull/7): it is now possible to tag fields for sanitization.
  Sanitizers can be specified in the schema and will be executed before inserting or updating records.
  - The library comes with built-in sanitizers for common use cases (e.g., trimming whitespace, converting to
    lowercase).
- [Memory Alignment](https://github.com/veeso/ic-dbms/pull/15): Changed the previous memory model which used to store
  records sequentially in a contiguous block of memory with padded fields to a more efficient model that aligns fields
  based on their data types. This change improves memory access speed and reduces fragmentation.
  - [Added a new `MemoryError::OffsetNotAligned`](https://github.com/veeso/ic-dbms/pull/16) variant to handle cases
    where field offsets are not properly aligned
    when writing, which notifies memory corruptions issues.
- [Int8, Int16, Uint8, Uint16 data types](https://github.com/veeso/ic-dbms/pull/17): Added support for smaller integer
  types to optimize memory usage
  and improve performance for applications that require precise control over data sizes.
- [Added `From` implementation for `Value` for inner types](https://github.com/veeso/ic-dbms/pull/18): `i8`, `i16`,
  `i32`, `i64`, `u8`, `u16`, `u32`, `u64`,
  `&[u8]`, `Vec<u8>`, `Principal`, `rust_decimal::Decimal`, `Uuid`, which
  automatically builds the corresponding `Value` variant when converting from these types.
  - Added `FromStr`, `From<&str>`, and `From<String>` implementations for `Value`, which automatically builds a
    `Value::Text`
    variant when converting from string types.
- [FreeSegmentLedger now uses many pages](https://github.com/veeso/ic-dbms/pull/20): The FreeSegmentLedger has been
  updated to utilize multiple pages for tracking free segments.
  This enhancement allows for the free segments ledger to grow and not to die when a single page is full.
  - Added logic to handle reading and writing free segments across multiple pages.
  - Updated tests to cover scenarios involving multiple pages in the FreeSegmentLedger.

## 0.2.1

Released on 2025-12-23

- TableReader never read following pages when reading a table. #5c0ffe6f

## 0.2.0

Released on 2025-12-21

- [Field Validation](https://github.com/veeso/ic-dbms/pull/6): it is now possible to tag fields for validation.
  Validators can be specified in the schema and will be executed before inserting or updating records.
  - The library comes with built-in validators for common use cases (e.g., email, URL, number range).

## 0.1.0

Released on 2025-12-11

- First stable release.

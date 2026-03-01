# Changelog

- [Changelog](#changelog)
  - [Unreleased](#unreleased)
  - [0.6.0](#060)
  - [0.5.0](#050)
  - [0.4.0](#040)
  - [0.3.0](#030)
  - [0.2.1](#021)
  - [0.2.0](#020)
  - [0.1.0](#010)

## Unreleased

### Changed

- **BREAKING**: `atomic()` now uses a write-ahead journal instead of panic-based rollback (#48)
  > The `MemoryManager` now supports `begin_journal()`, `commit_journal()`, and `rollback_journal()`
  > methods. All writes via `write_at` and `zero` are recorded in the journal when active, allowing
  > byte-level rollback on error. This makes atomicity runtime-agnostic — it no longer depends on
  > IC's trap-reverts-stable-memory semantics. Transaction `commit()` now uses a single journal
  > spanning all operations, ensuring all-or-nothing semantics even when multiple operations are
  > involved.

## 0.6.0

Released on 2026-02-27

### Added

- `AccessControl` trait with associated `type Id` for runtime-agnostic ACL (#48)
  > Introduced the `AccessControl` trait in `wasm-dbms-memory` to abstract access control behind
  > a generic interface. Different runtimes can now use different identity types: `Vec<u8>` for
  > the default `AccessControlList`, `Principal` for IC (`IcAccessControlList`), or `()` for
  > `NoAccessControl` (runtimes that don't need ACL). The `A: AccessControl` generic parameter
  > is propagated through `DbmsContext<M, A>`, `WasmDbmsDatabase<'ctx, M, A>`,
  > `DatabaseSchema<M, A>`, and all integrity validators and join engine types. Default type
  > parameters preserve backward compatibility.
- `#[derive(DatabaseSchema)]` macro for automatic `DatabaseSchema<M>` trait generation (#48)
  > A new derive macro that auto-generates the `DatabaseSchema<M>` trait implementation
  > from a `#[tables(...)]` attribute, eliminating ~130+ lines of boilerplate per schema.
  > Two variants exist: a generic one in `wasm-dbms-macros` (for any WASM runtime) and
  > an IC-specific one in `ic-dbms-macros` (uses IC crate paths so IC users don't need
  > `wasm-dbms` as a direct dependency). The macro also generates a `register_tables`
  > associated method for convenient table registration.
- Custom data types: define arbitrary column types with `#[derive(CustomDataType)]` and `#[type_tag = "..."]` (#35)
  > Developers can now define their own column types by implementing the `CustomDataType` trait
  > (via the derive macro) and annotating fields with `#[custom_type]`. Custom types are stored
  > as opaque byte blobs in stable memory and reconstructed on read via their type tag.
- `Value::Custom(CustomValue)` variant for type-erased custom values
  > A new Value variant that wraps custom data types as `(type_tag, encoded_bytes)` pairs,
  > enabling storage and retrieval without compile-time type knowledge.
- `#[custom_type]` field annotation for Table derive macro
  > Fields annotated with `#[custom_type]` are recognized as custom data types during schema
  > generation, insert request building, and record construction.
- `CandidDataTypeKind` for serializable type metadata at API boundaries
  > A Candid-compatible enum mirroring `DataTypeKind` that implements `Serialize`, `Deserialize`,
  > and `CandidType`, with bidirectional conversion to/from `DataTypeKind`.
- `Value::as_custom()` and `Value::as_custom_type::<T>()` accessors
  > Convenience methods to extract custom values: `as_custom()` returns the raw `CustomValue`,
  > while `as_custom_type::<T>()` decodes it into a concrete `CustomDataType` implementor.
- `CustomDataType` trait extending `DataType` with `TYPE_TAG` constant
  > The trait bridges custom types into the DBMS type system, providing encode/decode,
  > type tag identification, and `DataTypeKind::Custom` integration.

### Changed

- Removed duplicated database engine from `ic-dbms-canister`
  > The IC layer now uses the generic `wasm-dbms` engine directly via `DbmsContext<IcMemoryProvider>`.
  > The `IcDbmsDatabase` struct, IC-specific `DatabaseSchema` trait, and duplicated join engine,
  > integrity validators, and transaction system have been removed. The `DbmsCanister` macro now
  > generates `DatabaseSchema<M>` implementations on the annotated struct instead of a separate
  > `CanisterDatabaseSchema` type.

### Breaking Changes

- `AccessControlList` methods renamed: `add_principal` → `add_identity`, `remove_principal` → `remove_identity`, `allowed_principals` → `allowed_identities`
  > These methods are now part of the `AccessControl` trait and use generic identity types
  > instead of being hardcoded to `Principal`. Direct callers of `AccessControlList` must update
  > method names. IC users interacting through the canister API are unaffected.
- `DbmsContext`, `WasmDbmsDatabase`, `DatabaseSchema`, integrity validators, and join engine gain a second generic parameter `A: AccessControl`
  > All types now carry `A: AccessControl` (defaulting to `AccessControlList`), which may require
  > updating type annotations that previously only specified `M: MemoryProvider`.
- Removed `Value::Principal` and `DataTypeKind::Principal` built-in variants
  > Principal is no longer a first-class data type. Use the `#[custom_type]` field annotation
  > on Principal fields instead, which treats Principal as a custom data type.
- `DataTypeKind` no longer implements `Serialize`, `Deserialize`, `CandidType`
  > Use `CandidDataTypeKind` at API boundaries for serializable type metadata.
- Existing stable memory schemas are incompatible due to fingerprint changes
  > The removal of the Principal variant and addition of Custom variant in `DataTypeKind`
  > changes type fingerprints, making previously stored schemas incompatible.

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

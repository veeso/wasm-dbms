# Changelog

All notable changes to this project are documented in this file.

## 0.10.1

Released on 2026-10-07

### Performance

- **memory:** read table rows without copying the rest of the page

> Reading a variable-size record went through MemoryAccess::read_at, which
> allocates and zero-fills a buffer as large as the remainder of the page
> (up to 64 KiB) for every row, copies the page into it and decodes only
> the few bytes of the record.
>
> TableReader already holds the whole page, so decode each record straight
> from that buffer. TableRegistry::read_at now reads the length header and
> then exactly the record body.
>
> The filtered query benchmark over 10,000 rows drops from 7.85 ms to
> 1.35 ms and the join benchmark improves by about 36%.

- **memory:** look up index keys without decoding whole B-tree nodes

- **memory:** load table metadata without copying whole pages

## 0.10.0

Released on 2026-10-07

### Breaking changes

- **acl:** remove access control from wasm-dbms (#101)

> the ACL API, the permission types and the
> access-control generic parameter are removed, and the on-disk layout
> changes (RESERVED_PAGES is now 2). Databases created by releases up to
> 0.9 cannot be opened by this version.

- **dbms:** identify transactions by id and make the SQL engine stateless (#171)

> identify transactions by id and make the SQL engine stateless (#171)

### Added

- Breaking: **acl:** remove access control from wasm-dbms (#101)

> Access control is out of scope for the engine and is now the embedder's
> responsibility. The AccessControl trait, AccessControlList and
> NoAccessControl are removed from wasm-dbms-memory, the permission types
> (TablePerms, IdentityPerms, PermGrant, PermRevoke, RequiredPerm) and the
> DbmsError::AccessDenied and MemoryError::AclLayoutUnsupported variants are
> removed from wasm-dbms-api, and DbmsContext, WasmDbmsDatabase,
> DatabaseSchema, JoinEngine and the integrity validators lose their
> access-control type parameter. DbmsContext::new is the only constructor.
>
> The reserved ACL page is given back: the unclaimed-pages ledger moves from
> page 2 to page 1 and tables start at page 2.

- **sql:** add SQL query support with the wasm-dbms-sql crate (#169)

> The new wasm-dbms-sql crate parses SQL, binds it to a schema and runs it on a DbmsContext through SqlEngine. It supports SELECT with joins, DISTINCT, aggregate functions, GROUP BY, HAVING, ORDER BY, LIMIT and OFFSET; INSERT, UPDATE and DELETE, where UPDATE and DELETE require a WHERE clause and DELETE takes an optional CASCADE or RESTRICT; BEGIN, COMMIT and ROLLBACK with one transaction per caller; and positional ? parameters. Literals and parameters are converted to the type of the column they are used with.
>
> SqlResult and SqlError live in wasm-dbms-api behind the new sql feature, which wasm-dbms forwards. DatabaseSchema gains static_table_name so that a table can be written to by a borrowed name; the derive implements it.
>
> The parser is covered by 300 fixture cases under tests/fixtures/parser. The dialect is documented in docs/reference/sql.md and docs/guides/sql.md, and the SQL examples, the grammar and the reserved-word list of those pages are checked by tests.

- Breaking: **dbms:** identify transactions by id and make the SQL engine stateless (#171)

### Changed

- **api:** move null-aware filter helpers out of Filter impl

> Document the JSON-only serde form of ColumnChanges, the decode allocation error mapping and the test allocator scope.

### Fixed

- **memory:** reuse raw free segments only when the full footprint fits

> Raw free-segment selection compared only the payload length with the segment size, ignoring the 2-byte length header and alignment padding. A payload that fit by body length could overwrite the header of the next live record. Selection now uses the aligned physical footprint.

- **memory:** scan table pages in ledger order

> Typed and raw table readers chose the next page by numeric order. Reclaimed pages are reused last-in-first-out, so a table could own pages in descending order and rows on lower-numbered later pages were skipped. Both readers now walk the page ledger in insertion order.

- **memory:** avoid cursor overflow at the page boundary

> The typed table reader added the record size to a 16-bit page offset before checking for the end of the page. A record ending exactly at byte 65536 panicked in checked builds and wrapped to offset zero otherwise. The end offset is now computed in usize before it is compared with the page size.

- **memory:** skip empty fixed-size slots with the stored stride

> When the typed reader met an empty fixed-size slot it advanced by the data type alignment instead of the raw-record alignment, which includes the 2-byte length header. The cursor became misaligned and rows after a deleted fixed-size row were lost or failed to decode. Empty slots are now skipped with the raw-record alignment.

- **memory:** include alignment padding in write bounds

> The memory manager checked only the encoded length against the page end and then wrote the alignment padding, so a write near the end of a page could zero bytes on the next page. The bounds check now covers the padded footprint, and the page ledger reserves the padded size so inserts move to a new page instead of failing.

- **memory:** initialize autoincrement counters for snapshot tables

> Registering a table from a schema snapshot allocated the autoincrement page but never wrote its counters, so the first generated value after a create-table migration panicked. Snapshot registration now initializes every autoincrement counter from the snapshot column types and rejects non-integer autoincrement columns with an error.

- **memory:** reject oversized raw records without overflow

> Raw record footprints were rounded up in a 16-bit type, so a payload whose aligned footprint reached 65536 bytes overflowed and panicked before any size error could be returned. Footprints are now computed in u64 and records that cannot fit return DataTooLarge.

- **memory:** return errors for extreme heap provider inputs

> HeapMemoryProvider used unchecked multiplication and addition for growth and bounds, so extreme page counts or offsets panicked instead of returning a MemoryResult error. Growth now uses checked arithmetic and fallible allocation, and reads and writes check their range without overflow.

- **memory:** validate registry lengths before decoding

> The unclaimed-pages and schema-registry decoders trusted encoded counts and sliced fields before checking that enough bytes remained, so truncated persisted data panicked. Both decoders now check headers, counts, and complete entries first and return a TooShort decode error, and the schema registry no longer pre-allocates for counts the payload cannot hold.

- **memory:** reject partial fixed-size reads

> The typed read helper allocated a zeroed buffer, ignored how many bytes the raw read returned, and decoded the whole buffer. A fixed-size value crossing the page end was returned with invented zero bytes. Reads now fail with a segmentation fault error unless every byte was read.

- **memory:** split index nodes by encoded byte size

> B+ tree leaf and internal splits chose the midpoint by entry count. With uneven key lengths one half could stay larger than a page even when a valid two-page partition existed, causing a false KeyTooLarge error. Splits now pick the boundary that keeps both halves within a page while balancing their byte sizes.

- **memory:** validate snapshot autoincrement types before claiming pages

> Reject non-integer autoincrement columns before any page is claimed, so a
> failed registration leaves no stale registry entry or leaked pages. Also
> report the largest entry in KeyTooLarge split errors, use one error for
> heap growth failures, and cover mid-entry registry truncation.

- **memory:** keep emptied index leaves linked for range scans

> Deleting the last entry of a non-root B+ tree leaf unlinked it from its siblings while its parent still routed keys to it, so a reinserted key was found by equality lookups but skipped by range scans. Empty leaves now stay in the sibling chain, and the backward search for the first matching leaf skips over empty leaves so duplicates in earlier leaves remain reachable.

- **memory:** reject index and autoincrement metadata above one-byte limits

> Index column names, autoincrement column names and the autoincrement entry count are stored with one-byte prefixes, so values of 256 or more wrapped and reloaded as different metadata (a 256-counter registry decoded empty). Registration and index-ledger initialisation now reject such metadata with ConstraintViolation before any page is claimed.

- **memory:** report free-segment list size as its encoded length

- **transaction:** emit one overlay row per live primary key

> The overlay replayed the whole operation stack for every staged insert, so insert, delete and reinsert of the same primary key (or deleting a committed row and reinserting it) returned the row twice, and reusing a primary key freed by a PK update produced a duplicate of the moved row. Each insert now starts its own lineage and replays only the operations recorded after it; a lineage ends at a delete or at a reinsert of the same key. The overlay reader no longer re-patches inserted rows.

- **transaction:** apply staged updates and deletes only to captured rows at commit

> Transactional updates and deletes resolved the affected rows for the overlay but recorded only the caller's filter, and commit re-executed that filter against committed storage. Rows inserted or changed to match by other callers after staging were therefore updated or deleted too. Both operations now record the primary keys captured at staging and commit with a primary-key IN filter, so the committed row set matches the count returned when the operation was staged. Transaction::update and Transaction::delete no longer take the unused filter argument.

- **join:** pad outer joins from table schema columns

> LEFT, RIGHT and FULL joins built NULL padding from a sample row of the missing side, so an empty table contributed no columns and rows lost their shape. The join engine now takes column definitions from the schema through a new DatabaseSchema::table_columns method generated by #[derive(DatabaseSchema)], so every row contains all joined columns.

- **query:** intersect repeated range bounds on indexed columns

> The filter analyzer merged two range predicates on the same indexed column with Option::or, keeping the left bound instead of the stricter one. Inclusive predicates leave no residual filter, so rows violating the stricter bound were returned. Lower bounds now use the maximum and upper bounds the minimum; exclusive bounds stay as residual filters. The index range scan also returned records equal to the upper bound when the lower bound was greater than it; an inverted range now matches nothing.

- **join:** propagate filter evaluation errors from joined queries

> The join engine evaluated filters with matches_joined_row(..).unwrap_or(false), so ambiguous unqualified columns, out-of-scope tables and invalid operands turned into an empty successful result. Filter errors are now returned to the caller as QueryError.

- **memory:** reject oversized values before storage instead of wrapping lengths

> Encoded sizes and 2-byte length prefixes of Text, Blob, Json, Nullable, Value (including custom values) and derived records now saturate at MSize::MAX instead of wrapping or overflowing. TableRegistry insert and update reject any record whose footprint does not fit a page with MemoryError::DataTooLarge before writing, using the same limit as the raw insert path, which also stops a full-page record from producing a zero-size free segment on delete.

- **memory:** reject schema metadata that exceeds the snapshot format

> Add TableSchemaSnapshot::validate_encoding, which range-checks every one-byte and two-byte length and count of the snapshot format, and call it before any snapshot is persisted (register_table, register_table_from_snapshot, SchemaSnapshotLedger init and write). Oversized names, index column counts or default values now fail with MemoryError::ConstraintViolation without claiming pages, instead of wrapping and reloading as different metadata. Snapshot sizes saturate instead of overflowing.

- **api:** make Json equality, ordering and hashing consistent

> Compare JSON numbers by exact mathematical value without lossy f64 conversion, order an integer before an equal float, treat 0.0 and -0.0 as equal, and derive equality and hashing from the same ordering so Eq, Ord and Hash always agree.

- **dbms:** harden transaction and join validation

- **api:** implement DataType for Decimal

> Decimal was the only built-in column type without a DataType implementation, so Nullable<Decimal> did not compile, neither directly nor as a table column. Implement the trait and cover every built-in and nullable built-in type with a compile test.

- **api:** round-trip Uuid through serde_json

> Uuid serialized itself as bytes, which serde_json writes as a number array, but deserialization only accepted a borrowed byte slice, so a Uuid could not read back its own JSON. Deserialize through a visitor that accepts both byte buffers and byte sequences and requires exactly 16 bytes.

- **query:** compare JSON extracted numbers by magnitude

> JSON extract filters compared values with the derived Value ordering, which orders enum variants before their contents, so 1.5 was not greater than Int64(1) and 1 was greater than Decimal(100). Compare integers and decimals by their numeric value for every operator, including Eq, Ne and In.

- **query:** keep JSON numbers outside the Decimal range

> A JSON number too large for a Decimal, such as 1e100, was extracted as Value::Null, so a present number matched IS NULL and failed NOT NULL. Keep such numbers as a JSON value so they stay present and non-null.

- **sanitize:** convert timezones correctly before 1970

> The timezone sanitizer counted days only forward from 1970, so dates before the Unix epoch, or conversions crossing it backward, ended in the wrong year or on day zero. Use signed civil-date conversions on both sides of the epoch and reject results whose year does not fit the DateTime year field.

- **sanitize:** reject invalid calendar fields in timezone sanitizer

> DateTime fields are public, and the timezone sanitizer assumed they were valid: an invalid month reached unreachable code, and other invalid fields were silently turned into a different date. Validate month, day, hour, minute, second and microsecond first and return a sanitize error.

- **sanitize:** bound timezone offsets and widen offset math

> The timezone sanitizer subtracted two i16 offsets before widening, which overflowed for extreme values and panicked or wrapped. Widen before subtracting and reject offsets of one day or more with a sanitize error.

- **api:** format negative fractional timezone offsets

> DateTime display formatted the signed hours and the signed remaining minutes separately, producing offsets such as -03:-30 and +00:-30. Print one sign followed by the absolute hours and minutes.

- **validate:** count characters in string length validators

> The string length validators measured UTF-8 bytes although the documentation promises character limits, so one emoji counted as four characters. Count Unicode characters in the minimum, maximum and range validators.

- **validate:** reject malformed email dot-atoms and labels

> The email pattern accepted consecutive or trailing dots in the local part and domain labels starting or ending with a hyphen. Match the local part as dot-separated atoms and require every domain label to start and end with a letter or digit.

- **validate:** follow RFC 6838 restricted-name grammar for MIME types

> The MIME type validator rejected characters allowed by RFC 6838 such as _ and !, accepted names starting with a symbol, and had no length limit. Type and subtype now follow the restricted-name grammar: a letter or digit first, then letters, digits or !#$&-^_.+, up to 127 characters each.

- **api:** keep foreign-key removal in ColumnChanges JSON

> ColumnChanges stores a foreign-key change as a nested Option, and JSON wrote both no change and remove as null, so a removal read back as no change. JSON now omits the field for no change, writes null for a removal and the snapshot for a new key. Candid decoding keeps both option layers as before.

- **api:** bound Vec<Value> decode count by input length

> The Vec<Value> decoder reserved capacity from the element count in the header before checking the input, so a four-byte input could request tens of megabytes or more. The count is now checked against the remaining bytes, using the smallest possible encoded entry, and the reservation is fallible.

- **sanitize:** clamp 8-bit and 16-bit integers

> The signed and unsigned clamp sanitizers handled only 32-bit and 64-bit values, leaving 8-bit and 16-bit values unchanged. All integer widths now clamp within their own type ranges, saturating configured bounds that exceed those ranges.

- **query:** treat null rows as no match in LIKE and JSON filters

> LIKE and JSON filters returned InvalidQuery when a nullable Text or Json column held null, aborting otherwise valid queries. A null value now does not match these filters, in normal and joined filtering, while invalid patterns, invalid JSON paths and wrong non-null types are still rejected.

- **query:** reject LIKE patterns ending in a dangling escape

> A LIKE pattern ending in an unescaped backslash silently dropped it, so abc\ matched abc. Such patterns are now rejected with InvalidQuery, while an escaped trailing backslash still matches a literal backslash.

- **memory:** check file provider growth and offset arithmetic

> The WASI and example file memory providers computed the grown file size and read/write ranges with unchecked arithmetic. An extreme growth request could panic or wrap to a small size and truncate the database file. Growth now fails with FailedToAllocatePage and out-of-range offsets with OutOfBounds, leaving the file unchanged.

- **memory:** return previous byte size from WASI grow

- **macros:** parse tuple-style sanitizer attributes

> The Table derive tried the named-argument parser first and returned its error on the documented #[sanitizer(RoundToScaleSanitizer(2))] form. One parser now handles unit, tuple and named-argument sanitizers.

- **macros:** generate consistent records for nullable foreign keys

> A Nullable foreign key produced record code with mismatched types, so a documented optional relation did not compile, and the engine treated a null key as a broken reference. The related record is now Option<Box<Record>> for nullable and required keys alike, and null keys skip the existence check and eager loading.

- **macros:** resolve qualified field types through syn

> The Table derive turned the whole field type into a single identifier, so qualified types such as wasm_dbms_api::prelude::Uint32 made the macro panic. Field types are now inspected through syn by their last path segment, and unsupported types produce a normal compile error instead of a panic.

- **macros:** give generated table types the struct visibility

> The Table derive declared every generated record, request and fetcher type as pub, so a private table struct leaked through a public interface and failed with E0446. Generated types now use the visibility of the source struct.

- **macros:** carry generics through Table and DatabaseSchema derives

> Both derives emitted only the struct identifier, so generic tables and schemas failed with missing type parameters. Type parameters and where clauses now flow into every generated type and impl, table entries in the tables attribute accept generic types, and lifetime or const parameters are rejected with a clear compile error.

- **macros:** keep generated locals clear of user field names

> Generated code used plain local names such as data, values and column next to bindings named after user fields, so ordinary field names broke the expansion. Implementation locals now use a reserved prefix, and a column named where_clause is rejected with a clear error because the update request uses that field for its filter.

- **macros:** derive Encode for structs without fields

> The Encode derive summed field sizes with an empty expression for a struct without fields and produced unparsable code. Such structs now encode to zero bytes with an alignment of one, which also keeps alignment math defined for structs made only of zero-size fields.

- **api:** reject wrong-typed values in dynamic updates

> Generated update requests silently dropped values whose type did not match the column, so a dynamic update could report success while leaving the row unchanged. UpdateRecord::from_values now returns a result, every supplied value is type-checked, and the schema dispatch returns the error to the caller.

- **macros:** fetch foreign records by the referenced column

> Generated foreign fetchers always queried the target primary key, so a foreign key declared on another column was validated and eager loaded against the wrong relation, and delete rules used the wrong value. Fetchers and the engine now use the declared referenced column, and fetch_batch receives the local column to tell relations apart.

- **macros:** keep primary keys in composite indexes and drop duplicates

> Index collection skipped every index attribute on the primary key, so a composite index containing it lost that column, and unique or repeated index attributes emitted the same single-column index several times. The primary key is now a valid composite member and identical indexes are emitted once.

- **macros:** run every declared validator and sanitizer

> Field metadata kept only one validator and one sanitizer, so repeating either attribute silently dropped the earlier ones. Every declaration is now kept in source order and combined through the new SanitizerChain and ValidatorChain types, which apply sanitizers in sequence and return the first validation error.

- **example:** reject malformed WIT values instead of storing defaults

> The example guest turned date-time and UUID values into null and coerced malformed decimal, date and JSON strings into zero or null, so inserts and updates could silently store the wrong data. Conversions are now fallible: valid values round-trip and malformed ones return an invalid-query error before the database is touched.

- **example:** return WIT errors when guest initialization fails

> The example guest opened its database file and registered its tables with expect, so an unusable path or a registration failure trapped the component instead of returning the declared error. Initialization is now fallible, every exported operation returns the failure as a WIT error, and a failed start is retried on the next call.

- **example:** resolve guest table names without leaking memory

> The example guest leaked every caller-provided table name into a static string before checking that the table existed, so each distinct invalid name stayed allocated forever. Names are now resolved against the registered schema tables first and unknown names are rejected without any permanent allocation.

- **example:** run the host demo in an isolated temporary directory

> The host demo used the current directory as guest storage and always deleted wasm-dbms.db on exit, so running it next to a real database destroyed that file. Each run now creates its own fresh temporary directory, preopens it for the guest and removes only that directory.

- **bench:** require a nonzero user count for post generation

- **dbms:** rewrite references when a referenced column is updated

> Updating a column referenced by a foreign key left referencing rows
> pointing at the old value. Any changed referenced column, not only the
> primary key, is now cascaded to the referencing rows. Errors while
> resolving the referenced column are propagated instead of falling back
> to the primary key.

- **api:** decode Decimal and Uuid columns in any position

> A record decoder passes each field the rest of the record bytes, so every column except the last one receives more than its own 16 bytes. Decimal and Uuid rejected any input that was not exactly 16 bytes long, which made a table unreadable as soon as such a column was followed by another one. Both types now decode their first 16 bytes and only reject shorter input, like the other fixed-size types.

- **dbms:** apply transaction changes to rows found through an index

> Inside a transaction, a filter on an indexed column such as the primary key returned rows as they were stored, without the changes the transaction had made to columns that are not indexed. The same row could therefore be read with two different values, and a later update keyed on the new value missed it. Rows read through the stored index are now patched with the transaction's changes before the rest of the filter is checked, and rows the transaction deleted are skipped.

## 0.9.0

Released on 2026-04-28

### Breaking changes

- **acl:** granular per-identity permissions (closes #87)

> granular per-identity permissions (closes #87)

### Added

- **query:** add DISTINCT support to query API

> Adds `distinct_by` field on `Query` and `.distinct(&[..])` builder method.
> The select pipeline deduplicates rows by the listed columns before applying
> ORDER BY / OFFSET / LIMIT, matching standard SQL semantics. Closes #85.

- **query:** add aggregates, GROUP BY, and HAVING (#86)

> Introduce `Database::aggregate` with COUNT/SUM/AVG/MIN/MAX, GROUP BY,
> HAVING, and aggregate-aware ORDER BY/LIMIT/OFFSET. Expose the new flow
> through the IC canister macro (`aggregate_<table>`) and the
> `ic-dbms-client` Client trait + all three client impls.
>
> - AggregateFunction / AggregatedRow / AggregatedValue types in
>   wasm-dbms-api; HAVING / ORDER BY reference aggregate outputs by
>   synthetic agg{N} names.
> - Plan-time validation: SUM/AVG numeric, unknown col/agg, LIKE/JSON
>   rejected in HAVING, joins/eager relations rejected in aggregate.
> - Reject group_by/having on non-aggregate select paths via new
>   QueryError::AggregateClauseInSelect (mirrors JoinInsideTypedSelect).
> - Fix Query candid serialization order after group_by/having insertion
>   (idl_hash-sorted: eager_relations, distinct_by, joins, offset, limit,
>   filter, group_by, having, order_by, columns).
> - Tests: 29 unit tests, 3 select-guard tests, 6 pocket-ic integration
>   tests including round-trip via wrapper canister.
> - Docs: guides/querying.md aggregations section, reference/query.md
>   Aggregate Types + Errors, ic/reference/schema.md endpoint listing,
>   ic/guides/client-api.md client.aggregate example, errors.md updates.
> - Database trait method docs rewritten with proper Errors sections.

- **migrations:** add schema snapshot, Migrate trait, and macro support (#34)

> Lay the groundwork for schema migrations:
>
> - Snapshot types (`TableSchemaSnapshot`, `ColumnSnapshot`, `IndexSnapshot`,
>   `ForeignKeySnapshot`, `OnDeleteSnapshot`, `DataTypeSnapshot`) with
>   versioned binary `Encode`/`Decode`, `Serialize`/`Deserialize`, and
>   feature-gated `CandidType` derives.
> - `TableSchema::schema_snapshot()` with default impl assembling from
>   `table_name` / `primary_key` / `columns` / `indexes` and
>   `Encode::ALIGNMENT`.
> - `dbms::migration` module: `Migrate` trait, `MigrationOp`, `ColumnChanges`,
>   `MigrationPolicy`, `MigrationError`, plus `DbmsError::Migration` and
>   prelude re-exports.
> - `ColumnDef` gains `default: Option<fn() -> Value>` (kept fn-pointer to
>   preserve `Copy`) and `renamed_from: &'static [&'static str]`.
> - `#[derive(Table)]` parses `#[default = ...]`, `#[renamed_from(...)]`, and
>   `#[migrate]`; emits `impl Migrate for T {}` unless `#[migrate]` is set.
> - `DatabaseSchema` trait and `#[derive(DatabaseSchema)]` gain
>   `migrate_default`, `migrate_transform`, `compiled_snapshots` dispatch
>   methods (object-safe via `Self: Sized`).
> - WIT: new `migration-error(string)` variant; guest maps
>   `DbmsError::Migration`.
> - Docs: schema reference covers the three new attributes; new
>   `docs/reference/migrations.md` and `docs/guides/migrations.md`; errors
>   reference lists every `MigrationError` variant.
> - Tests: 13 new macro tests cover `#[default]`, `#[renamed_from]`,
>   `#[migrate]`, dispatch fall-through, unknown-table behaviour, and
>   multi-table snapshot ordering.
>
> Memory layer integration, the migration engine, `Database` wiring, IC
> endpoints, client surface, integration tests, and full WIT migrate APIs
> are tracked in the issue checklist.

- **migrations:** name-hash fingerprint + schema snapshot ledger

> Switch `TableSchema::fingerprint` to hash `table_name()` instead of
> `TypeId` so table identity stays stable across rebuilds and schema
> evolution. `SchemaRegistry::register_table` now detects name-hash
> collisions eagerly: when the fingerprint slot is occupied, it loads the
> persisted snapshot and compares names, returning the new
> `MemoryError::NameCollision` variant on mismatch.
>
> Wire `SchemaSnapshotLedger` into the registry: `register_table`
> initializes the snapshot on the dedicated page, and the encode/decode of
> `SchemaRegistry` now persists `schema_snapshot_page`. Add full module-,
> struct-, and method-level docs to `SchemaSnapshotLedger` plus coverage
> tests for init/load/write/get and page isolation.
>
> Update `docs/technical/memory.md` with the new per-table snapshot page,
> the name-hash fingerprint semantics, the collision-detection flow, the
> ledger API, and the `TableSchemaSnapshot` serialization layout.

- **migrations:** engine + Database wiring for schema migrations

> Wire the migration engine into the generic `wasm-dbms` crate so callers
> can detect drift, plan migrations, and apply structural ops through the
> existing `Database` trait surface.
>
> Engine layer (`crates/wasm-dbms/wasm-dbms/src/database/migration/`):
>
> - `snapshots`: drift hash via `xxh3` (`xxhash-rust`), seeded with
>   `TableSchemaSnapshot::latest_version()`. `compute_drift` compares
>   persisted snapshots (loaded through `SchemaRegistry::stored_snapshots`)
>   with `S::compiled_snapshots()` reachable through the boxed schema.
> - `diff`: pure stored-vs-compiled diff producing `Vec<MigrationOp>`.
>   Renames resolved via the new `DatabaseSchema::renamed_from_dyn`
>   dispatch, type changes routed through the widening whitelist or
>   `MigrationOp::TransformColumn`.
> - `plan`: deterministic op ordering plus policy-driven validation
>   (destructive-op gate, defense-in-depth missing-default check).
> - `apply`: journaled execution of structural ops (`CreateTable`,
>   `DropTable`, `AlterColumn`, `AddIndex`, `DropIndex`); tightening
>   validation (`nullable: false`, `unique: true`) scans existing rows
>   through schema dispatch. Column-mutating ops (`AddColumn`,
>   `DropColumn`, `RenameColumn`, `WidenColumn`, `TransformColumn`) return
>   the new `MigrationError::DataRewriteUnsupported` until the
>   snapshot-driven (de)serializer (issue #91) lands. `DropTable` leaks
>   pages (issue #90).
>
> Database wiring:
>
> - `DbmsContext` gains `Cell<Option<bool>> drift` (lazy cache) and
>   `Cell<bool> migrating` (apply-in-progress guard so internal reads
>   bypass the gate).
> - `WasmDbmsDatabase::ensure_no_drift` is called as the first line of
>   every `Database` method except `rollback`. ACL methods on
>   `DbmsContext` continue to bypass.
> - `Database` trait gains `has_drift`, `pending_migrations` (renamed
>   from the previous `plan_migration`), and `migrate(policy)`. The IC
>   adapter and any other `Database` implementor must now implement them;
>   the test mock in `wasm-dbms-api` is updated.
>
> API additions (`wasm-dbms-api`):
>
> - `fingerprint_for_name(&str) -> u64` (`xxh3_64`), so the engine can
>   derive a registry key for tables it knows only by name.
> - `MigrationError::DataRewriteUnsupported { op }`.
> - `xxhash-rust` workspace dependency.
>
> Memory layer (`wasm-dbms-memory`):
>
> - `SchemaRegistry::{stored_snapshots, register_table_from_snapshot,
>   unregister_table, table_registry_page_by_name}`.
> - `IndexLedger::init_from_keys` for snapshot-keyed init paths.
> - `test_utils::write_dummy_schema_snapshot` so the table-registry unit
>   tests (which allocate raw pages and call `TableRegistry::load`) can
>   satisfy the snapshot-ledger decode that landed in #34's prior commit.
>
> Macro additions (`wasm-dbms-macros`):
>
> - Emit `compiled_snapshots_dyn`, `migrate_default_dyn`,
>   `migrate_transform_dyn`, and `renamed_from_dyn` as object-safe
>   siblings of the existing `Sized` dispatch methods.
>
> Benches (`ic-dbms-canister`):
>
> - Replace hand-written `DatabaseSchema` impls in `eager_relation` and
>   `read_table` with `#[derive(DatabaseSchema)]`. ~330 LOC of boilerplate
>   removed; the derive macro covers the same dispatch.

- **migrations:** IC layer endpoints for schema migrations

> Wires the migration surface through the IC layer: `has_drift` and
> `pending_migrations` as queries, `migrate` as an update, all admin-gated
> via the existing ACL check. Adds matching `Client` trait methods with
> implementations for `IcDbmsCanisterClient`, `IcDbmsAgentClient`, and
> `IcDbmsPocketIcClient`, plus wrapper-canister bindings and PocketIC
> coverage exercising both the direct client and wrapper paths on a
> fresh canister (no drift, empty plan, migrate is a no-op).

- **migrations:** WIT surface and IC migration docs

> Add has-drift / pending-migrations / migrate to wit/dbms.wit plus the
> supporting snapshot, op, and policy records. Wire them through the
> example guest and host so the round-trip is exercised end-to-end.
>
> Document the IC migration flow: new docs/ic/guides/migrations.md, full
> Candid signatures in docs/ic/reference/schema.md, and a Schema
> Migrations section in docs/ic/guides/client-api.md.

- **migrations:** snapshot-driven record codec for column-mutating ops

- **memory:** page reclamation via unclaimed-pages ledger (closes #90)

> Reserve page 2 for an LIFO ledger of pages released by destructive ops.
> Rename `MemoryAccess::allocate_page` to `claim_page` and add `unclaim_page`,
> both built on new `grow_one_page` / `zero_page` primitives. `claim_page`
> pops the ledger before bumping the high-water mark; `unclaim_page` zeros
> the page and pushes it. JournaledWriter records the full pre-zero page so
> rollback restores ledger state and contents.
>
> Wire MigrationOp::DropTable to walk every page owned by a dropped table —
> record pages, page/free-segments/index ledgers (incl. every B-tree node
> across each index), schema-snapshot, autoincrement — and return them all
> to the unclaimed-pages ledger. Add `release_pages` on each ledger and on
> TableRegistry; SchemaRegistry::unregister_table drives the full release.

- Breaking: **acl:** granular per-identity permissions (closes #87)

> Replaces the flat allow-list ACL with a granular permission model.
> Each identity carries `admin` / `manage_acl` / `migrate` flags plus
> `TablePerms` (READ/INSERT/UPDATE/DELETE) scoped to all tables or to
> specific tables. CRUD endpoints generated by `#[derive(DbmsCanister)]`
> gate per-operation; migration endpoints gate on `migrate`.
>
> Breaking changes:
>
> - ACL page layout bumped to v2 (no migration from 0.8.x).
> - `IcDbmsCanisterInitArgs.allowed_principals` is `Option<Vec<Principal>>`;
>   `None`/empty bootstraps the deployer as full admin.
> - Canister endpoints `acl_add_principal` / `acl_remove_principal` /
>   `acl_allowed_principals` removed; replaced by `grant_admin`,
>   `revoke_admin`, `grant_manage_acl`, `revoke_manage_acl`,
>   `grant_migrate`, `revoke_migrate`, `grant_all_tables_perms`,
>   `revoke_all_tables_perms`, `grant_table_perms`, `revoke_table_perms`,
>   `remove_identity`, `list_identities`, `my_perms`.
> - New `DbmsError::AccessDenied { table, required }` returned in lieu
>   of trapping for unauthorized CRUD/ACL/migrate calls.
> - New `MemoryError::AclLayoutUnsupported`.

### Fixed

- **query:** correct candid field hash order for Query serialization

> distinct_by hash places it between eager_relations and joins, not between
> filter and order_by. Roundtrip test passed because both ends used same
> wrong order; cross-canister decode failed with integer/usize mismatch.

- **wit:** realign dbms.wit with current Database trait

> Rewrite the WIT interface to match the current `wasm-dbms-api`:
>
> - value: drop f32/f64 (no Value variants); add decimal/date/datetime/json/
>   uuid (string-encoded) and custom (record).
> - query: replace single order-by/dir with list<order-key>; add distinct-by,
>   eager-relations, joins, group-by, having (JSON-encoded Filter).
> - delete: take new delete-behavior enum (restrict/cascade).
> - update: take optional filter (JSON-encoded Filter).
> - aggregate: new endpoint computing aggregate-function list per group,
>   returning aggregated-row entries.
> - dbms-error: split into structured variants mirroring DbmsError /
>   QueryError (PrimaryKeyConflict, UniqueConstraintViolation,
>   AggregateClauseInSelect, JoinInsideTypedSelect, etc.).
>
> Threads the aggregate dispatch through `DatabaseSchema` (trait + macro)
> and updates the wasmtime/wasi guest + host examples accordingly.
> End-to-end demo verified.

- **bench:** add aggregate to DatabaseSchema impls and CI bench-build job

- migration drift caching and rewrite handling

### Build

- **integration-tests:** use pocket-ic-harness crate

> Replace in-tree pocket-ic-tests-macro and PocketIcTestEnv wrapper with
> the external pocket-ic-harness crate.
>
> - Remove pocket-ic-tests-macro crate (replaced by pocket_ic_harness::test).
> - Remove src/pocket_ic.rs, src/actor.rs, src/wasm.rs and the TestEnv trait.
> - Implement Canister for TestCanister and CanisterSetup for TestCanisterSetup.
> - Add TestEnvExt trait providing env.dbms_canister() and
>   env.dbms_canister_client_integration() helpers.
> - Resolve wasm paths via CARGO_MANIFEST_DIR so loading is cwd-independent.

- bump rust 1.95.0

- bump MSRV to 1.92.0

## 0.8.2

Released on 2026-04-21

### Added

- derive Default on generated *UpdateRequest structs

> All fields on update requests are Option<T>, so Default is always
> derivable. This enables the ..Default::default() struct-update pattern
> when constructing partial updates, removing the need to write None
> for every untouched field.
>
> Bumps workspace version to 0.8.2.

### Fixed

- **ci:** combine WASM targets and fail on missing artifacts

> Use single toolchain install with both targets and add
> if-no-files-found: error to artifact uploads for fast failure.

- **ci:** correct host binary name to wasm-dbms-example

> The [[bin]] name in Cargo.toml is wasm-dbms-example, not
> wasm-dbms-example-host (which is the package name).

- **ci:** enable include-hidden-files for artifact upload

> The .artifact/ directory starts with a dot, so upload-artifact@v4
> skips it by default when include-hidden-files is false.

## 0.8.1

Released on 2026-04-05

### Fixed

- align insert offset to RawRecord alignment for fixed-size records

> The second insert into a table with all fixed-size columns (e.g. Uint64 +
> Uint64 + 1-byte CustomDataType) failed with OffsetNotAligned because:
>
> 1. table_registry::insert aligned the write offset using E::ALIGNMENT
>    instead of RawRecord<E>::ALIGNMENT (which includes the 2-byte header).
> 2. page_ledger::commit double-wrapped the type in RawRecord, computing
>    RawRecord<RawRecord<E>>::ALIGNMENT instead of RawRecord<E>::ALIGNMENT,
>    causing incorrect free-space tracking.

## 0.8.0

Released on 2026-04-05

### Added

- add select_join to Database trait and rename CandidColumnDef to JoinColumnDef

> Move select_join from a #[doc(hidden)] method on WasmDbmsDatabase to a
> proper public method on the Database trait, making it the official API
> for join queries. Rename CandidColumnDef to JoinColumnDef to better
> reflect its purpose as the column definition type that carries table
> provenance for join results. Update all docs to reference select_join
> instead of select_raw for join queries.

### Fixed

- apply LIMIT and OFFSET after ORDER BY to match SQL semantics

> LIMIT and OFFSET were applied during the table scan (before sorting),
> so ORDER BY + LIMIT didn't produce correct "top N sorted" results.
> Now, when ORDER BY is present, LIMIT and OFFSET are deferred to after
> sorting, matching standard SQL evaluation order. The early-exit
> optimization is preserved when no ORDER BY is specified.

## 0.7.2

Released on 2026-04-02

### Fixed

- export macros in wasm-dbms-api

> wasm-dbms-macros were not actually exported as the documentation example were showing

## 0.7.1

Released on 2026-04-01

### Fixed

- rename generated loop variables in Table macro to avoid shadowing user field names

> Fields named `value` caused compile errors because the generated code
> used `value` as both the loop binding and destructure binding, shadowing
> the field accumulator variable.

## 0.7.0

Released on 2026-03-31

### Breaking changes

- change MemoryProvider::read and MemoryAccess::read_at to take &mut self

> MemoryProvider::read signature changed from &self to &mut self.

### Added

- **bench:** add benchmark comparison crate against other in-memory DBMS

> Add wasm-dbms-bench crate with Criterion benchmarks comparing wasm-dbms
> against SQLite (in-memory) and DuckDB (in-memory) across CRUD operations,
> bulk inserts, queries (filter, order, join), and transactions.
>
> Includes CI workflow for running benchmarks and uploading artifacts.

- B+ tree indexes for accelerated queries

> Add a complete B+ tree index system to wasm-dbms. Every table
> automatically gets an index on its primary key, and users can declare
> additional single-column or composite indexes with the `#[index]`
> attribute.
>
> Key changes:
>
> Memory layer (wasm-dbms-memory):
>
> - IndexLedger: per-table registry mapping column sets to B-tree roots
> - IndexTree: page-per-node B+ tree with variable-size keys, doubly-linked
>   leaves for range scans, and automatic node splitting/merging
> - RecordAddress: lightweight (page, offset) pointer stored in leaf entries
> - SchemaRegistry/TableRegistryPage extended with index_registry_page
> - TableRegistry now owns and exposes an IndexLedger
> - INSERT/UPDATE/DELETE maintain all indexes eagerly
>
> DBMS layer (wasm-dbms):
>
> - FilterAnalyzer: extracts index plans (Eq, Range, In) from query filters
> - IndexReader: unified view merging base B-tree results with transaction
>   overlay additions/removals
> - IndexOverlay: in-memory BTreeMap tracking uncommitted index changes per
>   transaction, flushed on commit, discarded on rollback
> - SELECT, UPDATE, DELETE, and JOIN queries use indexes when a suitable
>   plan is found; remaining filter conditions applied as residual checks
>
> Macro layer (wasm-dbms-macros):
>
> - `#[index]` attribute on fields for single-column indexes
> - `#[index(group = "name")]` for composite indexes
> - Automatic primary key index generation in TableSchema
> - Deduplicated shared macro logic from ic-dbms-macros into wasm-dbms-macros
>
> Also includes CI improvements, dependency updates, and documentation
> updates covering the index memory layout, query optimization, and
> architecture changes.

- add wasi-dbms-memory crate with file-backed MemoryProvider

> Implements WasiMemoryProvider backed by a single flat file,
> enabling wasm-dbms to persist data on any WASI-compliant runtime
> (Wasmer, Wasmtime, WasmEdge). The file layout is byte-for-byte
> equivalent to IC stable memory.

- add #[unique] attribute for table fields

> Add support for the #[unique] field attribute that enforces uniqueness
> constraints on non-primary-key columns. A unique field automatically
> gets a B+ tree index for efficient O(log n) duplicate detection.
>
> - Parse #[unique] in Table derive macro, set ColumnDef::unique and
>   auto-generate an index for the field
> - Add UniqueConstraintViolation error variant to QueryError
> - Enforce uniqueness in InsertIntegrityValidator and
>   UpdateIntegrityValidator (update allows keeping own value)
> - Add comprehensive tests for insert, update, and transaction scenarios
> - Update schema, errors, and IC reference documentation

- add #[autoincrement] attribute for table fields

> Add support for autoincrement columns in table schemas. Fields annotated
> with `#[autoincrement]` automatically generate sequential values on
> insert, starting from zero and incrementing by one.
>
> Implementation across all layers:
>
> **Memory layer (wasm-dbms-memory):**
>
> - AutoincrementLedger: per-table ledger storing current counter values
>   for each autoincrement column, persisted to a dedicated memory page
> - AutoincrementRegistry: HashMap-based registry mapping column names to
>   their current Value, with custom Encode implementation
> - SchemaRegistry: conditionally allocates an autoincrement page when a
>   table has autoincrement columns (Option<Page> in TableRegistryPage)
> - TableRegistry: integrates AutoincrementLedger as an optional field,
>   exposes autoincrement_next() to get the next value for a column
>
> **API layer (wasm-dbms-api):**
>
> - ColumnDef: add auto_increment field to column definitions
> - MemoryError::AutoincrementOverflow: new error variant returned when
>   a column reaches its type's maximum value (uses checked_add)
> - Filter: support autoincrement columns in query filters
>
> **Macro layer (wasm-dbms-macros):**
>
> - Table derive macro: parse #[autoincrement] attribute on fields,
>   propagate auto_increment flag to generated TableSchema impl
>
> **DBMS layer (wasm-dbms):**
>
> - Database: wire autoincrement through insert operations
> - Transaction overlay: support autoincrement in transactional context
>
> **Supported types:** Int8, Int16, Int32, Int64, Uint8, Uint16, Uint32,
> Uint64. Overflow returns AutoincrementOverflow error to prevent
> duplicate key generation.

### Changed

- remove duplicated macros from ic-dbms-macros

> Remove Encode, Table, CustomDataType, and DatabaseSchema derive macros
> from ic-dbms-macros, keeping only DbmsCanister. These macros were
> duplicated from wasm-dbms-macros with the only differences being crate
> path prefixes and Candid/Serde derives on generated types.
>
> IC crates now re-export the wasm-dbms-macros versions through their
> preludes. To support the IC requirement of Candid-serializable generated
> types, a #[candid] attribute is added to wasm-dbms-macros' Table derive:
> when present, generated Record, InsertRequest, and UpdateRequest types
> derive CandidType, Serialize, and Deserialize.

- Breaking: change MemoryProvider::read and MemoryAccess::read_at to take &mut self

> File-backed providers need mutable access to seek before reading.
> Previously this was worked around with try_clone() on every read.
> Making the trait honest about mutation removes that overhead and
> simplifies implementations.

### Fixed

- prevent PK from being indexed twice

> using `#[index]` on the primary key lead to duplicated index for the primary key

- track PK changes in overlay patch_row to chain subsequent operations

- add missing Int8, Int16, Uint8, Uint16 variants to DataTypeKind and CandidDataTypeKind

> Value enum already had these variants but DataTypeKind did not,
> causing compile errors when using 8-bit or 16-bit integer types
> in table field definitions via the derive macro.

- autoincrement macro codegen and DBMS integration

> Fix InsertRequest codegen for autoincrement fields:
>
> - from_values: wraps found values in Autoincrement::Value, absent ones
>   in Autoincrement::Auto
> - into_values: skips Autoincrement::Auto fields, includes Value fields
> - into_record: unwraps Autoincrement::Value to inner type for schema
>
> Fix insert_contract test helper using wrong column index (order vs
> user_id). Add full coverage tests for autoincrement at the DBMS layer:
> sequential generation, explicit override, no recycle after delete,
> transaction commit/rollback counter behavior, from_values/into_values
> variants, and filter on autoincrement column.
>
> Wire autoincrement_next into TableRegistry as a public method.

### Build

- update dependencies

## 0.6.0

Released on 2026-03-02

### Breaking changes

- migrate Principal from built-in to CustomDataType

> Value::Principal and DataTypeKind::Principal removed.
> Principal fields in tables must now use #[custom_type] annotation.
> Existing stable memory schemas are incompatible (fingerprint change).

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

- Breaking: migrate Principal from built-in to CustomDataType

- add WIT interface definition for wasm-dbms Component Model API

- add WIT guest crate with FileMemoryProvider and example schemas

> Create the wasm-dbms-example-guest crate scaffolding with:
>
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
>
> - Add JournalEntry, begin/commit/rollback_journal to MemoryManager
> - Refactor atomic() to use journal with nested-call awareness
> - Refactor commit() to use a single journal spanning all operations
> - Fix self vs db inconsistency in delete closure
> - Fix pre-existing clippy is_multiple_of lint
> - Add 14 journal unit tests and 1 commit-rollback integration test
> - Add docs/technical/atomicity.md

### Changed

- Breaking: restructure workspace into wasm-dbms and ic-dbms layers

> Split the monolithic ic-dbms crates into a two-layer architecture:
>
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

- **docs:** override minima page layout to remove duplicate title

> The jekyll-titles-from-headings plugin auto-extracts titles from
> markdown # headings, and minima's default page layout renders them
> as an <h1>. Since the markdown content already contains the heading,
> this caused every doc page to show the title twice.
>
> Override the page layout to only render the content, letting the
> markdown heading serve as the sole visible title.

### Style

- apply nightly rustfmt formatting

- formatted code

- formatted code

## 0.5.0

Released on 2026-02-27

### Breaking changes

- Remove generic T from Query, since it's unnecessary

> Remove `T` from `Query` and `QueryBuilder`

### Added

- Breaking: Remove generic T from Query, since it's unnecessary

> The `T: TableSchema` argument from `Query` and `QueryBuilder` was actually unnecessary, because it didn't provide any meaningful information. The T argument has just been moved to the dbms `select` method, in order to bring information to the selected entity.

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

- ic-dbms 0.5.0

> updated getrandom to 0.4

### Performance

- batch fetch foreign keys in eager relation loading (#41)

> Replace per-record N+1 foreign key fetching with a batched approach
> using Filter::In queries. Adds ForeignFetcher::fetch_batch trait method,
> HashSet-based FK deduplication, benchmarks, and uses the existing
> TableColumns type alias throughout.

## 0.4.0

Released on 2026-02-06

### Added

- IcDbmsAgentClient for external systems

> Add a new client implementation using ic-agent to allow external systems
> (frontend applications, backend services, CLI tools) to communicate with
> IC DBMS canisters.
>
> - Add IcDbmsAgentClient with full Client trait implementation
> - Add ic-agent feature flag to ic-dbms-client
> - Add IcAgentError for agent-specific error handling
> - Update documentation with usage examples
> - Reorganize integration tests into separate modules
> - Add comprehensive tests for the agent client
> - Update Rust toolchain to 1.93.0

- **api:** implement JSON filtering for queries (#13) (#30)

> - docs: add JSON filter design document
>
> Design for JSON filtering in ic-dbms queries covering:
>
> - JsonFilter enum with Contains, Extract, HasKey operations
> - JsonCmp enum for comparisons on extracted values
> - Dot notation path syntax with bracket array indices
> - Structural containment (PostgreSQL @> style)
> - Module structure and testing strategy

### Changed

- move ic-dbms-* crates into crates/ directory (#38)

> Move the 4 library crates (ic-dbms-api, ic-dbms-canister, ic-dbms-client,
> ic-dbms-macros) into a crates/ subdirectory for better workspace organization.
> Update all path references in workspace members, include paths, and
> dependency paths across the project.

- **canister:** clean up dbms.rs with multiple improvements (#40)

> - refactor(canister): clean up dbms.rs with multiple improvements
>
> * Extract duplicated record collection pattern from update/delete into
>   collect_matching_records helper method
> * Fix with_transaction to use immutable borrow (get_transaction)
>   instead of unnecessarily getting a mutable reference
> * Extract values_to_schema_entity as a standalone module-level function
>   instead of an inner function inside update
> * Wrap insert oneshot path in atomic for consistency with update/delete
> * Hoist PK lookup outside inner loop in delete_foreign_keys_cascade
> * Use pks.len() for update transaction count instead of re-querying
> * Select only PK column in existing_primary_keys_for_filter
> * Re-export NextRecord from memory module for crate-wide use

- **api:** replace like crate with custom LIKE pattern engine (#42)

> - refactor(api): replace `like` crate with custom LIKE pattern engine
>
> Remove the external `like` dependency and implement an in-house SQL LIKE
> pattern matcher with an iterative two-pointer algorithm. The new engine
> runs in O(n*m) worst-case with O(1) space and zero heap allocation,
> replacing the previous recursive approach that had exponential worst-case
> complexity. Includes full Unicode/multi-byte character support.

### Fixed

- **canister:** apply multi-column order_by sorts in correct order (#39)

> The order_by loop was applying each sort column sequentially, which
> meant only the last column's sort survived. Since Rust's sort_by is
> stable, reversing the iteration order (least-significant column first)
> produces correct multi-column ordering.

### Performance

- **canister:** implement in-place update instead of delete+insert (#37)

> Replace the delete-then-insert update strategy with a proper in-place
> update approach in TableRegistry, improving performance by avoiding
> unnecessary memory reallocation when record size is unchanged.
>
> - Add TableRegistry::update with two-path strategy: in-place overwrite
>   for same-size records, delete+reinsert for size changes
> - Add UpdateIntegrityValidator that allows keeping the same PK during
>   updates, unlike InsertIntegrityValidator which rejects any existing PK
> - Add validate_update to DatabaseSchema trait and macro generation
> - Extract shared validation logic (column, FK, non-nullable checks)
>   into integrity::common module to eliminate duplication
> - Cascade PK changes to referencing tables via
>   update_pk_referencing_updated_table
> - Remove DeleteBehavior::Break variant (workaround for old strategy)
> - Extract sanitize_values helper from insert/update flows
> - Add tests for multi-record updates, PK conflict e2e, FK cascade
>   in transactions, and non-nullable field validation

### Build

- PocketIC 12.x (#29)

## 0.3.0

Released on 2025-12-24

### Added

- Sanitizers (#8)

> - feat: Sanitizers
>
> it is now possible to tag fields for sanitization. Sanitizers can be specified in the schema and will be executed before inserting or updating records.

- Int8, Int16, Uint8, Uint16 data types (#17)

> - feat: Int8, Int16, Uint8, Uint16 data types
>
> Added support for smaller integer types to optimise memory usage and improve performance for applications that require precise control over data sizes

- Added `From` implementation for `Value` for inner types (#18)

> e.g. `u8` to `Value::Uint8`, `rust_decimal::Decimal` to `Value::Decimal`

### Changed

- memory align align checks (#16)

> - refactor: read, write and zero methods must check whether we are records with an offset not aligned
> - test: check whether padding bytes are zeroed when writing
> - test: Test to check the free segment has the new offset and size with padding taken into account
> - docs: changelog pr

### Fixed

- FreeSegmentLedger now uses many pages (#21)

> The FreeSegmentLedger has been updated to utilize multiple pages for tracking free segments.

### Performance

- Use aligned records in memory instead of sequential write (#15)

> Changed the previous memory model, which used to store records sequentially in a contiguous block of memory with padded fields, to a more efficient model that aligns fields based on their data types. This change improves memory access speed and reduces fragmentation.

## 0.2.1

Released on 2025-12-23

### Fixed

- table reader never read the next page

- 0.2.1

## 0.2.0

Released on 2025-12-21

### Added

- field validation (#6)

> - feat: Table field validation
>
> Added `Validate` trait for validation. Added many common used validators into `prelude`

### Build

- rust 1.92

## 0.1.0

Released on 2025-12-11

### Added

- initial commit

- MemoryProvider

- Memory Manager and Schema

- ACL

- Working on Table Registry

- DeletedRecordsLedger

- TableRegistry Insert Op

- TableReader

- Return also nextRecord position when reading from table

- TableRegistry::delete

- TableRegistry::update

- Data types

- Table types for dbms

- Filter

- QueryBuilder

- prelude

- Insert/Update records

- Transaction

- Implementing Database; Implemented Filter::matches

- Select

- Created Encode derive within ic-dbms-macros

- eager relations loader in select

- DatabaseOverlay

- Select using the overlay

- Validate insert record

- Insert oneshot

- IntegrityValidator trait

- Insert with transaction; rollback and commit

- Delete

- Update

- Example canister

- Derive Table

- API module

- Query must be CandidType + Serialize

- ic-dbms-client init

- ic-dbms-client

- ic-dbms-client example

- Pocket IC client for ic-dbms

- IcDbmsCanister derive macro

- Automatically derive referenced tables

- ic_dbms_canister macro rules

### Changed

- Renamed DeletedRecords to FreeSegments

- Dbms modules

- Moved public API types to ic-dbms-api

- Export ic_dbms_api from ic_dbms_canister

### Fixed

- Records must be prefixed with their length in 2 bytes

- Find adjacent free segments to optimize space

- Removed Delegate

- Load relations from values

- do not register same table twice in the register

- Removed Untyped records; no more necessary

- use ValuesSource to allow loading records correctly when the same table has more than one FK on a record

- Made table registry independent from T

- Do not export ic-dbms-api in ic-dbms-canister

- Missing order_by use

- url

- Changed bin name (will be removed later(

- force use of ic_dbms_api for macros

- Made Query exportable

- error

- get_referencing_tables not working with self referencing tables

- removed macro rules

- missing methods and docs for ic-dbms-client

- table method

- Query candid encoding

### Performance

- Reduced memory usage

- Read page only when the offset is zero.

- Cache referenced tables

### Build

- just script for publish

- 0.1

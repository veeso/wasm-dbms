# Design: Custom Data Types

**Date:** 2026-02-27
**Status:** Approved
**Target:** 0.6.0
**Issue:** #35
**Related:** #48 (wasm-dbms migration)

## Goal

Allow users to define arbitrary custom column types beyond the built-in primitives, with full
support for filtering (Eq, Gt, Lt, etc.), ordering, and hashing. The design must avoid generic
type parameter propagation through the core API (Value, Filter, Query, DbmsContext stay non-generic).

## Decisions

| Decision | Choice |
|----------|--------|
| Extension mechanism | Type-erased `CustomValue` in a single `Value::Custom` variant |
| Generic propagation | None — `Value`, `Filter`, `Query`, `DbmsContext` stay non-generic |
| Operation support | Full (Eq, Gt, Lt, ordering, hashing) via byte-level comparison |
| Custom type detection in macros | Explicit `#[custom_type]` annotation on fields |
| Principal in 0.6 | Becomes a `CustomDataType` impl, removed as built-in variant |
| Data migration | Clean break at 0.6 (no migration support, pre-1.0) |
| Candid in wasm-dbms (future) | Feature flag on `wasm-dbms-api`, not wrapper types |

## Design

### CustomDataType Trait

Custom types implement `CustomDataType`, which extends the existing `DataType` trait:

```rust
pub trait CustomDataType: DataType {
    /// Unique string identifier for this type (e.g., "principal", "role").
    /// Must be stable across versions — it appears in runtime Value representations.
    const TYPE_TAG: &'static str;
}
```

`DataType` already requires: Clone, Debug, Display, PartialEq, Eq, Default, PartialOrd, Ord,
Hash, Encode, CandidType, Serialize, Deserialize, `Into<Value>`.

### CustomValue Struct

The DBMS engine stores custom values as type-erased `CustomValue`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CustomValue {
    /// Type identifier (from CustomDataType::TYPE_TAG).
    pub type_tag: String,
    /// Binary encoding via Encode trait.
    pub encoded: Vec<u8>,
    /// Cached Display output for human-readable representation.
    pub display: String,
}
```

Trait implementations:

- **PartialEq/Eq**: compare `type_tag` + `encoded` bytes.
- **Ord**: compare `type_tag` first; if equal, compare `encoded` bytes lexicographically.
- **Hash**: hash `type_tag` + `encoded`.
- **Display**: returns `display` field.
- **CandidType**: derived (all fields are Candid-compatible).

Ordering contract: for custom types used with range filters (Gt, Lt, Ge, Le) or ORDER BY,
the `Encode` output must be order-preserving (if `a < b` then `a.encode() < b.encode()`
lexicographically). Equality filters (Eq, Ne, In) only require canonical encoding (same value
produces same bytes), which `Encode` already guarantees.

### Value Enum

One new variant appended at the end:

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, CandidType, Serialize, Deserialize)]
pub enum Value {
    Blob(types::Blob),
    Boolean(types::Boolean),
    Date(types::Date),
    DateTime(types::DateTime),
    Decimal(types::Decimal),
    Int8(types::Int8),
    Int16(types::Int16),
    Int32(types::Int32),
    Int64(types::Int64),
    Json(types::Json),
    Null,
    Principal(types::Principal),  // removed in 0.6, present during transition
    Text(types::Text),
    Uint8(types::Uint8),
    Uint16(types::Uint16),
    Uint32(types::Uint32),
    Uint64(types::Uint64),
    Uuid(types::Uuid),
    Custom(CustomValue),
}
```

New accessor methods:

```rust
impl Value {
    pub fn as_custom(&self) -> Option<&CustomValue> { /* ... */ }
    pub fn as_custom_type<T: CustomDataType>(&self) -> Option<T> { /* ... */ }
}
```

In 0.6, the `Principal` variant is removed and `Principal` becomes a `CustomDataType` impl.

### DataTypeKind Split

`DataTypeKind` gains a `Custom(&'static str)` variant. To preserve `Copy` and static slice
compatibility while supporting serialization at the API boundary, we split into two enums:

**Internal** (used in `ColumnDef`, macro-generated code, static slices):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataTypeKind {
    Blob, Boolean, Date, DateTime, Decimal,
    Int32, Int64, Json, Principal, Text,
    Uint32, Uint64, Uuid,
    Custom(&'static str),
}
```

Loses `CandidType`, `Serialize`, `Deserialize` derives (can't round-trip `&'static str`).

**API boundary** (serializable, used in `CandidColumnDef`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, CandidType, Serialize, Deserialize)]
pub enum CandidDataTypeKind {
    Blob, Boolean, Date, DateTime, Decimal,
    Int32, Int64, Json, Principal, Text,
    Uint32, Uint64, Uuid,
    Custom(String),
}

impl From<DataTypeKind> for CandidDataTypeKind { /* variant-by-variant conversion */ }
```

`CandidColumnDef` updates to use `CandidDataTypeKind` instead of `DataTypeKind`.

In 0.6, the `Principal` variant is removed from both enums.

### Macro Changes

#### `#[derive(Table)]` — updated

The macro recognizes `#[custom_type]` field annotations. When present:

- **ColumnDef**: generates `DataTypeKind::Custom(<T as CustomDataType>::TYPE_TAG)`.
- **`to_values()`**: generates `Value::Custom(CustomValue { type_tag, encoded, display })`
  instead of `Value::<Variant>(field)`.
- **`from_values()`**: generates `<T as Encode>::decode(&cv.encoded)` to reconstruct the
  concrete type from a `CustomValue`.

#### `#[derive(CustomDataType)]` — new

Generates boilerplate for a custom type:

```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash,
         Encode, CandidType, Serialize, Deserialize, CustomDataType)]
#[type_tag = "role"]
pub enum Role {
    Admin,
    User,
    Moderator,
}
```

Generates:
- `impl CustomDataType for Role { const TYPE_TAG = "role"; }`
- `impl Display for Role` (uses Debug, skipped if manually implemented)
- `impl Default for Role` (first variant, skipped if manually implemented)
- `impl From<Role> for Value` (constructs `Value::Custom(CustomValue { ... })`)

Requires `#[type_tag = "..."]` attribute.

#### `#[derive(Encode)]` — untouched

Already works on any struct/enum with fields that implement `Encode`.

#### `#[derive(DbmsCanister)]` — untouched

Works with `Value` and `ColumnDef` transparently.

### Memory and Storage

The memory layer requires **no changes**. Custom types are transparent at the storage level:

- **Insert**: macro-generated `Encode` on the table struct encodes each field using the
  concrete type's `Encode` impl. Custom fields are just another field.
- **Read**: macro-generated `Encode::decode` on the table struct decodes each field using
  the concrete type. Custom fields reconstructed directly.
- **Schema registry**: stores `TableFingerprint -> page mappings`. Does not store column types.
  No changes needed.
- **Page management**: stores `RawRecord<E>` where E is the table struct. Unaffected.
- **Record size**: calculated from concrete types' `Encode::SIZE`. Unaffected.

`CustomValue` exists only as a **runtime representation** when values pass through the DBMS
engine (filters, query results, API responses). It is never persisted directly.

### Filter and Query Engine

**No changes needed.** The filter comparison logic uses `Value`'s derived `PartialOrd`:

```rust
Filter::Eq(field, value) => col_value == value   // uses Value::PartialEq
Filter::Gt(field, value) => col_value > value     // uses Value::PartialOrd
```

Since `Value::Custom(CustomValue)` implements `Ord` via byte comparison, all filter operations
work transparently.

### Principal Migration (0.6)

`Principal` transitions from built-in to custom type:

**Before (0.5):**
- `Value::Principal(types::Principal)` — built-in variant
- `DataTypeKind::Principal` — built-in variant
- Table fields: `pub owner: Principal` (no annotation)

**After (0.6):**
- `Value::Principal` variant removed
- `DataTypeKind::Principal` variant removed
- `Principal` gets `impl CustomDataType` with `TYPE_TAG = "principal"`
- Table fields: `#[custom_type] pub owner: Principal`

Binary compatibility: the `Encode` implementation for `Principal` stays identical, so bytes in
stable memory are unchanged. However, `TableFingerprint` changes (different `DataTypeKind`), so
existing schemas are incompatible. This is acceptable pre-1.0.

## Impact Summary

| Component | Change |
|-----------|--------|
| `CustomDataType` trait | New (extends `DataType`) |
| `CustomValue` struct | New |
| `Value` enum | +1 variant (`Custom`) |
| `DataTypeKind` | +1 variant, loses Serialize/Deserialize/CandidType derives |
| `CandidDataTypeKind` | New (API boundary mirror) |
| `CandidColumnDef` | Uses `CandidDataTypeKind` instead of `DataTypeKind` |
| `#[derive(Table)]` | Handles `#[custom_type]` annotation |
| `#[derive(CustomDataType)]` | New macro |
| `#[derive(Encode)]` | Untouched |
| `#[derive(DbmsCanister)]` | Untouched |
| Memory / pages / schema registry | Untouched |
| Filter / Query / DBMS engine | Untouched |
| `Principal` | Becomes `CustomDataType` impl (0.6) |

## Sequencing

Custom types are implemented first (0.6), then the wasm-dbms extraction (0.7). This ensures
the type system is extensible before the migration, and `Principal` moves cleanly out of the
generic layer.

## Wasm-dbms Migration Interaction

During the wasm-dbms extraction:
- `CustomDataType`, `CustomValue`, `Value`, `DataTypeKind` → `wasm-dbms-api`
- `CandidType` derives → behind `candid` feature flag on `wasm-dbms-api`
- `Principal` type + `CustomDataType` impl → `ic-dbms-canister` (or `ic-dbms-api`)
- `CandidDataTypeKind` → `wasm-dbms-api` (behind `candid` feature)

# Data Types Reference (IC)

> **Note:** This is the IC-specific data types reference. For the complete list of all data types, usage examples, and general documentation, see the [generic data types reference](../../reference/data-types.md).

- [Overview](#overview)
- [Principal Type](#principal-type)
  - [Usage](#usage)
  - [Common Patterns](#common-patterns)
- [Candid Type Mapping](#candid-type-mapping)
- [IC-Specific Considerations](#ic-specific-considerations)

---

## Overview

All wasm-dbms data types are available in ic-dbms through `ic_dbms_api::prelude::*` (which re-exports `wasm_dbms_api` types). This document covers the IC-specific aspects: the `Principal` type (which is unique to the Internet Computer) and the Candid type mappings used for canister API serialization.

---

## Principal Type

**Principal** is an Internet Computer-specific identifier type. It represents a canister ID, user identity, or the anonymous principal. This type is only meaningful in the IC context.

### Usage

```rust
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[custom_type]
    pub owner: Principal,  // IC principal who owns this record
}
```

**Creating principals:**

```rust
use candid::Principal;

// From text representation
let principal = Principal::from_text("aaaaa-aa").unwrap();

// Anonymous principal
let anon = Principal::anonymous();

// Caller principal (inside a canister)
let caller = ic_cdk::caller();

// Management canister
let mgmt = Principal::management_canister();
```

**Using in insert requests:**

```rust
let user = UserInsertRequest {
    id: 1.into(),
    owner: ic_cdk::caller(),  // Store the caller's principal
};

client.insert::<User>(User::table_name(), user, None).await??;
```

### Common Patterns

**Recording ownership:**

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "documents"]
pub struct Document {
    #[primary_key]
    pub id: Uuid,
    pub title: Text,
    #[custom_type]
    pub owner: Principal,      // Who created this
    #[custom_type]
    pub last_editor: Principal, // Who last modified this
}
```

**Filtering by principal:**

```rust
// Find all documents owned by the caller
let filter = Filter::eq("owner", ic_cdk::caller().into());
let query = Query::builder().filter(filter).build();
let my_docs = client.select::<Document>(Document::table_name(), query, None).await??;
```

**Nullable principal (optional ownership):**

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[custom_type]
    pub assignee: Nullable<Principal>,  // May be unassigned
}
```

---

## Candid Type Mapping

When ic-dbms generates the Candid interface (`.did` file) for your canister, each wasm-dbms type maps to a specific Candid type. This mapping is important for frontend integration, inter-canister calls, and using the Candid UI.

| ic-dbms Type | Rust Type | Candid Type | Notes |
|--------------|-----------|-------------|-------|
| `Uint8` | `u8` | `nat8` | |
| `Uint16` | `u16` | `nat16` | |
| `Uint32` | `u32` | `nat32` | |
| `Uint64` | `u64` | `nat64` | |
| `Int8` | `i8` | `int8` | |
| `Int16` | `i16` | `int16` | |
| `Int32` | `i32` | `int32` | |
| `Int64` | `i64` | `int64` | |
| `Decimal` | `rust_decimal::Decimal` | `text` | Serialized as string for precision |
| `Text` | `String` | `text` | |
| `Boolean` | `bool` | `bool` | |
| `Date` | `chrono::NaiveDate` | `record { year; month; day }` | Structured record |
| `DateTime` | `chrono::DateTime<Utc>` | `int64` | Unix timestamp |
| `Blob` | `Vec<u8>` | `blob` | |
| `Principal` | `candid::Principal` | `principal` | IC-specific |
| `Uuid` | `uuid::Uuid` | `text` | String representation |
| `Json` | `serde_json::Value` | `text` | Serialized JSON string |
| `Nullable<T>` | `Option<T>` | `opt T` | Candid optional |

**Frontend integration example (JavaScript/TypeScript):**

```typescript
// Calling from a frontend using @dfinity/agent
const user = await actor.select_users({
  filter: [{ Eq: ["name", { Text: "Alice" }] }],
  order_by: [],
  limit: [10n],  // nat64 maps to bigint
  columns: [],
  with_tables: [],
}, []);  // No transaction ID

// Principal values
import { Principal } from "@dfinity/principal";
const owner = Principal.fromText("aaaaa-aa");
```

---

## IC-Specific Considerations

**Re-exports:** `ic_dbms_api::prelude::*` re-exports all types from `wasm_dbms_api::prelude::*` plus IC-specific additions. You do not need to import `wasm_dbms_api` directly.

**CandidType requirement:** All data types used in your table schemas must implement `CandidType`. The built-in types already do. If you define [custom data types](../../guides/custom-data-types.md), they must also derive `CandidType`.

**Principal storage:** The `Principal` type is stored in binary format in stable memory (29 bytes max). It is serialized to/from its Candid `principal` representation when crossing canister boundaries.

**Decimal precision:** The `Decimal` type is serialized as `text` in Candid to preserve arbitrary precision. Frontends should parse the string representation rather than using floating-point conversion.

# Custom Data Types

- [Custom Data Types](#custom-data-types)
  - [Overview](#overview)
  - [Defining a Custom Type](#defining-a-custom-type)
    - [Step 1: Define the Type](#step-1-define-the-type)
    - [Step 2: Implement Display](#step-2-implement-display)
    - [Step 3: Implement Encode](#step-3-implement-encode)
    - [Step 4: Implement DataType and Derive CustomDataType](#step-4-implement-datatype-and-derive-customdatatype)
  - [Using Custom Types in Tables](#using-custom-types-in-tables)
    - [The custom\_type Attribute](#the-custom_type-attribute)
    - [Nullable Custom Types](#nullable-custom-types)
  - [Filtering and Querying](#filtering-and-querying)
  - [Ordering Contract](#ordering-contract)
  - [Examples](#examples)
    - [Enum: Priority](#enum-priority)
    - [Struct: Address](#struct-address)

---

## Overview

wasm-dbms ships with a set of [built-in data types](../reference/data-types.md) that cover the most common use cases. When your domain requires types that go beyond those built-ins, you can define **custom data types**.

Custom data types let you store any Rust type -- enums, newtypes, structs -- inside your tables. The DBMS engine stores them as opaque bytes internally and uses a **type tag** string to identify each custom type.

**When to use custom types:**

- Domain-specific enums (e.g., `Priority`, `Status`, `Role`)
- Composite value objects (e.g., `Address`, `Coordinates`)
- Newtypes that wrap primitives with domain meaning (e.g., `Email(String)`)

---

## Defining a Custom Type

Creating a custom type requires four steps:

1. Define the type with the required derives
2. Implement `Display`
3. Implement `Encode` (binary serialization)
4. Implement `DataType` and derive `CustomDataType`

### Step 1: Define the Type

Your type must derive or implement several traits. For enums, all must be implemented manually or derived:

```rust
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Default, Serialize, Deserialize,
)]
pub enum Priority {
    #[default]
    Low,
    Medium,
    High,
}
```

For structs, the same traits are required:

```rust
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord,
    Hash, Default, Serialize, Deserialize,
)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub zip: String,
}
```

**Required traits:**

| Trait | Purpose |
|-------|---------|
| `Clone` | Cloning values |
| `Debug` | Debug formatting |
| `PartialEq`, `Eq` | Equality comparison |
| `PartialOrd`, `Ord` | Ordering (for sorting and range filters) |
| `Hash` | Hashing (for hash-based lookups) |
| `Default` | Default value construction |
| `Serialize`, `Deserialize` | Serde serialization |
| `Display` | Human-readable display (see Step 2) |
| `Encode` | Binary encoding for storage (see Step 3) |

> **Note:** For IC canister usage, also derive `CandidType` and `Deserialize` from the `candid` crate.

### Step 2: Implement Display

The `Display` implementation provides a human-readable representation used for logging and diagnostics:

```rust
use std::fmt;

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Priority::Low => write!(f, "low"),
            Priority::Medium => write!(f, "medium"),
            Priority::High => write!(f, "high"),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, {} {}", self.street, self.city, self.zip)
    }
}
```

### Step 3: Implement Encode

The `Encode` trait defines how your type is serialized to and from bytes for memory storage. Enums require a manual implementation; for structs, you can use `#[derive(Encode)]`.

**Enum (manual implementation):**

```rust
use std::borrow::Cow;
use wasm_dbms_api::prelude::*;

impl Encode for Priority {
    const SIZE: DataSize = DataSize::Fixed(1);
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&self) -> Cow<'_, [u8]> {
        Cow::Owned(vec![match self {
            Priority::Low => 0,
            Priority::Medium => 1,
            Priority::High => 2,
        }])
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
        match data[0] {
            0 => Ok(Priority::Low),
            1 => Ok(Priority::Medium),
            2 => Ok(Priority::High),
            other => Err(MemoryError::DecodeError(
                DecodeError::TryFromSliceError(
                    format!("invalid Priority byte: {other}"),
                ),
            )),
        }
    }

    fn size(&self) -> MSize {
        1
    }
}
```

**Struct (derive macro):**

The `#[derive(Encode)]` macro works for structs whose fields all implement `Encode`. Since `String` does not implement `Encode` but `Text` does, use wasm-dbms types for the struct fields:

```rust
use wasm_dbms_api::prelude::*;

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord,
    Hash, Default, Serialize, Deserialize,
    Encode,
)]
pub struct Address {
    pub street: Text,
    pub city: Text,
    pub zip: Text,
}
```

**Key `Encode` concepts:**

| Constant | Description |
|----------|-------------|
| `DataSize::Fixed(n)` | Type always encodes to exactly `n` bytes |
| `DataSize::Dynamic` | Encoded size varies per value |
| `DEFAULT_ALIGNMENT` | Default memory page alignment (32 bytes) |

### Step 4: Implement DataType and Derive CustomDataType

Finally, implement the `DataType` marker trait and derive `CustomDataType` with a unique type tag:

```rust
use wasm_dbms_api::prelude::*;

impl DataType for Priority {}

// Manual CustomDataType implementation for enums
impl CustomDataType for Priority {
    const TYPE_TAG: &'static str = "priority";
}

// Manual From<Priority> for Value implementation
impl From<Priority> for Value {
    fn from(val: Priority) -> Value {
        Value::Custom(CustomValue {
            type_tag: <Priority as CustomDataType>::TYPE_TAG.to_string(),
            encoded: Encode::encode(&val).into_owned(),
            display: val.to_string(),
        })
    }
}
```

For structs, you can use the `CustomDataType` derive macro instead of the manual implementation above:

```rust
use wasm_dbms_api::prelude::*;

impl DataType for Address {}

#[derive(CustomDataType)]
#[type_tag = "address"]
pub struct Address {
    // ...
}
```

The `#[derive(CustomDataType)]` macro generates both the `CustomDataType` trait implementation and the `From<T> for Value` conversion. For enums, you must write these implementations manually.

**Type tag rules:**

- Must be unique across all custom types in your database
- Must be stable across upgrades (changing it makes existing data unreadable)
- Use lowercase, descriptive names (e.g., `"priority"`, `"address"`, `"role"`)

---

## Using Custom Types in Tables

### The custom_type Attribute

To use a custom type in a table, annotate the field with `#[custom_type]`:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[custom_type]
    pub priority: Priority,
    #[custom_type]
    pub address: Address,
}
```

Without the `#[custom_type]` attribute, the `Table` macro won't know how to handle your type and compilation will fail.

### Nullable Custom Types

Custom types can be wrapped in `Nullable<T>` for optional fields:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[custom_type]
    pub priority: Nullable<Priority>,
}
```

When `Nullable::Null`, the value is stored as `Value::Null`. When `Nullable::Value(v)`, it is stored as `Value::Custom(...)`.

---

## Filtering and Querying

To filter on custom type fields, construct a `Value::Custom` with the appropriate `CustomValue`:

```rust
use wasm_dbms_api::prelude::*;

// Create a filter for Priority::High
let high_priority = Priority::High;
let filter = Filter::eq("priority", high_priority.into());

// You can also construct the Value manually
let filter = Filter::eq("priority", Value::Custom(CustomValue {
    type_tag: "priority".to_string(),
    encoded: Encode::encode(&Priority::High).into_owned(),
    display: "high".to_string(),
}));
```

To extract a custom type from a `Value`:

```rust
let value: Value = Priority::High.into();

// Get the raw CustomValue
if let Some(cv) = value.as_custom() {
    println!("type: {}, display: {}", cv.type_tag, cv.display);
}

// Decode into the concrete type
if let Some(priority) = value.as_custom_type::<Priority>() {
    println!("Priority: {priority}");
}
```

---

## Ordering Contract

Custom types support all filter operations: `Eq`, `Ne`, `In`, `Gt`, `Lt`, `Ge`, `Le`.

For **equality filters** (`Eq`, `Ne`, `In`), the only requirement is that the `Encode` implementation produces canonical output -- the same value always encodes to the same bytes.

For **range filters** (`Gt`, `Lt`, `Ge`, `Le`) and `ORDER BY`, the encoding must be **order-preserving**: if `a < b` according to `Ord`, then `a.encode() < b.encode()` lexicographically. This is because the DBMS compares custom values by their encoded bytes.

**Example of order-preserving encoding:**

The `Priority` enum above encodes `Low = 0`, `Medium = 1`, `High = 2`. Since `Low < Medium < High` in the `Ord` implementation and `[0] < [1] < [2]` lexicographically, range filters work correctly.

> **Warning:** If your encoding is not order-preserving, equality filters will still work, but range filters and sorting will produce incorrect results.

---

## Examples

### Enum: Priority

A complete example of a custom enum type used in a table:

```rust
use std::borrow::Cow;
use std::fmt;

use serde::{Deserialize, Serialize};
use wasm_dbms_api::prelude::*;

// 1. Define the type
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord,
    Hash, Default, Serialize, Deserialize,
)]
pub enum Priority {
    #[default]
    Low,
    Medium,
    High,
}

// 2. Implement Display
impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Priority::Low => write!(f, "low"),
            Priority::Medium => write!(f, "medium"),
            Priority::High => write!(f, "high"),
        }
    }
}

// 3. Implement Encode (manual for enums)
impl Encode for Priority {
    const SIZE: DataSize = DataSize::Fixed(1);
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&self) -> Cow<'_, [u8]> {
        Cow::Owned(vec![match self {
            Priority::Low => 0,
            Priority::Medium => 1,
            Priority::High => 2,
        }])
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self> {
        match data[0] {
            0 => Ok(Priority::Low),
            1 => Ok(Priority::Medium),
            2 => Ok(Priority::High),
            other => Err(MemoryError::DecodeError(
                DecodeError::TryFromSliceError(
                    format!("invalid Priority byte: {other}"),
                ),
            )),
        }
    }

    fn size(&self) -> MSize {
        1
    }
}

// 4. Implement DataType + CustomDataType + From<Priority> for Value
impl DataType for Priority {}

impl CustomDataType for Priority {
    const TYPE_TAG: &'static str = "priority";
}

impl From<Priority> for Value {
    fn from(val: Priority) -> Value {
        Value::Custom(CustomValue {
            type_tag: <Priority as CustomDataType>::TYPE_TAG.to_string(),
            encoded: Encode::encode(&val).into_owned(),
            display: val.to_string(),
        })
    }
}

// Use in a table
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    #[custom_type]
    pub priority: Priority,
}
```

### Struct: Address

A complete example of a custom struct type. Structs can use `#[derive(Encode)]` and `#[derive(CustomDataType)]`:

```rust
use std::fmt;

use serde::{Deserialize, Serialize};
use wasm_dbms_api::prelude::*;

// 1. Define the type with Encode and CustomDataType derives
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord,
    Hash, Default, Serialize, Deserialize,
    Encode, CustomDataType,
)]
#[type_tag = "address"]
pub struct Address {
    pub street: Text,
    pub city: Text,
    pub zip: Text,
}

// 2. Implement Display
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f, "{}, {} {}",
            self.street.as_str(),
            self.city.as_str(),
            self.zip.as_str(),
        )
    }
}

// 3. Implement DataType
impl DataType for Address {}

// Use in a table
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "customers"]
pub struct Customer {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[custom_type]
    pub address: Address,
}
```

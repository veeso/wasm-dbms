# Data Types

- [Data Types](#data-types)
  - [Overview](#overview)
  - [Integer Types](#integer-types)
    - [Unsigned Integers](#unsigned-integers)
    - [Signed Integers](#signed-integers)
  - [Decimal](#decimal)
  - [Text](#text)
  - [Boolean](#boolean)
  - [Date and Time](#date-and-time)
    - [Date](#date)
    - [DateTime](#datetime)
  - [Binary Data](#binary-data)
    - [Blob](#blob)
  - [Identifiers](#identifiers)
    - [Uuid](#uuid)
  - [Semi-Structured Data](#semi-structured-data)
    - [Json](#json)
  - [Nullable](#nullable)
  - [Custom Types](#custom-types)
  - [Type Conversion Reference](#type-conversion-reference)

---

## Overview

wasm-dbms provides a rich set of data types for defining table schemas. Each type maps to standard Rust types for seamless integration.

**Type categories:**

| Category        | Types                                                    |
| --------------- | -------------------------------------------------------- |
| Integers        | Uint8, Uint16, Uint32, Uint64, Int8, Int16, Int32, Int64 |
| Decimal         | Decimal                                                  |
| Text            | Text                                                     |
| Boolean         | Boolean                                                  |
| Date/Time       | Date, DateTime                                           |
| Binary          | Blob                                                     |
| Identifiers     | Uuid                                                     |
| Semi-structured | Json                                                     |
| Wrapper         | Nullable\<T\>                                            |

> **Note:** The `Principal` type is available in `ic-dbms-api` for Internet Computer integration. See the [IC Data Types](../ic/reference/data-types.md) reference for details.

---

## Integer Types

### Unsigned Integers

**Uint8** - 8-bit unsigned integer (0 to 255)

```rust
use wasm_dbms_api::prelude::Uint8;

#[derive(Table, ...)]
#[table = "settings"]
pub struct Setting {
    #[primary_key]
    pub id: Uint32,
    pub priority: Uint8,  // 0-255
}

// Usage
let setting = SettingInsertRequest {
    id: 1.into(),
    priority: 10.into(),  // or Uint8::from(10)
};
```

**Uint16** - 16-bit unsigned integer (0 to 65,535)

```rust
use wasm_dbms_api::prelude::Uint16;

pub struct Product {
    pub stock_count: Uint16,  // 0-65,535
}

let count: Uint16 = 1000.into();
```

**Uint32** - 32-bit unsigned integer (0 to 4,294,967,295)

```rust
use wasm_dbms_api::prelude::Uint32;

pub struct User {
    #[primary_key]
    pub id: Uint32,  // Common for primary keys
}

let id: Uint32 = 12345.into();
```

**Uint64** - 64-bit unsigned integer (0 to 18,446,744,073,709,551,615)

```rust
use wasm_dbms_api::prelude::Uint64;

pub struct Transaction {
    pub amount_e8s: Uint64,  // For large numbers like token amounts
}

let amount: Uint64 = 1_000_000_000u64.into();
```

### Signed Integers

**Int8** - 8-bit signed integer (-128 to 127)

```rust
use wasm_dbms_api::prelude::Int8;

pub struct Temperature {
    pub celsius: Int8,  // -128 to 127
}

let temp: Int8 = (-10).into();
```

**Int16** - 16-bit signed integer (-32,768 to 32,767)

```rust
use wasm_dbms_api::prelude::Int16;

pub struct Altitude {
    pub meters: Int16,  // Can be negative (below sea level)
}

let altitude: Int16 = (-100).into();
```

**Int32** - 32-bit signed integer (-2,147,483,648 to 2,147,483,647)

```rust
use wasm_dbms_api::prelude::Int32;

pub struct Account {
    pub balance_cents: Int32,  // Can be negative (debt)
}

let balance: Int32 = (-5000).into();
```

**Int64** - 64-bit signed integer

```rust
use wasm_dbms_api::prelude::Int64;

pub struct Statistics {
    pub total_change: Int64,  // Large signed values
}

let change: Int64 = (-1_000_000_000i64).into();
```

---

## Decimal

**Decimal** - Arbitrary-precision decimal number

```rust
use wasm_dbms_api::prelude::Decimal;

pub struct Product {
    pub price: Decimal,      // $19.99
    pub weight_kg: Decimal,  // 2.5
}

// From f64
let price: Decimal = 19.99.into();

// From string (more precise)
let price: Decimal = "19.99".parse().unwrap();

// With sanitizer for rounding
#[derive(Table, ...)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(RoundToScaleSanitizer(2))]  // Round to 2 decimal places
    pub price: Decimal,
}
```

**Note:** Use `RoundToScaleSanitizer` to ensure consistent decimal precision.

---

## Text

**Text** - UTF-8 string

```rust
use wasm_dbms_api::prelude::Text;

pub struct User {
    pub name: Text,
    pub email: Text,
    pub bio: Text,
}

// From &str
let name: Text = "Alice".into();

// From String
let email: Text = String::from("alice@example.com").into();

// Access the string
let text: Text = "Hello".into();
assert_eq!(text.as_str(), "Hello");
```

**With validation:**

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
    #[validate(EmailValidator)]
    pub email: Text,
}
```

**With sanitization:**

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(CollapseWhitespaceSanitizer)]
    pub name: Text,
    #[sanitizer(LowerCaseSanitizer)]
    pub email: Text,
}
```

---

## Boolean

**Boolean** - True or false value

```rust
use wasm_dbms_api::prelude::Boolean;

pub struct User {
    pub is_active: Boolean,
    pub email_verified: Boolean,
}

let active: Boolean = true.into();
let verified: Boolean = false.into();

// Convert back
let value: bool = active.into();
```

**Filtering by boolean:**

```rust
// Find active users
let filter = Filter::eq("is_active", Value::Boolean(true));

// Find unverified users
let filter = Filter::eq("email_verified", Value::Boolean(false));
```

---

## Date and Time

### Date

**Date** - Calendar date (year, month, day)

```rust
use wasm_dbms_api::prelude::Date;

pub struct Event {
    pub event_date: Date,
}

// Create from components
let date = Date::new(2024, 6, 15);  // June 15, 2024

// From chrono NaiveDate (if using chrono)
use chrono::NaiveDate;
let naive = NaiveDate::from_ymd_opt(2024, 6, 15).unwrap();
let date: Date = naive.into();
```

### DateTime

**DateTime** - Date and time with timezone

```rust
use wasm_dbms_api::prelude::DateTime;

pub struct User {
    pub created_at: DateTime,
    pub last_login: DateTime,
}

// Current time
let now = DateTime::now();

// From chrono DateTime<Utc>
use chrono::{DateTime as ChronoDateTime, Utc};
let chrono_dt: ChronoDateTime<Utc> = Utc::now();
let dt: DateTime = chrono_dt.into();
```

**With sanitization:**

```rust
#[derive(Table, ...)]
#[table = "events"]
pub struct Event {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(UtcSanitizer)]  // Convert to UTC
    pub scheduled_at: DateTime,
}
```

---

## Binary Data

### Blob

**Blob** - Binary large object (byte array)

```rust
use wasm_dbms_api::prelude::Blob;

pub struct Document {
    pub content: Blob,      // File content
    pub thumbnail: Blob,    // Image data
}

// From Vec<u8>
let data: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47];  // PNG header
let blob: Blob = data.into();

// From slice
let blob: Blob = Blob::from(&[1, 2, 3, 4][..]);

// Access bytes
let bytes: &[u8] = blob.as_slice();
```

**Note:** Be mindful of storage costs when storing large blobs. Consider storing only references (hashes, URLs) for very large files.

---

## Identifiers

### Uuid

**Uuid** - Universally unique identifier (128-bit)

```rust
use wasm_dbms_api::prelude::Uuid;

pub struct Order {
    #[primary_key]
    pub id: Uuid,  // UUID as primary key
}

// Generate new UUID
let id = Uuid::new_v4();

// From string
let id: Uuid = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();

// From bytes
let bytes: [u8; 16] = [/* 16 bytes */];
let id = Uuid::from_bytes(bytes);
```

**Benefits over sequential IDs:**

- Globally unique without coordination
- No sequential guessing
- Safe for distributed systems

---

## Semi-Structured Data

### Json

**Json** - JSON object or array

```rust
use wasm_dbms_api::prelude::Json;
use std::str::FromStr;

pub struct User {
    pub metadata: Json,    // Flexible schema
    pub preferences: Json, // User settings
}

// From string
let json = Json::from_str(r#"{"theme": "dark", "language": "en"}"#).unwrap();

// From serde_json::Value
use serde_json::json;
let json: Json = json!({
    "notifications": true,
    "timezone": "UTC"
}).into();
```

**Querying JSON:**

```rust
// Check if JSON contains pattern
let filter = Filter::json("metadata", JsonFilter::contains(
    Json::from_str(r#"{"active": true}"#).unwrap()
));

// Extract and compare
let filter = Filter::json("preferences",
    JsonFilter::extract_eq("theme", Value::Text("dark".into()))
);

// Check path exists
let filter = Filter::json("metadata", JsonFilter::has_key("email"));
```

See the [JSON Reference](./json.md) for comprehensive JSON documentation.

---

## Nullable

**Nullable\<T\>** - Optional value wrapper

```rust
use wasm_dbms_api::prelude::{Nullable, Text, Uint32};

pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub phone: Nullable<Text>,      // Optional phone number
    pub age: Nullable<Uint32>,      // Optional age
    pub bio: Nullable<Text>,        // Optional biography
}

// Insert with value
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    phone: Nullable::Value("555-1234".into()),
    age: Nullable::Null,
    bio: Nullable::Null,
};

// Check if null
let phone = user.phone;
match phone {
    Nullable::Value(p) => println!("Phone: {}", p.as_str()),
    Nullable::Null => println!("No phone number"),
}
```

**Filtering nullable fields:**

```rust
// Find users with phone numbers
let filter = Filter::not_null("phone");

// Find users without phone numbers
let filter = Filter::is_null("phone");

// Find users with specific phone
let filter = Filter::eq("phone", Value::Text("555-1234".into()));
```

**Nullable foreign keys:**

```rust
pub struct Employee {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[foreign_key(entity = "Employee", table = "employees", column = "id")]
    pub manager_id: Nullable<Uint32>,  // Top-level employees have no manager
}
```

---

## Custom Types

Beyond the built-in types listed above, wasm-dbms supports **user-defined custom data types**. Custom types let you store enums, structs, and newtypes in your tables by implementing the `CustomDataType` trait.

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    #[custom_type]
    pub priority: Priority,  // User-defined custom type
}
```

See the [Custom Data Types Guide](../guides/custom-data-types.md) for step-by-step instructions on defining and using custom types.

---

## Type Conversion Reference

| wasm-dbms Type | Rust Type               |
| -------------- | ----------------------- |
| `Uint8`        | `u8`                    |
| `Uint16`       | `u16`                   |
| `Uint32`       | `u32`                   |
| `Uint64`       | `u64`                   |
| `Int8`         | `i8`                    |
| `Int16`        | `i16`                   |
| `Int32`        | `i32`                   |
| `Int64`        | `i64`                   |
| `Decimal`      | `rust_decimal::Decimal` |
| `Text`         | `String`                |
| `Boolean`      | `bool`                  |
| `Date`         | `chrono::NaiveDate`     |
| `DateTime`     | `chrono::DateTime<Utc>` |
| `Blob`         | `Vec<u8>`               |
| `Uuid`         | `uuid::Uuid`            |
| `Json`         | `serde_json::Value`     |
| `Nullable<T>`  | `Option<T>`             |

> **Note:** For IC canister usage, these types also map to Candid types. See the [IC Data Types](../ic/reference/data-types.md) reference for the Candid mapping.

**Conversion examples:**

```rust
// Rust primitive to wasm-dbms type
let uint: Uint32 = 42u32.into();
let text: Text = "hello".into();
let boolean: Boolean = true.into();

// wasm-dbms type to Rust primitive
let num: u32 = uint.into();
let s: String = text.into();
let b: bool = boolean.into();
```

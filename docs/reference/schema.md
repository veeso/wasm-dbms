# Schema Definition

- [Schema Definition](#schema-definition)
  - [Overview](#overview)
  - [Table Definition](#table-definition)
    - [Required Derives](#required-derives)
    - [Table Attribute](#table-attribute)
  - [Column Attributes](#column-attributes)
    - [Primary Key](#primary-key)
    - [Unique](#unique)
    - [Index](#index)
    - [Foreign Key](#foreign-key)
    - [Custom Type](#custom-type)
    - [Sanitizer](#sanitizer)
    - [Validate](#validate)
    - [Candid](#candid)
    - [Alignment](#alignment)
  - [Generated Types](#generated-types)
    - [Record Type](#record-type)
    - [InsertRequest Type](#insertrequest-type)
    - [UpdateRequest Type](#updaterequest-type)
    - [ForeignFetcher Type](#foreignfetcher-type)
  - [Complete Example](#complete-example)
  - [Best Practices](#best-practices)

---

## Overview

wasm-dbms schemas are defined entirely in Rust using derive macros and attributes. Each struct represents a database table, and each field represents a column.

**Key concepts:**

- Structs with `#[derive(Table)]` become database tables
- Fields become columns with their types
- Attributes configure primary keys, foreign keys, validation, and more

---

## Table Definition

### Required Derives

Every table struct must have these derives:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}
```

| Derive | Required | Purpose |
|--------|----------|---------|
| `Table` | Yes | Generates table schema and related types |
| `Clone` | Yes | Required by the macro system |
| `Debug` | Recommended | Useful for debugging |
| `PartialEq`, `Eq` | Recommended | Useful for comparisons in tests |

> **Note:** For IC canister usage, also add `CandidType` and `Deserialize` derives plus the `#[candid]` attribute. See the [IC Schema Reference](../ic/reference/schema.md).

### Table Attribute

The `#[table = "name"]` attribute specifies the table name in the database:

```rust
#[derive(Table, ...)]
#[table = "user_accounts"]  // Table name in database
pub struct UserAccount {    // Rust struct name (can differ)
    // ...
}
```

**Naming conventions:**

- Use `snake_case` for table names
- Table names should be plural (e.g., `users`, `posts`, `order_items`)
- Keep names short but descriptive

---

## Column Attributes

### Primary Key

Every table must have exactly one primary key:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,  // Primary key
    pub name: Text,
}
```

**Primary key rules:**

- Exactly one field must be marked with `#[primary_key]`
- Primary keys must be unique across all records
- Primary keys cannot be null
- Common types: `Uint32`, `Uint64`, `Uuid`, `Text`

**UUID as primary key:**

```rust
#[derive(Table, ...)]
#[table = "orders"]
pub struct Order {
    #[primary_key]
    pub id: Uuid,  // UUID primary key
    pub total: Decimal,
}
```

### Unique

Enforce uniqueness on a column:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[unique]
    pub email: Text,  // Must be unique across all rows

    pub name: Text,
}
```

**Unique constraint rules:**

- Insert and update operations that would create a duplicate value return a `UniqueConstraintViolation` error
- Multiple fields in the same table can each be marked `#[unique]` independently
- A `#[unique]` field automatically gets a B+ tree index -- no separate `#[index]` annotation is needed
- Primary keys are always unique by definition; you don't need `#[unique]` on a `#[primary_key]` field

**Combining with other attributes:**

```rust
#[unique]
#[sanitizer(TrimSanitizer)]
#[sanitizer(LowerCaseSanitizer)]
#[validate(EmailValidator)]
pub email: Text,  // Sanitized, validated, then checked for uniqueness
```

> **Note:** Sanitization and validation run before the uniqueness check, so the sanitized value is what gets compared.

### Index

Define indexes on columns for faster lookups:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[index]
    pub email: Text,  // Single-column index

    pub name: Text,
}
```

**The primary key is always an implicit index** -- you don't need to add `#[index]` to it.

**Composite indexes:**

Use `group` to group multiple fields into a single composite index:

```rust
#[derive(Table, ...)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uint32,

    #[index(group = "category_brand")]
    pub category: Text,

    #[index(group = "category_brand")]
    pub brand: Text,

    pub name: Text,
}
```

Fields sharing the same `group` name form a composite index, with columns ordered by field declaration order. In the example above, the composite index covers `(category, brand)`.

**Syntax variants:**

```rust
// Single-column index
#[index]

// Composite index (group multiple fields by name)
#[index(group = "group_name")]
```

### Foreign Key

Define relationships between tables:

```rust
#[derive(Table, ...)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,

    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}
```

**Attribute parameters:**

| Parameter | Description |
|-----------|-------------|
| `entity` | Rust struct name of the referenced table |
| `table` | Table name (from `#[table = "..."]`) |
| `column` | Column name in the referenced table |

**Nullable foreign key:**

```rust
#[foreign_key(entity = "User", table = "users", column = "id")]
pub manager_id: Nullable<Uint32>,  // Can be null
```

**Self-referential foreign key:**

```rust
#[derive(Table, ...)]
#[table = "categories"]
pub struct Category {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,

    #[foreign_key(entity = "Category", table = "categories", column = "id")]
    pub parent_id: Nullable<Uint32>,
}
```

### Custom Type

Mark a field as a user-defined custom data type:

```rust
#[derive(Table, ...)]
#[table = "tasks"]
pub struct Task {
    #[primary_key]
    pub id: Uint32,
    #[custom_type]
    pub priority: Priority,  // User-defined type
}
```

The `#[custom_type]` attribute tells the `Table` macro that this field implements the `CustomDataType` trait. Without it, the macro won't know how to serialize and deserialize the field.

**Nullable custom types:**

```rust
#[custom_type]
pub priority: Nullable<Priority>,  // Optional custom type
```

See the [Custom Data Types Guide](../guides/custom-data-types.md) for how to define custom types.

### Sanitizer

Apply data transformations before storage:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[sanitizer(TrimSanitizer)]
    pub name: Text,

    #[sanitizer(LowerCaseSanitizer)]
    #[sanitizer(TrimSanitizer)]
    pub email: Text,

    #[sanitizer(RoundToScaleSanitizer(2))]
    pub balance: Decimal,

    #[sanitizer(ClampSanitizer, min = 0, max = 120)]
    pub age: Uint8,
}
```

**Syntax variants:**

```rust
// Unit struct (no parameters)
#[sanitizer(TrimSanitizer)]

// Tuple struct (positional parameter)
#[sanitizer(RoundToScaleSanitizer(2))]

// Named fields struct
#[sanitizer(ClampSanitizer, min = 0, max = 100)]
```

See [Sanitization Reference](./sanitization.md) for all available sanitizers.

### Validate

Add validation rules:

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

    #[validate(UrlValidator)]
    pub website: Nullable<Text>,
}
```

**Validation happens after sanitization:**

```rust
#[sanitizer(TrimSanitizer)]           // 1. First: trim whitespace
#[validate(MaxStrlenValidator(100))]  // 2. Then: check length
pub name: Text,
```

See [Validation Reference](./validation.md) for all available validators.

### Candid

Enable `CandidType` and `Deserialize` derives on generated types:

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}
```

When the `#[candid]` attribute is present, the `Table` macro adds `candid::CandidType`, `serde::Serialize`, and `serde::Deserialize` derives to the generated `Record`, `InsertRequest`, and `UpdateRequest` types.

**When to use:**

- Required for IC canister deployment where types must cross canister boundaries via Candid
- Any context where generated types need Candid serialization

> **Note:** The `#[candid]` attribute only affects the types *generated* by the `Table` macro. You still need to derive `CandidType` and `Deserialize` on the table struct itself.

See the [IC Schema Reference](../ic/reference/schema.md) for full IC integration details.

### Alignment

Advanced: Configure memory alignment for dynamic-size tables:

```rust
#[derive(Table, ...)]
#[table = "large_records"]
#[alignment = 64]  // 64-byte alignment
pub struct LargeRecord {
    #[primary_key]
    pub id: Uint32,
    pub data: Text,  // Variable-size field
}
```

**When to use:**

- Performance tuning for specific access patterns
- Optimizing memory layout for large records

**Rules:**

- Minimum alignment is 8 bytes for dynamic types
- Default alignment is 32 bytes
- Fixed-size tables ignore this attribute (alignment equals record size)

> **Caution:** Only change alignment if you understand the performance implications.

---

## Generated Types

The `Table` macro generates several types for each table.

### Record Type

`{StructName}Record` - The full record type returned from queries:

```rust
// Generated from User struct
pub struct UserRecord {
    pub id: Uint32,
    pub name: Text,
    pub email: Text,
}

// Usage
let users: Vec<UserRecord> = database.select::<User>(query)?;

for user in users {
    println!("{}: {}", user.id, user.name);
}
```

### InsertRequest Type

`{StructName}InsertRequest` - Request type for inserting records:

```rust
// Generated from User struct
pub struct UserInsertRequest {
    pub id: Uint32,
    pub name: Text,
    pub email: Text,
}

// Usage
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
};

database.insert::<User>(user)?;
```

### UpdateRequest Type

`{StructName}UpdateRequest` - Request type for updating records:

```rust
// Generated from User struct (with builder pattern)
let update = UserUpdateRequest::builder()
    .set_name("New Name".into())
    .set_email("new@example.com".into())
    .filter(Filter::eq("id", Value::Uint32(1.into())))
    .build();

// Usage
database.update::<User>(update)?;
```

**Builder methods:**

- `set_{field_name}(value)` - Set a field value
- `filter(Filter)` - WHERE clause (required)
- `build()` - Build the update request

### ForeignFetcher Type

`{StructName}ForeignFetcher` - Internal type for eager loading:

```rust
// Generated automatically, used internally
// You typically don't interact with this directly
```

---

## Complete Example

```rust
// schema/src/lib.rs
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,

    #[unique]
    #[sanitizer(TrimSanitizer)]
    #[sanitizer(LowerCaseSanitizer)]
    #[validate(EmailValidator)]
    pub email: Text,

    pub created_at: DateTime,

    pub is_active: Boolean,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uuid,

    #[validate(MaxStrlenValidator(200))]
    pub title: Text,

    pub content: Text,

    pub published: Boolean,

    #[index(group = "author_date")]
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,

    pub metadata: Nullable<Json>,

    #[index(group = "author_date")]
    pub created_at: DateTime,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "comments"]
pub struct Comment {
    #[primary_key]
    pub id: Uuid,

    #[validate(MaxStrlenValidator(1000))]
    pub content: Text,

    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,

    #[foreign_key(entity = "Post", table = "posts", column = "id")]
    pub post_id: Uuid,

    pub created_at: DateTime,
}
```

> For generating a complete IC canister API from this schema, see the [IC Schema Reference](../ic/reference/schema.md).

---

## Best Practices

**1. Keep schema in a separate crate**

```
my-project/
├── schema/           # Reusable types
│   ├── Cargo.toml
│   └── src/lib.rs
└── app/              # Application using the database
    ├── Cargo.toml
    └── src/lib.rs
```

**2. Use appropriate primary key types**

```rust
// Sequential IDs - simple, good for internal use
pub id: Uint32,

// UUIDs - better for distributed systems, no guessing
pub id: Uuid,
```

**3. Always validate user input**

```rust
#[validate(MaxStrlenValidator(1000))]  // Prevent huge strings
pub content: Text,

#[validate(EmailValidator)]  // Validate format
pub email: Text,
```

**4. Use nullable for optional fields**

```rust
pub phone: Nullable<Text>,  // Clearly optional
pub bio: Nullable<Text>,
```

**5. Consider sanitization for consistency**

```rust
#[sanitizer(TrimSanitizer)]
#[sanitizer(LowerCaseSanitizer)]
pub email: Text,  // Always lowercase, no whitespace
```

**6. Document your schema**

```rust
/// User account information
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    /// Unique user identifier
    #[primary_key]
    pub id: Uint32,

    /// User's display name (max 100 chars)
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
}
```

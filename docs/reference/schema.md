# Schema Definition

- [Schema Definition](#schema-definition)
  - [Overview](#overview)
  - [Table Definition](#table-definition)
    - [Required Derives](#required-derives)
    - [Table Attribute](#table-attribute)
  - [Column Attributes](#column-attributes)
    - [Primary Key](#primary-key)
    - [Autoincrement](#autoincrement)
    - [Unique](#unique)
    - [Index](#index)
    - [Foreign Key](#foreign-key)
    - [Custom Type](#custom-type)
    - [Sanitizer](#sanitizer)
    - [Validate](#validate)
    - [Candid](#candid)
    - [Alignment](#alignment)
  - [Migration Attributes](#migration-attributes)
    - [Default Value](#default-value)
    - [Renamed From](#renamed-from)
    - [Migrate Override](#migrate-override)
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

> [!WARNING]
> The schema snapshot format used for migration detection imposes hard limits on identifier lengths and table shape. Exceeding any of these will cause the snapshot encoder to truncate or panic at runtime:
>
> - **Table name**: at most **255 bytes** (UTF-8).
> - **Column name**: at most **255 bytes** (UTF-8). Applies to every column, including the primary key and any column referenced by an index or foreign key.
> - **Custom data type name**: at most **255 bytes** (UTF-8).
> - **Foreign key target** (table name and column name): each at most **255 bytes**.
> - **Columns per index**: at most **255**.
> - **Columns per table**: at most **65,535**.
> - **Indexes per table**: at most **65,535**.
>
> Pick short, `snake_case` identifiers. The 255-byte cap is well above any sensible name length, but binary identifiers or non-ASCII text can blow past it faster than expected because the limit is in **bytes**, not characters.

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

| Derive            | Required    | Purpose                                  |
| ----------------- | ----------- | ---------------------------------------- |
| `Table`           | Yes         | Generates table schema and related types |
| `Clone`           | Yes         | Required by the macro system             |
| `Debug`           | Recommended | Useful for debugging                     |
| `PartialEq`, `Eq` | Recommended | Useful for comparisons in tests          |

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

### Autoincrement

Automatically generate sequential values for a column on insert:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    #[autoincrement]
    pub id: Uint32,  // Automatically assigned 1, 2, 3, ...
    pub name: Text,
}
```

**Autoincrement rules:**

- Only integer types are supported: `Int8`, `Int16`, `Int32`, `Int64`, `Uint8`, `Uint16`, `Uint32`, `Uint64`
- The counter starts at zero and increments by one on each insert
- Each autoincrement column has an independent counter
- Counters persist across canister upgrades (stored in stable memory)
- When the counter reaches the type's maximum value, inserts return an `AutoincrementOverflow` error
- Deleted records do **not** recycle their autoincrement values
- A table can have multiple `#[autoincrement]` columns

**Choosing the right type:**

| Type     | Max Records       |
| -------- | ----------------- |
| `Uint32` | ~4.3 billion      |
| `Uint64` | ~18.4 quintillion |
| `Int32`  | ~2.1 billion      |
| `Int64`  | ~9.2 quintillion  |

> **Tip:** `Uint64` is recommended for most use cases. Only use smaller types when storage space is critical and you are certain the record count will stay within bounds.

**Combining with other attributes:**

```rust
#[primary_key]
#[autoincrement]
pub id: Uint64,  // Auto-generated unique primary key
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

| Parameter | Description                              |
| --------- | ---------------------------------------- |
| `entity`  | Rust struct name of the referenced table |
| `table`   | Table name (from `#[table = "..."]`)     |
| `column`  | Column name in the referenced table      |

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

## Migration Attributes

These attributes feed the schema migration subsystem. They produce no runtime behaviour for normal CRUD; the planner only consults them when the compiled schema diverges from the snapshot stored in stable memory.

See the [Schema Migrations Guide](./migrations.md) for the end-to-end flow (drift detection, `plan_migration`, `migrate(policy)`).

### Default Value

Attach a per-column default that the migration planner uses when adding a non-nullable column to an existing table:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    pub name: Text,

    #[default = 0]
    pub login_count: Uint32,
}
```

**How it is used:**

- When `migrate()` plans an `AddColumn` op for a non-nullable column, it pulls the value from `#[default = ...]` (after first checking `Migrate::default_value`).
- Without a resolvable default, planning aborts with `MigrationError::MissingDefault`.

**Rules:**

- The expression must convert into the column's `Value` variant via `From`/`Into`. Examples: `#[default = 0]` on `Uint32`, `#[default = ""]` on `Text`, `#[default = false]` on `Boolean`.
- The expression is evaluated **at migration time**, not at insert time, so it has no effect on regular `INSERT` calls — those still need an explicit value (or omit the field if nullable).
- Custom data types must implement `From<MyType> for Value`; the `#[derive(CustomDataType)]` macro emits this automatically.
- Defaults are persisted into the table's snapshot (`ColumnSnapshot::default`), so the planner can compare them across releases.

**Combining with nullable:**

```rust
// Redundant — nullable columns default to NULL implicitly. Don't write
// #[default] on a Nullable<T> field.
pub bio: Nullable<Text>,
```

### Renamed From

Tell the migration planner that a column used to be known by one or more previous names:

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,

    #[renamed_from("username", "user_name")]
    pub name: Text,
}
```

**How it is used:**

When planning a migration, the planner first matches stored columns against compiled columns by name. For each compiled column with no direct match, it walks `renamed_from` in order and looks for a stored column with one of those names. The first hit is emitted as a `RenameColumn` op, preserving the column's data.

**Rules:**

- Entries are string literals.
- Order matters: list newer renames first, older renames last (mirroring the chronological order of releases).
- A stored column matched by `renamed_from` is **not** matched by another compiled column. If two compiled columns claim the same previous name, the earlier-declared field wins.
- Without `#[renamed_from]`, a column rename is indistinguishable from a `DropColumn` + `AddColumn` pair, which loses data.

### Migrate Override

By default, `#[derive(Table)]` emits an empty `impl Migrate for T {}` for every table, giving you trait defaults for `default_value` and `transform_column`. Add `#[migrate]` at the struct level to suppress that emission and provide a hand-written impl:

```rust
#[derive(Table, ...)]
#[table = "events"]
#[migrate]
pub struct Event {
    #[primary_key]
    pub id: Uint32,
    pub kind: Text,
    pub severity: Uint8,
}

impl Migrate for Event {
    fn default_value(column: &str) -> Option<Value> {
        match column {
            "severity" => Some(Value::Uint8(Uint8(1))),
            _ => None,
        }
    }

    fn transform_column(column: &str, old: Value) -> DbmsResult<Option<Value>> {
        match column {
            // Example: convert legacy text severities into the new Uint8 column.
            "severity" => match old {
                Value::Text(Text(s)) => match s.as_str() {
                    "low" => Ok(Some(Value::Uint8(Uint8(1)))),
                    "medium" => Ok(Some(Value::Uint8(Uint8(5)))),
                    "high" => Ok(Some(Value::Uint8(Uint8(9)))),
                    other => Err(DbmsError::Migration(MigrationError::TransformAborted {
                        table: "events".into(),
                        column: column.into(),
                        reason: format!("unknown severity `{other}`"),
                    })),
                },
                _ => Ok(None),
            },
            _ => Ok(None),
        }
    }
}
```

**When to use `#[migrate]`:**

- The new column is non-nullable and the default cannot be a constant literal (e.g. requires hashing the row, or pulls from another column).
- A column changed to an incompatible type that is not in the [widening whitelist](./migrations.md#compatible-widening-whitelist), and you can derive the new value from the old one.

**Trait contract:**

| Method                          | Returns       | Effect                                                                                                                   |
| ------------------------------- | ------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `default_value(column)`         | `Some(v)`     | Use `v` for `AddColumn` on `column`.                                                                                     |
| `default_value(column)`         | `None`        | Fall back to `#[default = ...]`, else `MigrationError::MissingDefault`.                                                  |
| `transform_column(column, old)` | `Ok(Some(v))` | Replace stored value with `v`.                                                                                           |
| `transform_column(column, old)` | `Ok(None)`    | No transform; framework errors with `MigrationError::IncompatibleType` unless the type change is a whitelisted widening. |
| `transform_column(column, old)` | `Err(_)`      | Abort the migration; the journaled session rolls back.                                                                   |

> **Note:** Without `#[migrate]`, do **not** write `impl Migrate for T {}` yourself — the macro already emitted one and you would get a duplicate-impl error.

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

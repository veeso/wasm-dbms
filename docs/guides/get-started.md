# Get Started

- [Prerequisites](#prerequisites)
- [Project Setup](#project-setup)
  - [Workspace Structure](#workspace-structure)
  - [Cargo Configuration](#cargo-configuration)
- [Define Your Schema](#define-your-schema)
  - [Create the Schema Crate](#create-the-schema-crate)
  - [Define Tables](#define-tables)
- [Define a Database Schema](#define-a-database-schema)
- [Using the Database](#using-the-database)
  - [Create a DbmsContext](#create-a-dbmscontext)
  - [Perform CRUD Operations](#perform-crud-operations)
- [Quick Example: Complete Workflow](#quick-example-complete-workflow)
- [Testing with HeapMemoryProvider](#testing-with-heapmemoryprovider)
- [Next Steps](#next-steps)

This guide walks you through setting up a database using wasm-dbms. By the end, you'll have a working database with CRUD operations and transactions.

---

## Prerequisites

Before starting, ensure you have:

- Rust 1.91.1 or later
- `wasm32-unknown-unknown` target: `rustup target add wasm32-unknown-unknown`

---

## Project Setup

### Workspace Structure

We recommend organizing your project as a Cargo workspace with a schema crate:

```
my-dbms-project/
├── Cargo.toml          # Workspace manifest
├── schema/             # Schema definitions (reusable types)
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs
└── app/                # Your application using the database
    ├── Cargo.toml
    └── src/
        └── lib.rs
```

**Workspace Cargo.toml:**

```toml
[workspace]
members = ["schema", "app"]
resolver = "2"
```

### Cargo Configuration

Create `.cargo/config.toml` to configure the `getrandom` crate for WebAssembly:

```toml
[target.wasm32-unknown-unknown]
rustflags = ['--cfg', 'getrandom_backend="custom"']
```

This is required because the `uuid` crate depends on `getrandom`.

---

## Define Your Schema

### Create the Schema Crate

Create `schema/Cargo.toml`:

```toml
[package]
name = "my-schema"
version = "0.1.0"
edition = "2024"

[dependencies]
wasm-dbms-api = "0.6"
```

### Define Tables

In `schema/src/lib.rs`, define your database tables using the `Table` derive macro:

```rust
use wasm_dbms_api::prelude::*;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(100))]
    pub name: Text,
    #[validate(EmailValidator)]
    pub email: Text,
    pub created_at: DateTime,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    #[validate(MaxStrlenValidator(200))]
    pub title: Text,
    pub content: Text,
    pub published: Boolean,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub author_id: Uint32,
}
```

**Required derives:** `Table`, `Clone`

The `Table` macro generates additional types for each table:

| Generated Type | Purpose |
|----------------|---------|
| `UserRecord` | Full record returned from queries |
| `UserInsertRequest` | Request type for inserting records |
| `UserUpdateRequest` | Request type for updating records |
| `UserForeignFetcher` | Internal type for relationship loading |

---

## Define a Database Schema

Once you've defined your tables, create a schema struct with `#[derive(DatabaseSchema)]` to wire them together:

```rust
use wasm_dbms::prelude::DatabaseSchema;

#[derive(DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct MySchema;
```

The `DatabaseSchema` derive macro auto-generates the `DatabaseSchema<M>` trait implementation and a `register_tables` method. This replaces what would otherwise be ~130+ lines of manual dispatch code.

---

## Using the Database

### Create a DbmsContext

The `DbmsContext` holds all database state. Create one using a `MemoryProvider`:

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;

// For testing, use HeapMemoryProvider
let ctx = DbmsContext::new(HeapMemoryProvider::default());

// Register tables from the schema
MySchema::register_tables(&ctx).expect("failed to register tables");
```

### Perform CRUD Operations

Create a `WasmDbmsDatabase` from the context to perform operations:

```rust
use wasm_dbms::prelude::*;
use my_schema::{User, UserInsertRequest};

// Create a one-shot (non-transactional) database
let database = WasmDbmsDatabase::oneshot(&ctx, MySchema);

// Insert a record
let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
    created_at: DateTime::now(),
};
database.insert::<User>(user)?;

// Query records
let query = Query::builder().all().build();
let users = database.select::<User>(query)?;
```

---

## Quick Example: Complete Workflow

Here's a complete example showing insert, query, update, and delete operations:

```rust
use wasm_dbms_api::prelude::*;
use my_schema::{User, UserInsertRequest, UserUpdateRequest};

fn example(database: &impl Database) -> Result<(), DbmsError> {
    // 1. INSERT a new user
    let insert_req = UserInsertRequest {
        id: 1.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        created_at: DateTime::now(),
    };
    database.insert::<User>(insert_req)?;

    // 2. SELECT users
    let query = Query::builder()
        .filter(Filter::eq("name", Value::Text("Alice".into())))
        .build();
    let users = database.select::<User>(query)?;
    println!("Found {} user(s)", users.len());

    // 3. UPDATE the user
    let update_req = UserUpdateRequest::builder()
        .set_email("alice.new@example.com".into())
        .filter(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let updated = database.update::<User>(update_req)?;
    println!("Updated {} record(s)", updated);

    // 4. DELETE the user
    let deleted = database.delete::<User>(
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(1.into()))),
    )?;
    println!("Deleted {} record(s)", deleted);

    Ok(())
}
```

---

## Testing with HeapMemoryProvider

For unit tests, use `HeapMemoryProvider` which stores data in heap memory:

```rust
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;
use my_schema::{User, UserInsertRequest, MySchema};

#[test]
fn test_insert_and_select() {
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    MySchema::register_tables(&ctx).expect("register failed");
    let database = WasmDbmsDatabase::oneshot(&ctx, MySchema);

    let insert_req = UserInsertRequest {
        id: 1.into(),
        name: "Test User".into(),
        email: "test@example.com".into(),
        created_at: DateTime::now(),
    };

    database.insert::<User>(insert_req).expect("insert failed");

    let query = Query::builder().all().build();
    let users = database.select::<User>(query).expect("select failed");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name.as_str(), "Test User");
}
```

> For deploying on the Internet Computer as a canister, see the [IC Getting Started Guide](../ic/guides/get-started.md).

---

## Next Steps

Now that you have a working database, explore these topics:

- [CRUD Operations](./crud-operations.md) - Detailed guide on all database operations
- [Querying](./querying.md) - Filters, ordering, pagination, and field selection
- [Transactions](./transactions.md) - ACID transactions with commit/rollback
- [Relationships](./relationships.md) - Foreign keys and eager loading
- [Custom Data Types](./custom-data-types.md) - Define your own data types (enums, structs)
- [Schema Definition](../reference/schema.md) - Complete schema reference
- [Data Types](../reference/data-types.md) - All supported field types

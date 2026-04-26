# CRUD Operations

- [CRUD Operations](#crud-operations)
  - [Overview](#overview)
  - [Insert](#insert)
    - [Basic Insert](#basic-insert)
    - [Handling Primary Keys](#handling-primary-keys)
    - [Nullable Fields](#nullable-fields)
    - [Insert with Transaction](#insert-with-transaction)
  - [Select](#select)
    - [Select All Records](#select-all-records)
    - [Select with Filter](#select-with-filter)
    - [Select Specific Columns](#select-specific-columns)
    - [Select with Eager Loading](#select-with-eager-loading)
  - [Update](#update)
    - [Basic Update](#basic-update)
    - [Partial Updates](#partial-updates)
    - [Update with Filter](#update-with-filter)
    - [Update Return Value](#update-return-value)
  - [Delete](#delete)
    - [Delete with Filter](#delete-with-filter)
    - [Delete Behaviors](#delete-behaviors)
    - [Delete All Records](#delete-all-records)
  - [Operations with Transactions](#operations-with-transactions)
  - [Error Handling](#error-handling)

---

## Overview

wasm-dbms provides four fundamental database operations through the `Database` trait:

| Operation  | Description                 | Returns                       |
| ---------- | --------------------------- | ----------------------------- |
| **Insert** | Add a new record to a table | `Result<()>`                  |
| **Select** | Query records from a table  | `Result<Vec<Record>>`         |
| **Update** | Modify existing records     | `Result<u64>` (affected rows) |
| **Delete** | Remove records from a table | `Result<u64>` (affected rows) |

All operations:
- Support optional transaction IDs
- Validate and sanitize data according to schema rules
- Enforce foreign key constraints

---

## Insert

### Basic Insert

To insert a record, create an `InsertRequest` and call the insert method:

```rust
use wasm_dbms_api::prelude::*;
use my_schema::{User, UserInsertRequest};

let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
    created_at: DateTime::now(),
};

// Insert without transaction
database.insert::<User>(user)?;
```

### Handling Primary Keys

Every table must have a primary key. Insert will fail if a record with the same primary key already exists:

```rust
// First insert succeeds
database.insert::<User>(user1)?;

// Second insert with same ID fails with PrimaryKeyConflict
let result = database.insert::<User>(user2_same_id);
assert!(matches!(result, Err(DbmsError::Query(QueryError::PrimaryKeyConflict))));
```

### Nullable Fields

For fields wrapped in `Nullable<T>`, you can insert either a value or null:

```rust
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "profiles"]
pub struct Profile {
    #[primary_key]
    pub id: Uint32,
    pub bio: Nullable<Text>,      // Optional field
    pub website: Nullable<Text>,  // Optional field
}

// Insert with value
let profile = ProfileInsertRequest {
    id: 1.into(),
    bio: Nullable::Value("Hello world".into()),
    website: Nullable::Null,  // No website
};

database.insert::<Profile>(profile)?;
```

### Insert with Transaction

To insert within a transaction, use a transactional database instance:

```rust
// Begin transaction
let tx_id = ctx.begin_transaction();

// Create a transactional database
let database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// Insert within transaction
database.insert::<User>(user)?;

// Commit or rollback
database.commit()?;
```

---

## Select

### Select All Records

Use `Query::builder().all()` to select all records:

```rust
use wasm_dbms_api::prelude::*;

let query = Query::builder().all().build();
let users: Vec<UserRecord> = database.select::<User>(query)?;

for user in users {
    println!("User: {} ({})", user.name, user.email);
}
```

### Select with Filter

Add filters to narrow down results:

```rust
// Select users with specific name
let query = Query::builder()
    .filter(Filter::eq("name", Value::Text("Alice".into())))
    .build();

let users = database.select::<User>(query)?;
```

See the [Querying Guide](./querying.md) for comprehensive filter documentation.

### Select Specific Columns

Select only the columns you need:

```rust
let query = Query::builder()
    .columns(vec!["id".to_string(), "name".to_string()])
    .build();

let users = database.select::<User>(query)?;
// Only id and name are populated; other fields have default values
```

### Select with Eager Loading

Load related records in a single query using `with()`:

```rust
// Load posts with their authors
let query = Query::builder()
    .all()
    .with("users")  // Eager load the related users table
    .build();

let posts = database.select::<Post>(query)?;
```

See the [Relationships Guide](./relationships.md) for more on eager loading.

---

## Update

### Basic Update

Create an `UpdateRequest` to modify records:

```rust
use my_schema::UserUpdateRequest;

let update = UserUpdateRequest::builder()
    .set_name("Alice Smith".into())
    .filter(Filter::eq("id", Value::Uint32(1.into())))
    .build();

let affected_rows = database.update::<User>(update)?;

println!("Updated {} row(s)", affected_rows);
```

### Partial Updates

Only specify the fields you want to change. Unspecified fields remain unchanged:

```rust
// Only update the email, keep everything else
let update = UserUpdateRequest::builder()
    .set_email("new.email@example.com".into())
    .filter(Filter::eq("id", Value::Uint32(1.into())))
    .build();

database.update::<User>(update)?;
```

### Update with Filter

The filter determines which records are updated:

```rust
// Update all users with a specific domain
let update = UserUpdateRequest::builder()
    .set_verified(true.into())
    .filter(Filter::like("email", "%@company.com"))
    .build();

let affected = database.update::<User>(update)?;
println!("Verified {} company users", affected);
```

### Update Return Value

Update returns the number of affected rows:

```rust
let affected = database.update::<User>(update)?;

if affected == 0 {
    println!("No records matched the filter");
} else {
    println!("Updated {} record(s)", affected);
}
```

---

## Delete

### Delete with Filter

Delete records matching a filter:

```rust
use wasm_dbms_api::prelude::DeleteBehavior;

let filter = Filter::eq("id", Value::Uint32(1.into()));

let deleted = database.delete::<User>(
    DeleteBehavior::Restrict,
    Some(filter),
)?;

println!("Deleted {} record(s)", deleted);
```

### Delete Behaviors

When deleting records that are referenced by foreign keys, you must specify a behavior:

| Behavior   | Description                                    |
| ---------- | ---------------------------------------------- |
| `Restrict` | Fail if any foreign keys reference this record |
| `Cascade`  | Delete all records that reference this record  |

**Restrict Example:**

```rust
// Will fail if any posts reference this user
let result = database.delete::<User>(
    DeleteBehavior::Restrict,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
);

match result {
    Ok(count) => println!("Deleted {} user(s)", count),
    Err(DbmsError::Query(QueryError::ForeignKeyConstraintViolation)) => {
        println!("Cannot delete: user has posts");
    }
    Err(e) => return Err(e),
}
```

**Cascade Example:**

```rust
// Deletes the user AND all their posts
database.delete::<User>(
    DeleteBehavior::Cascade,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
)?;
```

### Delete All Records

Pass `None` as the filter to delete all records (use with caution):

```rust
// Delete ALL users (respecting foreign key behavior)
let deleted = database.delete::<User>(
    DeleteBehavior::Cascade,
    None,  // No filter = all records
)?;

println!("Deleted all {} users and their related records", deleted);
```

---

## Operations with Transactions

All CRUD operations can be performed within a transaction. When using a transactional database instance, operations won't be visible to other callers until committed:

```rust
// Begin transaction
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// Perform operations within transaction
database.insert::<User>(user1)?;
database.insert::<User>(user2)?;

// Update within same transaction
let update = UserUpdateRequest::builder()
    .set_verified(true.into())
    .filter(Filter::all())
    .build();
database.update::<User>(update)?;

// Commit all changes atomically
database.commit()?;
```

See the [Transactions Guide](./transactions.md) for comprehensive transaction documentation.

---

## Error Handling

CRUD operations can fail for various reasons. Here are common errors:

| Error                           | Cause                                                 | Operation              |
| ------------------------------- | ----------------------------------------------------- | ---------------------- |
| `PrimaryKeyConflict`            | Record with same primary key exists                   | Insert                 |
| `ForeignKeyConstraintViolation` | Referenced record doesn't exist, or delete restricted | Insert, Update, Delete |
| `BrokenForeignKeyReference`     | Foreign key points to non-existent record             | Insert, Update         |
| `UnknownColumn`                 | Invalid column name in filter or select               | Select, Update, Delete |
| `MissingNonNullableField`       | Required field not provided                           | Insert, Update         |
| `RecordNotFound`                | No record matches the criteria                        | Update, Delete         |
| `TransactionNotFound`           | Invalid transaction ID                                | All                    |
| `InvalidQuery`                  | Malformed query (e.g., invalid JSON path)             | Select                 |

**Example error handling:**

```rust
use wasm_dbms_api::prelude::{DbmsError, QueryError};

let result = database.insert::<User>(user);

match result {
    Ok(()) => println!("Insert successful"),
    Err(DbmsError::Query(QueryError::PrimaryKeyConflict)) => {
        println!("User with this ID already exists");
    }
    Err(DbmsError::Query(QueryError::BrokenForeignKeyReference)) => {
        println!("Referenced record does not exist");
    }
    Err(DbmsError::Validation(msg)) => {
        println!("Validation failed: {}", msg);
    }
    Err(e) => {
        println!("Unexpected error: {:?}", e);
    }
}
```

See the [Errors Reference](../reference/errors.md) for complete error documentation.

> For IC canister client usage with the `IcDbmsCanisterClient`, see the [IC CRUD Guide](../ic/guides/crud-operations.md).

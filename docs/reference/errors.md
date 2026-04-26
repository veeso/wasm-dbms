# Errors Reference

- [Errors Reference](#errors-reference)
  - [Overview](#overview)
  - [Error Hierarchy](#error-hierarchy)
  - [DbmsError](#dbmserror)
  - [Query Errors](#query-errors)
    - [PrimaryKeyConflict](#primarykeyconflict)
    - [UniqueConstraintViolation](#uniqueconstraintviolation)
    - [BrokenForeignKeyReference](#brokenforeignkeyreference)
    - [ForeignKeyConstraintViolation](#foreignkeyconstraintviolation)
    - [UnknownColumn](#unknowncolumn)
    - [MissingNonNullableField](#missingnonnullablefield)
    - [RecordNotFound](#recordnotfound)
    - [InvalidQuery](#invalidquery)
  - [Transaction Errors](#transaction-errors)
    - [TransactionNotFound](#transactionnotfound)
  - [Validation Errors](#validation-errors)
  - [Sanitization Errors](#sanitization-errors)
  - [Memory Errors](#memory-errors)
  - [Error Handling Examples](#error-handling-examples)

---

## Overview

wasm-dbms uses a structured error system to provide clear information about what went wrong. Errors are categorized by their source:

| Category | Description |
| -------- | ----------- |
| Query | Database operation errors (constraints, missing data) |
| Transaction | Transaction state errors |
| Validation | Data validation failures |
| Sanitization | Data sanitization failures |
| Memory | Low-level memory errors |
| Table | Schema/table definition errors |

---

## Error Hierarchy

```txt
DbmsError
├── Query(QueryError)
│   ├── PrimaryKeyConflict
│   ├── UniqueConstraintViolation
│   ├── BrokenForeignKeyReference
│   ├── ForeignKeyConstraintViolation
│   ├── UnknownColumn
│   ├── MissingNonNullableField
│   ├── RecordNotFound
│   └── InvalidQuery
├── Transaction(TransactionError)
│   └── NotFound
├── Validation(String)
├── Sanitize(String)
├── Memory(MemoryError)
└── Table(TableError)
```

---

## DbmsError

The top-level error enum:

```rust
use wasm_dbms_api::prelude::DbmsError;

pub enum DbmsError {
    Memory(MemoryError),
    Query(QueryError),
    Table(TableError),
    Transaction(TransactionError),
    Sanitize(String),
    Validation(String),
}
```

**Matching on error types:**

```rust
match error {
    DbmsError::Query(query_err) => {
        // Handle query errors
    }
    DbmsError::Transaction(tx_err) => {
        // Handle transaction errors
    }
    DbmsError::Validation(msg) => {
        // Handle validation errors
        println!("Validation failed: {}", msg);
    }
    DbmsError::Sanitize(msg) => {
        // Handle sanitization errors
        println!("Sanitization failed: {}", msg);
    }
    DbmsError::Memory(mem_err) => {
        // Handle memory errors (rare)
    }
    DbmsError::Table(table_err) => {
        // Handle table errors (rare)
    }
}
```

---

## Query Errors

Query errors occur during database operations.

### PrimaryKeyConflict

**Cause:** Attempting to insert a record with a primary key that already exists.

```rust
// Insert first user
database.insert::<User>(UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    ..
})?;

// Insert second user with same ID - FAILS
let result = database.insert::<User>(UserInsertRequest {
    id: 1.into(),  // Same ID!
    name: "Bob".into(),
    ..
});

match result {
    Err(DbmsError::Query(QueryError::PrimaryKeyConflict)) => {
        println!("A user with this ID already exists");
    }
    _ => {}
}
```

**Solutions:**

- Use a unique primary key (e.g., UUID)
- Check if record exists before inserting
- Use upsert pattern (check, then insert or update)

### UniqueConstraintViolation

**Cause:** Attempting to insert or update a record with a value that violates a `#[unique]` constraint.

```rust
// Insert first user
database.insert::<User>(UserInsertRequest {
    id: 1.into(),
    email: "alice@example.com".into(),
    ..
})?;

// Insert second user with same email - FAILS
let result = database.insert::<User>(UserInsertRequest {
    id: 2.into(),
    email: "alice@example.com".into(),  // Duplicate!
    ..
});

match result {
    Err(DbmsError::Query(QueryError::UniqueConstraintViolation { field })) => {
        println!("Duplicate value on field: {}", field);
        // field == "email"
    }
    _ => {}
}
```

**Also triggered on update:**

```rust
// Update user 2's email to match user 1's email - FAILS
let result = database.update::<User>(
    UserUpdateRequest::from_values(
        &[(email_col, Value::Text("alice@example.com".into()))],
        Some(Filter::eq("id", Value::Uint32(2.into()))),
    ),
);
```

**Solutions:**

- Check if a record with the same value exists before inserting
- Use a different value

### BrokenForeignKeyReference

**Cause:** Foreign key references a record that doesn't exist.

```rust
// Insert post with non-existent author
let result = database.insert::<Post>(PostInsertRequest {
    id: 1.into(),
    title: "My Post".into(),
    author_id: 999.into(),  // User 999 doesn't exist!
    ..
});

match result {
    Err(DbmsError::Query(QueryError::BrokenForeignKeyReference)) => {
        println!("Referenced user does not exist");
    }
    _ => {}
}
```

**Solutions:**

- Ensure referenced record exists before inserting
- Create referenced record first in a transaction

### ForeignKeyConstraintViolation

**Cause:** Attempting to delete a record that is referenced by other records (with `Restrict` behavior).

```rust
// User has posts - cannot delete with Restrict
let result = database.delete::<User>(
    DeleteBehavior::Restrict,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
);

match result {
    Err(DbmsError::Query(QueryError::ForeignKeyConstraintViolation)) => {
        println!("Cannot delete: user has related records");
    }
    _ => {}
}
```

**Solutions:**

- Delete related records first
- Use `DeleteBehavior::Cascade` to delete related records automatically

### UnknownColumn

**Cause:** Referencing a column that doesn't exist in the table.

```rust
// Filter with wrong column name
let filter = Filter::eq("username", Value::Text("alice".into()));  // Column is "name", not "username"

let result = database.select::<User>(
    Query::builder().filter(filter).build(),
);

match result {
    Err(DbmsError::Query(QueryError::UnknownColumn)) => {
        println!("Column does not exist in table");
    }
    _ => {}
}
```

**Solutions:**

- Check column names in your schema
- Use IDE autocompletion with typed column names

### MissingNonNullableField

**Cause:** Required field not provided in insert/update.

```rust
// This typically happens at compile time with the generated types,
// but can occur if manually constructing requests or using dynamic queries
```

**Solutions:**

- Provide all required fields
- Use `Nullable<T>` for optional fields

### RecordNotFound

**Cause:** Operation targets a record that doesn't exist.

```rust
// Update non-existent record
let update = UserUpdateRequest::builder()
    .set_name("New Name".into())
    .filter(Filter::eq("id", Value::Uint32(999.into())))  // Doesn't exist
    .build();

let affected = database.update::<User>(update)?;

// affected == 0 indicates no records matched
if affected == 0 {
    println!("No records found to update");
}
```

**Note:** Update and delete operations return the count of affected rows. A count of 0 isn't necessarily an error but indicates no matches.

### InvalidQuery

**Cause:** Malformed query (invalid JSON path, bad filter syntax, etc.).

```rust
// Invalid JSON path
let filter = Filter::json("metadata", JsonFilter::has_key("user."));  // Trailing dot

let result = database.select::<User>(
    Query::builder().filter(filter).build(),
);

match result {
    Err(DbmsError::Query(QueryError::InvalidQuery)) => {
        println!("Query is malformed");
    }
    _ => {}
}
```

**Common causes:**

- Invalid JSON paths (trailing dots, unclosed brackets)
- Applying JSON filter to non-JSON column
- Type mismatches in comparisons
- Aggregate-specific:
  - `SUM` or `AVG` on non-numeric column
    (`"aggregate requires numeric column: '<col>'"`)
  - `HAVING` references unknown column or `agg{N}`
    (`"HAVING references unknown column or aggregate: '<col>'"`)
  - `ORDER BY` references unknown `agg{N}`
    (`"ORDER BY references unknown aggregate output: '<col>'"`)
  - `LIKE` or JSON filter inside `HAVING`
  - Joins or eager relations on `Database::aggregate`

### JoinInsideTypedSelect

**Cause:** A typed `Database::select::<T>` was called with a query that
contains joins. Joins must go through `select_join`.

### AggregateClauseInSelect

**Cause:** `group_by` or `having` was set on a non-aggregate select path
(`select`, `select_raw`, or `select_join`). Use
[`Database::aggregate`](./query.md#aggregate-types) instead — those clauses
have no meaning outside aggregation and are rejected to prevent silent data
loss.

```rust
let result = database.select::<User>(
    Query::builder().group_by(&["role"]).build(),
);

match result {
    Err(DbmsError::Query(QueryError::AggregateClauseInSelect)) => {
        // call database.aggregate::<User>(query, &aggregates) instead
    }
    _ => {}
}
```

---

## Transaction Errors

### TransactionNotFound

**Cause:** Invalid transaction ID or transaction already completed.

```rust
use wasm_dbms_api::prelude::{DbmsError, TransactionError};

match database.commit() {
    Err(DbmsError::Transaction(TransactionError::NoActiveTransaction)) => {
        println!("No active transaction to commit");
    }
    _ => {}
}
```

**Causes:**

- Transaction ID never existed
- Transaction was already committed
- Transaction was already rolled back

---

## Validation Errors

**Cause:** Data fails validation rules.

```rust
#[derive(Table, ...)]
#[table = "users"]
pub struct User {
    #[validate(EmailValidator)]
    pub email: Text,
}

// Insert with invalid email
let result = database.insert::<User>(UserInsertRequest {
    id: 1.into(),
    email: "not-an-email".into(),  // Invalid!
    ..
});

match result {
    Err(DbmsError::Validation(msg)) => {
        println!("Validation failed: {}", msg);
        // msg might be: "Invalid email format"
    }
    _ => {}
}
```

**Common validation errors:**

- String too long (`MaxStrlenValidator`)
- String too short (`MinStrlenValidator`)
- Invalid email format (`EmailValidator`)
- Invalid URL format (`UrlValidator`)
- Invalid phone format (`PhoneNumberValidator`)

---

## Sanitization Errors

**Cause:** Sanitizer fails to process the data.

```rust
// Sanitization errors are rare but can occur with malformed data
match result {
    Err(DbmsError::Sanitize(msg)) => {
        println!("Sanitization failed: {}", msg);
    }
    _ => {}
}
```

Sanitization errors are less common than validation errors since sanitizers typically transform data rather than reject it.

---

## Memory Errors

**Cause:** Low-level memory errors.

```rust
pub enum MemoryError {
    OutOfBounds,           // Read/write outside allocated memory
    ProviderError(String),      // Memory provider error
    InsufficientSpace,     // Not enough space to allocate
}
```

**Memory errors are rare** and usually indicate:

- Running out of available memory
- Corrupted memory state
- Bug in wasm-dbms (please report!)

---

## Error Handling Examples

**Basic error handling:**

```rust
let result = database.insert::<User>(user);

match result {
    Ok(()) => println!("Insert successful"),
    Err(DbmsError::Query(QueryError::PrimaryKeyConflict)) => {
        println!("User already exists");
    }
    Err(DbmsError::Query(QueryError::UniqueConstraintViolation { field })) => {
        println!("Duplicate value on field: {}", field);
    }
    Err(DbmsError::Query(QueryError::BrokenForeignKeyReference)) => {
        println!("Referenced record doesn't exist");
    }
    Err(DbmsError::Validation(msg)) => {
        println!("Validation error: {}", msg);
    }
    Err(e) => {
        println!("Database error: {:?}", e);
    }
}
```

**Helper function pattern:**

```rust
fn handle_db_error(error: DbmsError) -> String {
    match error {
        DbmsError::Query(QueryError::PrimaryKeyConflict) =>
            "Record with this ID already exists".to_string(),
        DbmsError::Query(QueryError::UniqueConstraintViolation { field }) =>
            format!("Duplicate value on unique field: {}", field),
        DbmsError::Query(QueryError::BrokenForeignKeyReference) =>
            "Referenced record not found".to_string(),
        DbmsError::Query(QueryError::ForeignKeyConstraintViolation) =>
            "Cannot delete: record has dependencies".to_string(),
        DbmsError::Validation(msg) =>
            format!("Invalid data: {}", msg),
        _ =>
            format!("Unexpected error: {:?}", error),
    }
}
```

> For IC client-specific error handling (double result pattern with `CallError`), see the [IC Errors Reference](../ic/reference/errors.md).

# CRUD Operations (IC)

> **Note:** This is the IC-specific CRUD operations guide, covering usage via the `ic-dbms-client`. For core CRUD concepts (filtering, delete behaviors, error types), see the [generic CRUD operations guide](../../guides/crud-operations.md).

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

ic-dbms provides four fundamental database operations, accessed through the `ic-dbms-client` crate's `Client` trait. All operations use Candid serialization under the hood and support the IC's inter-canister call model.

| Operation | Description | Returns |
|-----------|-------------|---------|
| **Insert** | Add a new record to a table | `Result<()>` |
| **Select** | Query records from a table | `Result<Vec<Record>>` |
| **Update** | Modify existing records | `Result<u64>` (affected rows) |
| **Delete** | Remove records from a table | `Result<u64>` (affected rows) |

All operations:
- Respect access control (caller must be in ACL)
- Support optional transaction IDs
- Validate and sanitize data according to schema rules
- Enforce foreign key constraints
- Return a double `Result` (see [Error Handling](#error-handling))

---

## Insert

### Basic Insert

To insert a record, create an `InsertRequest` and call the insert method:

```rust
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};
use my_schema::{User, UserInsertRequest};
use ic_dbms_api::prelude::*;

let client = IcDbmsCanisterClient::new(canister_id);

let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
    created_at: DateTime::now(),
};

// Insert without transaction (None)
client
    .insert::<User>(User::table_name(), user, None)
    .await??;
```

### Handling Primary Keys

Every table must have a primary key. Insert will fail if a record with the same primary key already exists:

```rust
// First insert succeeds
client.insert::<User>(User::table_name(), user1, None).await??;

// Second insert with same ID fails with PrimaryKeyConflict
let result = client.insert::<User>(User::table_name(), user2_same_id, None).await?;
assert!(matches!(result, Err(IcDbmsError::Query(QueryError::PrimaryKeyConflict))));
```

### Nullable Fields

For fields wrapped in `Nullable<T>`, you can insert either a value or null:

```rust
#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[candid]
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

client.insert::<Profile>(Profile::table_name(), profile, None).await??;
```

### Insert with Transaction

To insert within a transaction, pass the transaction ID:

```rust
// Begin transaction
let tx_id = client.begin_transaction().await?;

// Insert within transaction
client.insert::<User>(User::table_name(), user, Some(tx_id)).await??;

// Commit or rollback
client.commit(tx_id).await??;
```

---

## Select

### Select All Records

Use `Query::builder().all()` to select all records:

```rust
use ic_dbms_api::prelude::*;

let query = Query::builder().all().build();
let users: Vec<UserRecord> = client
    .select::<User>(User::table_name(), query, None)
    .await??;

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

let users = client.select::<User>(User::table_name(), query, None).await??;
```

See the [Querying Guide](../../guides/querying.md) for comprehensive filter documentation.

### Select Specific Columns

Select only the columns you need:

```rust
let query = Query::builder()
    .columns(vec!["id".to_string(), "name".to_string()])
    .build();

let users = client.select::<User>(User::table_name(), query, None).await??;
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

let posts = client.select::<Post>(Post::table_name(), query, None).await??;
```

See the [Relationships Guide](../../guides/relationships.md) for more on eager loading.

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

let affected_rows = client
    .update::<User>(User::table_name(), update, None)
    .await??;

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

client.update::<User>(User::table_name(), update, None).await??;
```

### Update with Filter

The filter determines which records are updated:

```rust
// Update all users with a specific domain
let update = UserUpdateRequest::builder()
    .set_verified(true.into())
    .filter(Filter::like("email", "%@company.com"))
    .build();

let affected = client.update::<User>(User::table_name(), update, None).await??;
println!("Verified {} company users", affected);
```

### Update Return Value

Update returns the number of affected rows:

```rust
let affected = client.update::<User>(User::table_name(), update, None).await??;

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
use ic_dbms_api::prelude::DeleteBehavior;

let filter = Filter::eq("id", Value::Uint32(1.into()));

let deleted = client
    .delete::<User>(
        User::table_name(),
        DeleteBehavior::Restrict,
        Some(filter),
        None  // No transaction
    )
    .await??;

println!("Deleted {} record(s)", deleted);
```

### Delete Behaviors

When deleting records that are referenced by foreign keys, you must specify a behavior:

| Behavior | Description |
|----------|-------------|
| `Restrict` | Fail if any foreign keys reference this record |
| `Cascade` | Delete all records that reference this record |

**Restrict Example:**

```rust
// Will fail if any posts reference this user
let result = client.delete::<User>(
    User::table_name(),
    DeleteBehavior::Restrict,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
    None
).await?;

match result {
    Ok(count) => println!("Deleted {} user(s)", count),
    Err(IcDbmsError::Query(QueryError::ForeignKeyConstraintViolation)) => {
        println!("Cannot delete: user has posts");
    }
    Err(e) => return Err(e.into()),
}
```

**Cascade Example:**

```rust
// Deletes the user AND all their posts
client.delete::<User>(
    User::table_name(),
    DeleteBehavior::Cascade,
    Some(Filter::eq("id", Value::Uint32(1.into()))),
    None
).await??;
```

### Delete All Records

Pass `None` as the filter to delete all records (use with caution):

```rust
// Delete ALL users (respecting foreign key behavior)
let deleted = client
    .delete::<User>(
        User::table_name(),
        DeleteBehavior::Cascade,
        None,  // No filter = all records
        None
    )
    .await??;

println!("Deleted all {} users and their related records", deleted);
```

---

## Operations with Transactions

All CRUD operations accept an optional transaction ID. When provided, the operation is performed within that transaction and won't be visible to other callers until committed:

```rust
// Begin transaction
let tx_id = client.begin_transaction().await?;

// Perform operations within transaction
client.insert::<User>(User::table_name(), user1, Some(tx_id)).await??;
client.insert::<User>(User::table_name(), user2, Some(tx_id)).await??;

// Update within same transaction
let update = UserUpdateRequest::builder()
    .set_verified(true.into())
    .filter(Filter::all())
    .build();
client.update::<User>(User::table_name(), update, Some(tx_id)).await??;

// Commit all changes atomically
client.commit(tx_id).await??;
```

See the [Transactions Guide](../../guides/transactions.md) for comprehensive transaction documentation.

---

## Error Handling

CRUD operations via the IC client return a **double Result**: `Result<Result<T, IcDbmsError>, CallError>`.

- **Outer `Result`**: Network/canister call errors (canister unreachable, cycles exhausted)
- **Inner `Result`**: Database logic errors (validation, constraint violations, etc.)

Use `??` to propagate both:

```rust
client.insert::<User>(User::table_name(), user, None).await??;
```

Or handle each layer explicitly:

```rust
match client.insert::<User>(User::table_name(), user, None).await {
    Ok(Ok(())) => println!("Insert successful"),
    Ok(Err(db_error)) => {
        // Handle database errors
        match db_error {
            IcDbmsError::Query(QueryError::PrimaryKeyConflict) => {
                println!("User already exists");
            }
            IcDbmsError::Validation(msg) => {
                println!("Validation error: {}", msg);
            }
            _ => println!("Database error: {:?}", db_error),
        }
    }
    Err(call_error) => {
        // Handle network/call errors
        println!("Failed to call canister: {:?}", call_error);
    }
}
```

Common error types:

| Error | Cause | Operation |
|-------|-------|-----------|
| `PrimaryKeyConflict` | Record with same primary key exists | Insert |
| `ForeignKeyConstraintViolation` | Referenced record doesn't exist, or delete restricted | Insert, Update, Delete |
| `BrokenForeignKeyReference` | Foreign key points to non-existent record | Insert, Update |
| `UnknownColumn` | Invalid column name in filter or select | Select, Update, Delete |
| `MissingNonNullableField` | Required field not provided | Insert, Update |
| `RecordNotFound` | No record matches the criteria | Update, Delete |
| `TransactionNotFound` | Invalid transaction ID | All |
| `InvalidQuery` | Malformed query (e.g., invalid JSON path) | Select |

See the [Errors Reference (IC)](../reference/errors.md) for complete IC-specific error documentation, or the [generic Errors Reference](../../reference/errors.md) for the full error hierarchy.

# Transactions

- [Transactions](#transactions)
  - [Overview](#overview)
  - [Transaction Lifecycle](#transaction-lifecycle)
    - [Begin Transaction](#begin-transaction)
    - [Perform Operations](#perform-operations)
    - [Commit](#commit)
    - [Rollback](#rollback)
  - [ACID Properties](#acid-properties)
    - [Atomicity](#atomicity)
    - [Consistency](#consistency)
    - [Isolation](#isolation)
    - [Durability](#durability)
  - [Error Handling](#error-handling)
    - [Handling Failures](#handling-failures)
    - [Transaction Errors](#transaction-errors)
  - [Best Practices](#best-practices)
    - [1. Keep transactions short](#1-keep-transactions-short)
    - [2. Always handle rollback](#2-always-handle-rollback)
    - [3. Use transactions for related operations](#3-use-transactions-for-related-operations)
    - [4. Don't mix transactional and non-transactional operations](#4-dont-mix-transactional-and-non-transactional-operations)
  - [Examples](#examples)
    - [Bank Transfer](#bank-transfer)
    - [Order Processing](#order-processing)

---

## Overview

wasm-dbms supports ACID transactions, allowing you to group multiple database operations into a single atomic unit. Either all operations succeed and are committed together, or none of them take effect.

**Key features:**

- **Atomicity**: All operations in a transaction succeed or fail together
- **Consistency**: Data integrity constraints are maintained
- **Isolation**: Transactions are isolated from each other
- **Durability**: Committed changes persist

---

## Transaction Lifecycle

### Begin Transaction

Start a new transaction using `DbmsContext::begin_transaction()`:

```rust
use wasm_dbms::prelude::*;

// Begin a new transaction
let tx_id = ctx.begin_transaction();
println!("Started transaction: {}", tx_id);
```

The returned transaction ID is used to create a transactional database instance.

### Perform Operations

Create a `WasmDbmsDatabase` with the transaction ID and perform operations:

```rust
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// Insert within transaction
database.insert::<User>(user)?;

// Update within transaction
database.update::<User>(update)?;

// Delete within transaction
database.delete::<User>(DeleteBehavior::Restrict, Some(filter))?;

// Select within transaction (sees uncommitted changes)
let users = database.select::<User>(query)?;
```

> **Note:** Operations within a transaction are visible to subsequent operations in the same transaction, but not to other callers until committed.

### Commit

Commit the transaction to make all changes permanent:

```rust
// Commit the transaction
database.commit()?;
println!("Transaction committed successfully");
```

After commit:

- All changes become visible to other callers
- The transaction ID becomes invalid
- Changes persist in storage

### Rollback

Rollback the transaction to discard all changes:

```rust
// Rollback the transaction
database.rollback()?;
println!("Transaction rolled back");
```

After rollback:

- All changes within the transaction are discarded
- The transaction ID becomes invalid
- The database state is as if the transaction never happened

---

## ACID Properties

### Atomicity

All operations in a transaction are treated as a single unit. If any operation fails, the entire transaction can be rolled back:

```rust
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// First operation succeeds
database.insert::<User>(user1)?;

// Second operation fails (e.g., primary key conflict)
let result = database.insert::<User>(user2_duplicate);

if result.is_err() {
    // Rollback everything - user1 is also discarded
    database.rollback()?;
}
```

### Consistency

Transactions maintain data integrity:

- Primary key uniqueness is enforced
- Foreign key constraints are checked
- Validators run on all data
- Sanitizers are applied

```rust
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// This will fail if referenced user doesn't exist
let post = PostInsertRequest {
    id: 1.into(),
    title: "My Post".into(),
    author_id: 999.into(),  // Non-existent user
};

let result = database.insert::<Post>(post);
// Returns Err(DbmsError::Query(QueryError::BrokenForeignKeyReference))
```

### Isolation

Changes made within a transaction are not visible to other callers until committed:

```rust
// Database A starts a transaction
let tx_id = ctx.begin_transaction();
let mut db_a = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);
db_a.insert::<User>(new_user)?;

// Database B queries - does NOT see the new user
let db_b = WasmDbmsDatabase::oneshot(&ctx, my_schema);
let users = db_b.select::<User>(query)?;
assert!(!users.iter().any(|u| u.id == new_user.id));

// Database A commits
db_a.commit()?;

// Now Database B can see the user
let users = db_b.select::<User>(query)?;
assert!(users.iter().any(|u| u.id == new_user.id));
```

### Durability

Committed transactions persist in storage. When using stable memory providers (e.g., on the Internet Computer), data survives across upgrades.

---

## Error Handling

### Handling Failures

When an operation fails within a transaction, you should typically rollback:

```rust
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

fn process_order(database: &impl Database) -> Result<(), DbmsError> {
    // Multiple operations that should succeed together
    database.insert::<Order>(order)?;
    database.update::<Inventory>(update)?;
    database.insert::<OrderItem>(item)?;
    Ok(())
}

match process_order(&database) {
    Ok(()) => {
        database.commit()?;
        println!("Order processed successfully");
    }
    Err(e) => {
        database.rollback()?;
        println!("Order failed, rolled back: {:?}", e);
    }
}
```

### Transaction Errors

| Error | Cause |
|-------|-------|
| `TransactionNotFound` | Invalid transaction ID or transaction already completed |
| `NoActiveTransaction` | Attempting to commit/rollback without an active transaction |

```rust
use wasm_dbms_api::prelude::{DbmsError, TransactionError};

match database.commit() {
    Ok(()) => println!("Committed"),
    Err(DbmsError::Transaction(TransactionError::NoActiveTransaction)) => {
        println!("No active transaction to commit");
    }
    Err(e) => println!("Other error: {:?}", e),
}
```

---

## Best Practices

### 1. Keep transactions short

Long-running transactions hold resources and block other operations:

```rust
// GOOD: Prepare data outside transaction
let users_to_insert = prepare_users();

let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);
for user in users_to_insert {
    database.insert::<User>(user)?;
}
database.commit()?;

// BAD: Doing expensive work inside transaction
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);
for raw_data in large_dataset {
    let user = expensive_parsing(raw_data);  // Don't do this in transaction
    database.insert::<User>(user)?;
}
database.commit()?;
```

### 2. Always handle rollback

Ensure transactions are either committed or rolled back:

```rust
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

let result = (|| -> Result<(), DbmsError> {
    database.insert::<User>(user1)?;
    database.insert::<User>(user2)?;
    Ok(())
})();

match result {
    Ok(()) => database.commit()?,
    Err(_) => database.rollback()?,
}
```

### 3. Use transactions for related operations

Group operations that should succeed or fail together:

```rust
// GOOD: Related operations in transaction
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);
database.insert::<Order>(order)?;
database.insert::<Payment>(payment)?;
database.update::<Inventory>(inv_update)?;
database.commit()?;

// BAD: Unrelated operations in transaction (unnecessary)
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);
database.insert::<UserPreferences>(prefs)?;
database.insert::<AuditLog>(log)?;  // Unrelated
database.commit()?;
```

### 4. Don't mix transactional and non-transactional operations

```rust
let tx_id = ctx.begin_transaction();
let mut database = WasmDbmsDatabase::from_transaction(&ctx, my_schema, tx_id);

// GOOD: All operations use the transaction
database.insert::<Order>(order)?;
database.insert::<OrderItem>(item)?;

// BAD: Mixing transaction and non-transaction
let oneshot = WasmDbmsDatabase::oneshot(&ctx, my_schema);
database.insert::<Order>(order)?;
oneshot.insert::<AuditLog>(log)?;  // Not in transaction!
```

---

## Examples

### Bank Transfer

Transfer money between accounts atomically:

```rust
fn transfer(
    ctx: &DbmsContext<impl MemoryProvider>,
    from_account: u32,
    to_account: u32,
    amount: Decimal,
) -> Result<(), DbmsError> {
    let tx_id = ctx.begin_transaction();
    let mut database = WasmDbmsDatabase::from_transaction(ctx, my_schema, tx_id);

    // Deduct from source account
    let deduct = AccountUpdateRequest::builder()
        .decrease_balance(amount)
        .filter(Filter::eq("id", Value::Uint32(from_account.into())))
        .build();
    database.update::<Account>(deduct)?;

    // Add to destination account
    let add = AccountUpdateRequest::builder()
        .increase_balance(amount)
        .filter(Filter::eq("id", Value::Uint32(to_account.into())))
        .build();
    database.update::<Account>(add)?;

    // Record the transfer
    let transfer_record = TransferInsertRequest {
        id: Uuid::new_v4().into(),
        from_account: from_account.into(),
        to_account: to_account.into(),
        amount,
        timestamp: DateTime::now(),
    };
    database.insert::<Transfer>(transfer_record)?;

    // Commit atomically
    database.commit()?;
    Ok(())
}
```

### Order Processing

Process an order with inventory update:

```rust
fn process_order(
    ctx: &DbmsContext<impl MemoryProvider>,
    order: OrderInsertRequest,
    items: Vec<OrderItemInsertRequest>,
) -> Result<u32, Box<dyn std::error::Error>> {
    let tx_id = ctx.begin_transaction();
    let mut database = WasmDbmsDatabase::from_transaction(ctx, my_schema, tx_id);

    // Insert the order
    database.insert::<Order>(order.clone())?;

    // Insert order items and update inventory
    for item in items {
        // Insert order item
        database.insert::<OrderItem>(item.clone())?;

        // Decrease inventory
        let inv_update = InventoryUpdateRequest::builder()
            .decrease_quantity(item.quantity)
            .filter(Filter::eq("product_id", Value::Uint32(item.product_id)))
            .build();

        let updated = database.update::<Inventory>(inv_update)?;

        if updated == 0 {
            // Product not in inventory, rollback
            database.rollback()?;
            return Err("Product not found in inventory".into());
        }
    }

    // All successful, commit
    database.commit()?;
    Ok(order.id.into())
}
```

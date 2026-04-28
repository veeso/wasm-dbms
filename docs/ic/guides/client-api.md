# Client API (IC)

> **Note:** This is the IC-specific client API guide. For general wasm-dbms documentation, see
> the [generic docs](../../guides/).

- [Client API (IC)](#client-api-ic)
  - [Overview](#overview)
  - [Client Types](#client-types)
    - [IcDbmsCanisterClient](#icdbmscanisterclient)
    - [IcDbmsAgentClient](#icdbmsagentclient)
    - [IcDbmsPocketIcClient](#icdbmspocketicclient)
  - [Installation](#installation)
  - [The Client Trait](#the-client-trait)
  - [Operations](#operations)
    - [Insert](#insert)
    - [Select](#select)
    - [Aggregate](#aggregate)
    - [Update](#update)
    - [Delete](#delete)
    - [Transactions](#transactions)
    - [Schema Migrations](#schema-migrations)
    - [ACL Management](#acl-management)
  - [Error Handling](#error-handling)
  - [Examples](#examples)
    - [Inter-Canister Communication](#inter-canister-communication)
    - [External Application](#external-application)
    - [Integration Testing](#integration-testing)

---

## Overview

The `ic-dbms-client` crate provides type-safe Rust clients for interacting with ic-dbms canisters. Instead of manually
constructing Candid calls, you use a high-level API that handles serialization and error handling.

**Benefits:**

- Type-safe operations with compile-time checking
- Automatic Candid encoding/decoding
- Consistent API across different environments
- Built-in error handling

---

## Client Types

ic-dbms provides three client implementations for different use cases:

| Client                 | Use Case                                       | Feature Flag |
| ---------------------- | ---------------------------------------------- | ------------ |
| `IcDbmsCanisterClient` | Inter-canister calls (inside IC canisters)     | Default      |
| `IcDbmsAgentClient`    | External applications (frontend, backend, CLI) | `ic-agent`   |
| `IcDbmsPocketIcClient` | Integration tests with PocketIC                | `pocket-ic`  |

### IcDbmsCanisterClient

For calls from one IC canister to another:

```rust
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};
use candid::Principal;

// In your canister code
let dbms_canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
let client = IcDbmsCanisterClient::new(dbms_canister_id);

// Use the client
let users = client.select::<User>(User::table_name(), query, None).await??;
```

### IcDbmsAgentClient

For external applications using the IC Agent:

```rust
use ic_dbms_client::{IcDbmsAgentClient, Client as _};
use ic_agent::Agent;
use candid::Principal;

// Create an IC Agent (with identity, etc.)
let agent = Agent::builder()
    .with_url("https://ic0.app")
    .with_identity(identity)
    .build()?;

agent.fetch_root_key().await?;  // Only needed for local replica

let dbms_canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
let client = IcDbmsAgentClient::new(dbms_canister_id, &agent);

// Use the client
let users = client.select::<User>(User::table_name(), query, None).await??;
```

### IcDbmsPocketIcClient

For integration tests using PocketIC:

```rust
use ic_dbms_client::{IcDbmsPocketIcClient, Client as _};
use pocket_ic::PocketIc;
use candid::Principal;

let pic = PocketIc::new();
// ... setup canister ...

let client = IcDbmsPocketIcClient::new(
    canister_id,
    caller_principal,  // The principal making calls
    &pic
);

// Use the client in tests
let users = client.select::<User>(User::table_name(), query, None).await??;
```

---

## Installation

Add `ic-dbms-client` to your `Cargo.toml`:

**For canister development (inter-canister calls):**

```toml
[dependencies]
ic-dbms-client = "0.6"
```

**For external applications:**

```toml
[dependencies]
ic-dbms-client = { version = "0.9", features = ["ic-agent"] }
```

**For integration tests:**

```toml
[dev-dependencies]
ic-dbms-client = { version = "0.9", features = ["pocket-ic"] }
```

---

## The Client Trait

All clients implement the `Client` trait, providing a consistent API:

```rust
pub trait Client {
    // CRUD Operations
    async fn insert<T: Table>(&self, table: &str, record: T::InsertRequest, tx: Option<u64>) -> Result<Result<(), IcDbmsError>>;
    async fn select<T: Table>(&self, table: &str, query: Query<T>, tx: Option<u64>) -> Result<Result<Vec<T::Record>, IcDbmsError>>;
    async fn aggregate<T: Table>(&self, table: &str, query: Query, aggregates: Vec<AggregateFunction>, tx: Option<u64>) -> Result<Result<Vec<AggregatedRow>, IcDbmsError>>;
    async fn update<T: Table>(&self, table: &str, update: T::UpdateRequest, tx: Option<u64>) -> Result<Result<u64, IcDbmsError>>;
    async fn delete<T: Table>(&self, table: &str, behavior: DeleteBehavior, filter: Option<Filter>, tx: Option<u64>) -> Result<Result<u64, IcDbmsError>>;

    // Transactions
    async fn begin_transaction(&self) -> Result<u64>;
    async fn commit(&self, tx: u64) -> Result<Result<(), IcDbmsError>>;
    async fn rollback(&self, tx: u64) -> Result<Result<(), IcDbmsError>>;

    // ACL Management
    async fn acl_add_principal(&self, principal: Principal) -> Result<Result<(), IcDbmsError>>;
    async fn acl_remove_principal(&self, principal: Principal) -> Result<Result<(), IcDbmsError>>;
    async fn acl_allowed_principals(&self) -> Result<Vec<Principal>>;

    // Schema Migrations
    async fn has_drift(&self) -> Result<Result<bool, IcDbmsError>>;
    async fn pending_migrations(&self) -> Result<Result<Vec<MigrationOp>, IcDbmsError>>;
    async fn migrate(&self, policy: MigrationPolicy) -> Result<Result<(), IcDbmsError>>;
}
```

**Note the double Result:**

- Outer `Result`: Network/communication errors
- Inner `Result`: Business logic errors (IcDbmsError)

---

## Operations

### Insert

```rust
use ic_dbms_client::Client as _;
use my_schema::{User, UserInsertRequest};

let user = UserInsertRequest {
    id: 1.into(),
    name: "Alice".into(),
    email: "alice@example.com".into(),
};

// Without transaction
client.insert::<User>(User::table_name(), user, None).await??;

// With transaction
client.insert::<User>(User::table_name(), user, Some(tx_id)).await??;
```

### Select

```rust
use ic_dbms_api::prelude::*;

// Select all
let query = Query::builder().all().build();
let users: Vec<UserRecord> = client
    .select::<User>(User::table_name(), query, None)
    .await??;

// Select with filter
let query = Query::builder()
    .filter(Filter::eq("status", Value::Text("active".into())))
    .order_by("created_at", OrderDirection::Descending)
    .limit(10)
    .build();
let users = client.select::<User>(User::table_name(), query, None).await??;
```

### Aggregate

Aggregate queries dispatch to the per-table `aggregate_<table>` endpoint
generated by `DbmsCanister`. The pipeline (`WHERE` -> `DISTINCT` -> `GROUP BY`
-> aggregate computation -> `HAVING` -> `ORDER BY` -> `OFFSET`/`LIMIT`) is
described in the [Query API reference](../../reference/query.md#execution-order).

```rust
use ic_dbms_api::prelude::{AggregateFunction, AggregatedValue, Filter, Query, Uint64, Value};

// COUNT(*) of all rows
let result = client
    .aggregate::<User>(
        User::table_name(),
        Query::default(),
        vec![AggregateFunction::Count(None)],
        None,
    )
    .await??;
assert!(matches!(result[0].values[0], AggregatedValue::Count(_)));

// GROUP BY + HAVING: rows per role, only roles with more than 5 users
let query = Query::builder()
    .group_by(&["role"])
    .having(Filter::gt("agg0", Value::Uint64(Uint64(5))))
    .order_by_desc("agg0")
    .build();
let result = client
    .aggregate::<User>(
        User::table_name(),
        query,
        vec![AggregateFunction::Count(None)],
        None,
    )
    .await??;
```

`HAVING` and `ORDER BY` reference aggregate outputs by their positional name
`agg{N}` (`agg0` is the first aggregate, `agg1` the second, ...). They may
also reference any column listed in `group_by`.

### Update

```rust
use my_schema::UserUpdateRequest;

let update = UserUpdateRequest::builder()
    .set_email("new@example.com".into())
    .filter(Filter::eq("id", Value::Uint32(1.into())))
    .build();

let affected_rows: u64 = client
    .update::<User>(User::table_name(), update, None)
    .await??;
```

### Delete

```rust
use ic_dbms_api::prelude::DeleteBehavior;

// Delete with filter
let deleted: u64 = client
    .delete::<User>(
        User::table_name(),
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(1.into()))),
        None
    )
    .await??;

// Delete all (be careful!)
let deleted: u64 = client
    .delete::<User>(
        User::table_name(),
        DeleteBehavior::Cascade,
        None,  // No filter = all records
        None
    )
    .await??;
```

### Transactions

```rust
// Begin transaction
let tx_id = client.begin_transaction().await?;

// Perform operations
client.insert::<User>(User::table_name(), user1, Some(tx_id)).await??;
client.insert::<User>(User::table_name(), user2, Some(tx_id)).await??;

// Commit or rollback
match some_condition {
    true => client.commit(tx_id).await??,
    false => client.rollback(tx_id).await??,
}
```

### Schema Migrations

Three admin-gated methods inspect and apply schema drift. The Candid
endpoints behind them (`has_drift` query, `pending_migrations` query, `migrate`
update) are emitted by `#[derive(DbmsCanister)]`. See the
[IC migrations guide](./migrations.md) for the upgrade workflow.

```rust
use ic_dbms_api::prelude::{MigrationOp, MigrationPolicy};

// O(1) once cached on the canister side. True iff a migration is needed.
let drift: bool = client.has_drift().await??;
if !drift {
    return Ok(());
}

// Plan without applying. Always recomputes; safe to call during drift.
let plan: Vec<MigrationOp> = client.pending_migrations().await??;
for op in &plan {
    eprintln!("  {op:?}");
}

// Apply. Refuses DropTable / DropColumn unless allow_destructive is set.
client.migrate(MigrationPolicy::default()).await??;

// Equivalent to:
client
    .migrate(MigrationPolicy { allow_destructive: false })
    .await??;
```

`migrate` is idempotent — when there is no drift, the call is a cheap no-op.

### ACL Management

```rust
use candid::Principal;

// Add principal
let new_principal = Principal::from_text("aaaaa-aa").unwrap();
client.acl_add_principal(new_principal).await??;

// Remove principal
client.acl_remove_principal(new_principal).await??;

// List principals
let allowed = client.acl_allowed_principals().await?;
for p in allowed {
    println!("Allowed: {}", p);
}
```

---

## Error Handling

Client operations return nested Results:

```rust
// Full error handling
match client.insert::<User>(User::table_name(), user, None).await {
    Ok(Ok(())) => {
        println!("Insert successful");
    }
    Ok(Err(db_error)) => {
        // Database error (validation, constraint violation, etc.)
        match db_error {
            IcDbmsError::Query(QueryError::PrimaryKeyConflict) => {
                println!("User with this ID already exists");
            }
            IcDbmsError::Validation(msg) => {
                println!("Validation failed: {}", msg);
            }
            _ => println!("Database error: {:?}", db_error),
        }
    }
    Err(call_error) => {
        // Network/canister call error
        println!("Call failed: {:?}", call_error);
    }
}
```

**Simplified with `??`:**

```rust
// Propagate both error types
client.insert::<User>(User::table_name(), user, None).await??;
```

---

## Examples

### Inter-Canister Communication

A backend canister calling the database canister:

```rust
use ic_cdk::update;
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};
use candid::Principal;

const DBMS_CANISTER: &str = "rrkah-fqaaa-aaaaa-aaaaq-cai";

#[update]
async fn create_user(name: String, email: String) -> Result<u32, String> {
    let client = IcDbmsCanisterClient::new(
        Principal::from_text(DBMS_CANISTER).unwrap()
    );

    let user_id = generate_id();
    let user = UserInsertRequest {
        id: user_id.into(),
        name: name.into(),
        email: email.into(),
    };

    client
        .insert::<User>(User::table_name(), user, None)
        .await
        .map_err(|e| format!("Call failed: {:?}", e))?
        .map_err(|e| format!("Insert failed: {:?}", e))?;

    Ok(user_id)
}

#[update]
async fn get_users() -> Result<Vec<UserRecord>, String> {
    let client = IcDbmsCanisterClient::new(
        Principal::from_text(DBMS_CANISTER).unwrap()
    );

    let query = Query::builder().all().build();

    client
        .select::<User>(User::table_name(), query, None)
        .await
        .map_err(|e| format!("Call failed: {:?}", e))?
        .map_err(|e| format!("Query failed: {:?}", e))
}
```

### External Application

A CLI tool or backend service:

```rust
use ic_agent::{Agent, identity::BasicIdentity};
use ic_dbms_client::{IcDbmsAgentClient, Client as _};
use candid::Principal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load identity from PEM file
    let identity = BasicIdentity::from_pem_file("identity.pem")?;

    // Create agent
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .with_identity(identity)
        .build()?;

    // For local development, fetch root key
    // agent.fetch_root_key().await?;

    let canister_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai")?;
    let client = IcDbmsAgentClient::new(canister_id, &agent);

    // List all users
    let query = Query::builder().all().build();
    let users = client.select::<User>(User::table_name(), query, None).await??;

    for user in users {
        println!("User: {} ({})", user.name, user.email);
    }

    Ok(())
}
```

### Integration Testing

Testing with PocketIC:

```rust
use ic_dbms_client::{IcDbmsPocketIcClient, Client as _};
use pocket_ic::PocketIc;
use candid::{encode_one, Principal};

#[tokio::test]
async fn test_user_crud() {
    // Setup PocketIC
    let pic = PocketIc::new();

    // Create and install canister
    let canister_id = pic.create_canister();
    pic.add_cycles(canister_id, 2_000_000_000_000);

    let wasm = std::fs::read("path/to/canister.wasm").unwrap();
    let init_args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
        allowed_principals: vec![admin_principal],
    });

    pic.install_canister(
        canister_id,
        wasm,
        encode_one(init_args).unwrap(),
        None
    );

    // Create client
    let client = IcDbmsPocketIcClient::new(canister_id, admin_principal, &pic);

    // Test insert
    let user = UserInsertRequest {
        id: 1.into(),
        name: "Test User".into(),
        email: "test@example.com".into(),
    };
    client.insert::<User>(User::table_name(), user, None).await.unwrap().unwrap();

    // Test select
    let query = Query::builder().all().build();
    let users = client.select::<User>(User::table_name(), query, None).await.unwrap().unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].name.as_str(), "Test User");

    // Test update
    let update = UserUpdateRequest::builder()
        .set_name("Updated User".into())
        .filter(Filter::eq("id", Value::Uint32(1.into())))
        .build();
    let affected = client.update::<User>(User::table_name(), update, None).await.unwrap().unwrap();
    assert_eq!(affected, 1);

    // Test delete
    let deleted = client.delete::<User>(
        User::table_name(),
        DeleteBehavior::Restrict,
        Some(Filter::eq("id", Value::Uint32(1.into()))),
        None
    ).await.unwrap().unwrap();
    assert_eq!(deleted, 1);

    // Verify deletion
    let users = client.select::<User>(User::table_name(), Query::builder().all().build(), None).await.unwrap().unwrap();
    assert_eq!(users.len(), 0);
}
```

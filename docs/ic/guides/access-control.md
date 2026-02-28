# Access Control (IC)

> **Note:** This is the IC-specific access control guide. Access control is an IC-only feature. For general wasm-dbms documentation, see the [generic guides](../../guides/get-started.md).

- [Overview](#overview)
- [Initial Configuration](#initial-configuration)
  - [Init Arguments](#init-arguments)
  - [Deployment Example](#deployment-example)
- [Managing Principals](#managing-principals)
  - [Add Principal](#add-principal)
  - [Remove Principal](#remove-principal)
  - [List Allowed Principals](#list-allowed-principals)
- [Authorization Enforcement](#authorization-enforcement)
- [Common Patterns](#common-patterns)
  - [Frontend Canister Access](#frontend-canister-access)
  - [Admin Principal](#admin-principal)
  - [Multiple Services](#multiple-services)
- [Security Considerations](#security-considerations)

---

## Overview

ic-dbms uses an Access Control List (ACL) to manage which principals can interact with the database canister. Only principals in the ACL can:

- Perform CRUD operations (insert, select, update, delete)
- Manage transactions (begin, commit, rollback)
- Modify the ACL itself (add/remove principals)

**Key points:**

- The ACL is set during canister initialization
- Principals can be added or removed at runtime
- All database operations check the caller against the ACL
- The ACL persists across canister upgrades

---

## Initial Configuration

### Init Arguments

When deploying the canister, you must provide the initial list of allowed principals:

```candid
type IcDbmsCanisterArgs = variant {
  Init : IcDbmsCanisterInitArgs;
  Upgrade;
};

type IcDbmsCanisterInitArgs = record {
  allowed_principals : vec principal;
};
```

> **Warning**: If you deploy without including your own principal in the list, you won't be able to interact with the canister or add yourself later!

### Deployment Example

**Using dfx:**

```bash
# Single principal
dfx deploy my_dbms --argument '(variant { Init = record { allowed_principals = vec { principal "aaaaa-aa" } } })'

# Multiple principals
dfx deploy my_dbms --argument '(variant { Init = record { allowed_principals = vec { principal "aaaaa-aa"; principal "bbbbb-bb"; principal "ccccc-cc" } } })'
```

**Using dfx with identity:**

```bash
# Get your principal ID
dfx identity get-principal

# Deploy with your principal
ADMIN_PRINCIPAL=$(dfx identity get-principal)
dfx deploy my_dbms --argument "(variant { Init = record { allowed_principals = vec { principal \"$ADMIN_PRINCIPAL\" } } })"
```

**Programmatically (in another canister):**

```rust
use candid::Principal;
use ic_cdk::api::management_canister::main::{create_canister, install_code};

let init_args = IcDbmsCanisterArgs::Init(IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        Principal::from_text("aaaaa-aa").unwrap(),
        ic_cdk::caller(),  // Include the installing canister
    ],
});

// Install canister with init args...
```

---

## Managing Principals

### Add Principal

Add a new principal to the ACL:

```rust
use candid::Principal;
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};

let client = IcDbmsCanisterClient::new(canister_id);

// Add a new principal
let new_principal = Principal::from_text("aaaaa-aa").unwrap();
client.acl_add_principal(new_principal).await??;

println!("Principal added to ACL");
```

**Notes:**
- Only principals already in the ACL can add new principals
- Adding an already-allowed principal is a no-op (succeeds silently)

### Remove Principal

Remove a principal from the ACL:

```rust
// Remove a principal
let principal_to_remove = Principal::from_text("aaaaa-aa").unwrap();
client.acl_remove_principal(principal_to_remove).await??;

println!("Principal removed from ACL");
```

**Notes:**
- Only principals in the ACL can remove principals
- A principal can remove itself (be careful!)
- Removing a non-existent principal is a no-op (succeeds silently)

> **Warning**: If you remove all principals from the ACL, no one will be able to interact with the canister. This effectively locks the canister.

### List Allowed Principals

Query the current ACL:

```rust
let allowed = client.acl_allowed_principals().await?;

println!("Allowed principals:");
for principal in allowed {
    println!("  - {}", principal);
}
```

This is a query call (no cost, fast response).

---

## Authorization Enforcement

Every canister method checks the caller against the ACL:

```rust
// If caller is NOT in ACL, all operations fail
let result = client.select::<User>(User::table_name(), query, None).await?;
// Returns error if caller not authorized
```

The canister uses an inspect function to reject unauthorized calls before execution:

```rust
// Internal canister behavior (you don't write this):
#[inspect_message]
fn inspect_message() {
    let caller = ic_cdk::caller();
    if !ACL.with(|acl| acl.borrow().is_allowed(&caller)) {
        ic_cdk::trap("Unauthorized");
    }
    ic_cdk::accept_message();
}
```

This means:
- Unauthorized calls are rejected immediately
- No cycles are consumed for unauthorized calls
- The caller receives an error response

---

## Common Patterns

### Frontend Canister Access

Allow your frontend canister to access the database:

```bash
# Get the frontend canister ID
FRONTEND_ID=$(dfx canister id my_frontend)

# Add to ACL during deployment
dfx deploy my_dbms --argument "(variant { Init = record { allowed_principals = vec { principal \"$FRONTEND_ID\" } } })"
```

Or add at runtime:

```rust
// In your admin script or another canister
let frontend_id = Principal::from_text("rrkah-fqaaa-aaaaa-aaaaq-cai").unwrap();
client.acl_add_principal(frontend_id).await??;
```

### Admin Principal

Include an admin principal for maintenance:

```rust
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        frontend_canister_id,
        backend_canister_id,
        admin_principal,  // Your dfx identity for maintenance
    ],
};
```

**Recommended setup:**
- Include your dfx identity principal for administrative tasks
- Include all canisters that need database access
- Consider a separate admin canister for complex ACL management

### Multiple Services

If multiple canisters need database access:

```rust
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        user_service_canister,
        order_service_canister,
        analytics_canister,
        admin_principal,
    ],
};
```

**Architecture example:**

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  User Service   │     │ Order Service   │     │   Analytics     │
│    Canister     │     │    Canister     │     │    Canister     │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         │    All in ACL         │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                                 ▼
                    ┌────────────────────────┐
                    │    IC-DBMS Canister    │
                    │      (Database)        │
                    └────────────────────────┘
```

---

## Security Considerations

### Principle of Least Privilege

Only add principals that genuinely need database access:

```rust
// BAD: Too permissive
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        frontend_canister,
        logging_canister,      // Does logging need DB access?
        monitoring_canister,   // Does monitoring need DB access?
    ],
};

// GOOD: Only necessary principals
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        backend_canister,  // Only the backend talks to DB
    ],
};
```

### Avoid Locking Yourself Out

Always ensure at least one admin principal is in the ACL:

```rust
// DANGEROUS: Only include service canisters
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        frontend_canister,
    ],
};
// If frontend is deleted/upgraded incorrectly, you can't manage the DB

// SAFE: Include an admin
let init_args = IcDbmsCanisterInitArgs {
    allowed_principals: vec![
        frontend_canister,
        admin_principal,  // Fallback access
    ],
};
```

### Don't Share Admin Principals

Each developer should use their own principal:

```bash
# Each developer gets their own dfx identity
dfx identity new dev-alice
dfx identity new dev-bob

# Add each to ACL separately
```

### Audit ACL Changes

Log ACL modifications in your application:

```rust
async fn add_principal_with_audit(
    client: &impl Client,
    principal: Principal,
    added_by: Principal,
) -> Result<(), IcDbmsError> {
    // Add to ACL
    client.acl_add_principal(principal).await??;

    // Log the change (in your own audit table)
    let audit_log = AuditLogInsertRequest {
        id: Uuid::new_v4().into(),
        action: "ACL_ADD".into(),
        target_principal: principal.to_string().into(),
        performed_by: added_by.to_string().into(),
        timestamp: DateTime::now(),
    };
    client.insert::<AuditLog>(AuditLog::table_name(), audit_log, None).await??;

    Ok(())
}
```

### Consider Time-Limited Access

For temporary access (contractors, debugging), add and remove principals promptly:

```rust
// Grant temporary access
client.acl_add_principal(contractor_principal).await??;

// ... contractor does their work ...

// Revoke access when done
client.acl_remove_principal(contractor_principal).await??;
```

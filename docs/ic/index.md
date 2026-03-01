# IC-DBMS: Internet Computer Integration

- [Overview](#overview)
- [Architecture](#architecture)
- [Crates](#crates)
- [Quick Start](#quick-start)
- [Guides](#guides)
- [Reference](#reference)

---

## Overview

IC-DBMS is an adapter layer that brings the [wasm-dbms](https://github.com/veeso/wasm-dbms) relational database engine to the [Internet Computer](https://internetcomputer.org/) (IC). While wasm-dbms provides the core database functionality (tables, CRUD operations, transactions, memory management), ic-dbms adds everything needed to run it as an IC canister:

- **Candid serialization** for all types and API endpoints
- **Canister lifecycle management** (init, upgrade, inspect)
- **ACL-based access control** using IC principals
- **Procedural macros** to generate complete canister APIs from schema definitions
- **Client libraries** for inter-canister calls, external agent access, and integration testing

If you are using wasm-dbms outside the Internet Computer (e.g., in a standalone WASM runtime), you do not need ic-dbms. See the [generic wasm-dbms documentation](../guides/) instead.

---

## Architecture

```
┌─────────────────────────────────────────────────┐
│              Your Application                    │
│  (Frontend canister, backend canister, CLI, etc.)│
└──────────────────────┬──────────────────────────┘
                       │  Candid calls
                       ▼
┌─────────────────────────────────────────────────┐
│           ic-dbms-client                         │
│  (IcDbmsCanisterClient / IcDbmsAgentClient /     │
│   IcDbmsPocketIcClient)                          │
└──────────────────────┬──────────────────────────┘
                       │
                       ▼
┌─────────────────────────────────────────────────┐
│           ic-dbms-canister                       │
│  (Generated canister API, ACL, init/upgrade)     │
│                                                  │
│  ┌───────────────────────────────────────────┐   │
│  │           wasm-dbms (core engine)          │   │
│  │  Tables, CRUD, Transactions, Memory Mgmt  │   │
│  └───────────────────────────────────────────┘   │
└─────────────────────────────────────────────────┘
```

---

## Crates

IC-DBMS is composed of four crates:

| Crate | Description | Depends On |
|-------|-------------|------------|
| **ic-dbms-api** | Shared types, re-exports `wasm-dbms-api` types with IC additions. Provides `IcDbmsError` type alias and IC-compatible type wrappers. | `wasm-dbms-api` |
| **ic-dbms-canister** | Core canister engine. Provides the `DbmsCanister` derive macro target, ACL management, canister init/upgrade lifecycle, and the IC stable memory provider. | `wasm-dbms`, `ic-dbms-api` |
| **ic-dbms-macros** | Procedural macros: `#[derive(DatabaseSchema)]` (IC variant, uses IC crate paths) and `#[derive(DbmsCanister)]` for generating complete canister APIs. | `wasm-dbms-macros` |
| **ic-dbms-client** | Client library with three implementations: `IcDbmsCanisterClient` (inter-canister), `IcDbmsAgentClient` (external via IC agent), `IcDbmsPocketIcClient` (integration testing). | `ic-dbms-api` |

**Import convention:**

```rust
// In your schema crate
use ic_dbms_api::prelude::*;  // Re-exports wasm_dbms_api types

// In your canister crate
use ic_dbms_canister::prelude::DbmsCanister;

// In your client code
use ic_dbms_client::{IcDbmsCanisterClient, Client as _};
```

---

## Quick Start

1. Define your schema with IC-compatible derives:

```rust
use candid::{CandidType, Deserialize};
use ic_dbms_api::prelude::*;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub email: Text,
}
```

2. Generate the canister:

```rust
use ic_dbms_canister::prelude::{DatabaseSchema, DbmsCanister};

#[derive(DatabaseSchema, DbmsCanister)]
#[tables(User = "users")]
pub struct MyDbmsCanister;

ic_cdk::export_candid!();
```

3. Build, deploy, and interact:

```bash
cargo build --target wasm32-unknown-unknown --release
dfx deploy my_dbms --argument '(variant { Init = record { allowed_principals = vec { principal "your-principal" } } })'
```

```rust
let client = IcDbmsCanisterClient::new(canister_id);
client.insert::<User>(User::table_name(), user, None).await??;
```

For the full walkthrough, see the [Get Started guide](./guides/get-started.md).

---

## Guides

- [Get Started](./guides/get-started.md) - Set up and deploy your first IC database canister
- [CRUD Operations](./guides/crud-operations.md) - Insert, select, update, delete via the IC client
- [Access Control](./guides/access-control.md) - ACL management with IC principals
- [Client API](./guides/client-api.md) - All client types and usage patterns

For core wasm-dbms guides (querying, transactions, relationships, validators, sanitizers, custom data types), see the [generic guides](../guides/).

---

## Reference

- [Schema (IC)](./reference/schema.md) - DbmsCanister macro, Candid API generation, IC-specific derives
- [Data Types (IC)](./reference/data-types.md) - Principal type, Candid type mappings
- [Errors (IC)](./reference/errors.md) - IcDbmsError alias, double-Result pattern, client error handling

For the complete reference (all data types, error variants, sanitizers, validators, JSON operations), see the [generic reference](../reference/).

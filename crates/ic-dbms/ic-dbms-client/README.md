# ic-dbms-client

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/ic-dbms-client.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/ic-dbms-client.svg?logo=rust)](https://crates.io/crates/ic-dbms-client)
[![latest-version](https://img.shields.io/crates/v/ic-dbms-client.svg?logo=rust)](https://crates.io/crates/ic-dbms-client)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/ic-dbms-client/badge.svg?logo=rust)](https://docs.rs/ic-dbms-client)

This crate exposes all the types which may be used by an external canister to interact with an IC DBMS Canister
instance.

## Client

All the client methods can be accessed through the [`Client`](crate::prelude::Client) trait.

The crate provides an implementation of the client for IC DBMS Canister, called [
`IcDbmsCanisterClient`](crate::prelude::IcDbmsCanisterClient),
which can be used on ic canisters.

If you want to use the client in integration tests with `pocket-ic`, you can use the
[`IcDbmsPocketIcClient`](crate::prelude::IcDbmsPocketIcClient) implementation, but first you need to enable the
`pocket-ic` feature.

If you want to use the client from external systems (such as frontend applications, backend services, or CLI tools),
you can use the [`IcDbmsAgentClient`](crate::prelude::IcDbmsAgentClient) implementation, which requires the `ic-agent`
feature.

## Usage

### Add the dependencies

```toml
[dependencies]
candid = "0.10"
ic-dbms-api = "0.6"
ic-dbms-client = "0.6"
serde = "1"
```

### Implement the record types

You can define your table as you did for the database, or use a common crate to share the types between the canisters.

```rust
use candid::{CandidType, Principal};
use ic_dbms_api::prelude::{Nullable, Query, Table, TableSchema, Text, Uint32, Uint64};
use ic_dbms_client::prelude::{Client as _, IcDbmsCanisterClient};
use serde::Deserialize;

#[derive(Table, CandidType, Clone, Deserialize)]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint64,
    name: Text,
    email: Text,
    age: Nullable<Uint32>,
}
```

### Use the client

```rust

async fn main() -> anyhow::Result<()> {
    let principal = Principal::from_text("...")?;
    let client = IcDbmsCanisterClient::new(principal);

    let alice = UserInsertRequest {
        id: 1.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        age: Nullable::Value(30.into()),
    };

    client
        .insert::<User>(User::table_name(), alice, None)
        .await?;
}
```

### Use the client from external systems (ic-agent)

If you want to use the client from external systems (such as frontend applications, backend services, or CLI tools),
you need to enable the `ic-agent` feature and use the `IcDbmsAgentClient` implementation.

```toml
[dependencies]
ic-dbms-client = { version = "0.9", features = ["ic-agent"] }
ic-agent = "0.45"
```

```rust
use candid::Principal;
use ic_agent::Agent;
use ic_dbms_client::prelude::{Client as _, IcDbmsAgentClient};

async fn main() -> anyhow::Result<()> {
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .build()?;

    let canister_id = Principal::from_text("...")?;
    let client = IcDbmsAgentClient::new(&agent, canister_id);

    let alice = UserInsertRequest {
        id: 1.into(),
        name: "Alice".into(),
        email: "alice@example.com".into(),
        age: Nullable::Value(30.into()),
    };

    client
        .insert::<User>(User::table_name(), alice, None)
        .await?;
}
```

## Available Clients

- `IcDbmsCanisterClient`: Client implementation for IC canisters.
- `IcDbmsAgentClient`: Client implementation for external systems (frontend, backend services, CLI tools) using
  `ic-agent`. Requires the `ic-agent` feature.
- `IcDbmsPocketIcClient`: Client implementation for `pocket-ic` integration tests. Requires the `pocket-ic` feature.

## Available Methods

All the client methods are defined in the `Client` trait.

- `acl_add_principal`
- `acl_remove_principal`
- `acl_allowed_principals`
- `begin_transaction`
- `commit`
- `rollback`
- `select`
- `insert`
- `update`
- `delete`

## Available Types

You can import all the useful types and traits by using the prelude module:

```rust
use ic_dbms_client::prelude::*;
```

### Query

- [`AggregateFunction`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.AggregateFunction.html)
- [`AggregatedRow`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.AggregatedRow.html)
- [`AggregatedValue`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.AggregatedValue.html)
- [`DeleteBehavior`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.DeleteBehavior.html)
- [`Filter`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Filter.html)
- [`JsonCmp`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.JsonCmp.html)
- [`JsonFilter`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.JsonFilter.html)
- [`Query`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Query.html)
- [`QueryBuilder`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.QueryBuilder.html)
- [`OrderDirection`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.OrderDirection.html)
- [`Select`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.Select.html)

### Table

- [`ColumnDef`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.ColumnDef.html)
- [`ForeignKeyDef`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.ForeignKeyDef.html)
- [`InsertRecord`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.InsertRecord.html)
- [`TableColumns`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.TableColumns.html)
- [`TableError`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/enum.TableError.html)
- [`TableRecord`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.TableRecord.html)
- [`UpdateRecord`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.UpdateRecord.html)
- [`ValuesSource`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.ValuesSource.html)

### Types

- [`Blob`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Blob.html)
- [`Boolean`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Boolean.html)
- [`Date`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Date.html)
- [`DateTime`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.DateTime.html)
- [`Decimal`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Decimal.html)
- [`Int8`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Int8.html)
- [`Int16`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Int16.html)
- [`Int32`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Int32.html)
- [`Int64`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Int64.html)
- [`Json`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Json.html)
- [`Nullable`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Nullable.html)
- [`Principal`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Principal.html)
- [`Text`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Text.html)
- [`Uint8`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Uint8.html)
- [`Uint16`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Uint16.html)
- [`Uint32`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Uint32.html)
- [`Uint64`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Uint64.html)
- [`Uuid`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Uuid.html)

### Value

- [`Value`](https://docs.rs/ic-dbms-client/latest/ic_dbms_client/prelude/struct.Value.html)

## License

This project is licensed under the MIT License. See the [LICENSE](../LICENSE) file for details.

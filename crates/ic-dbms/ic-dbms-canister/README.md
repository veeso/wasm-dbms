# ic-dbms-canister

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/ic-dbms-canister.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/ic-dbms-canister.svg?logo=rust)](https://crates.io/crates/ic-dbms-canister)
[![latest-version](https://img.shields.io/crates/v/ic-dbms-canister.svg?logo=rust)](https://crates.io/crates/ic-dbms-canister)
[![ko-fi](https://img.shields.io/badge/donate-ko--fi-red)](https://ko-fi.com/veeso)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/ic-dbms-canister/badge.svg)](https://docs.rs/ic-dbms-canister)

A framework to build database canisters on the Internet Computer, powered by the [wasm-dbms](https://crates.io/crates/wasm-dbms) engine.

Define your data tables using Rust structs, derive the `Table` and `DbmsCanister` traits,
and get a fully functional database canister with CRUD operations, transactions, and ACL-based
access control.

## Usage

### Add dependencies

```toml
[dependencies]
candid = { version = "0.10", features = ["value"] }
ic-cdk = "0.19"
ic-dbms-api = "0.6"
ic-dbms-canister = "0.6"
serde = "1"
```

### Define tables

```rust
use candid::CandidType;
use ic_dbms_api::prelude::{Nullable, Text, Uint32, Uint64};
use ic_dbms_canister::prelude::{DbmsCanister, Table};
use serde::Deserialize;

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    id: Uint64,
    name: Text,
    email: Text,
    age: Nullable<Uint32>,
}

#[derive(Debug, Table, CandidType, Deserialize, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    id: Uint32,
    title: Text,
    content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    author: Uint32,
}

#[derive(DbmsCanister)]
#[tables(User = "users", Post = "posts")]
pub struct IcDbmsCanisterGenerator;
```

### Generated Candid API

```candid
service : (IcDbmsCanisterArgs) -> {
  acl_add_principal : (principal) -> (Result);
  acl_allowed_principals : () -> (vec principal) query;
  acl_remove_principal : (principal) -> (Result);
  begin_transaction : () -> (nat);
  commit : (nat) -> (Result);
  delete_posts : (DeleteBehavior, opt Filter_1, opt nat) -> (Result_1);
  delete_users : (DeleteBehavior, opt Filter_1, opt nat) -> (Result_1);
  insert_posts : (PostInsertRequest, opt nat) -> (Result);
  insert_users : (UserInsertRequest, opt nat) -> (Result);
  rollback : (nat) -> (Result);
  select_posts : (Query, opt nat) -> (Result_2) query;
  select_users : (Query_1, opt nat) -> (Result_3) query;
  update_posts : (PostUpdateRequest, opt nat) -> (Result_1);
  update_users : (UserUpdateRequest, opt nat) -> (Result_1);
}
```

## Interacting with the Canister

See the [ic-dbms-client](https://crates.io/crates/ic-dbms-client) crate for a client library
to interact with the canister from other canisters, frontend applications, or CLI tools.

## Documentation

Read the full documentation at <https://wasm-dbms.cc/ic/>

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

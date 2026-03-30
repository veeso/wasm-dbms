# wasm-dbms-macros

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms-macros.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms-macros.svg?logo=rust)](https://crates.io/crates/wasm-dbms-macros)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms-macros.svg?logo=rust)](https://crates.io/crates/wasm-dbms-macros)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms-macros/badge.svg)](https://docs.rs/wasm-dbms-macros)

Runtime-agnostic procedural macros for the wasm-dbms DBMS engine.

This crate provides procedural macros to automatically implement traits
required by the `wasm-dbms` engine.

## Provided Derive Macros

### `Encode`

Automatically implements the `Encode` trait for structs, generating binary serialization
and deserialization methods for memory storage.

```rust
use wasm_dbms_macros::Encode;

#[derive(Encode, Debug, PartialEq, Eq)]
struct Position {
    x: Int32,
    y: Int32,
}
```

### `Table`

Given a struct representing a database table, automatically implements the `TableSchema`
trait with all the necessary types. Generates:

- `${StructName}Record` - implementing `TableRecord`
- `${StructName}InsertRequest` - implementing `InsertRecord`
- `${StructName}UpdateRequest` - implementing `UpdateRecord`
- `${StructName}ForeignFetcher` (only if foreign keys are present)

```rust
use wasm_dbms_macros::{Encode, Table};

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
struct Post {
    #[primary_key]
    id: Uint32,
    title: Text,
    content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    author_id: Uint32,
}
```

#### Table Attributes

- `#[table = "table_name"]`: Specifies the table name in the database.
- `#[alignment = N]`: (optional) Specifies the alignment for table records.
- `#[primary_key]`: Marks a field as the primary key.
- `#[foreign_key(entity = "EntityName", table = "table_name", column = "column_name")]`: Defines a foreign key relationship.
- `#[sanitizer(SanitizerType)]`: Specifies a sanitizer for the field.
- `#[validate(ValidatorType)]`: Specifies a validator for the field.
- `#[custom_type]`: Marks a field as a custom data type.

### `CustomDataType`

Bridges user-defined types into the `Value` system. The type must also derive `Encode`
and implement `Display`.

```rust
use wasm_dbms_macros::{Encode, CustomDataType};

#[derive(Encode, CustomDataType)]
#[type_tag = "status"]
enum Status {
    Active,
    Inactive,
}
```

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

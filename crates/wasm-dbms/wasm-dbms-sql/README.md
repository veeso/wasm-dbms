# wasm-dbms-sql

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms-sql.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms-sql.svg?logo=rust)](https://crates.io/crates/wasm-dbms-sql)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms-sql.svg?logo=rust)](https://crates.io/crates/wasm-dbms-sql)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms-sql/badge.svg?logo=rust)](https://docs.rs/wasm-dbms-sql)

SQL front-end for the [wasm-dbms](https://crates.io/crates/wasm-dbms) DBMS engine.

This crate parses SQL text, binds it to a `wasm-dbms` schema, and runs it
against a `DbmsContext`. It covers queries (`SELECT` with joins, `DISTINCT`,
aggregates, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`, `OFFSET`), writes
(`INSERT`, `UPDATE`, `DELETE`), and transactions (`BEGIN`, `COMMIT`,
`ROLLBACK`). Tables are still defined with `#[derive(Table)]`: there is no
schema-changing SQL.

## Usage

```toml
[dependencies]
wasm-dbms = "0.9"
wasm-dbms-api = "0.9"
wasm-dbms-memory = "0.9"
wasm-dbms-sql = "0.9"
```

```rust,ignore
use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;
use wasm_dbms_memory::prelude::HeapMemoryProvider;
use wasm_dbms_sql::SqlEngine;

#[derive(Clone, DatabaseSchema)]
#[tables(User = "users")]
pub struct MySchema;

let ctx = DbmsContext::new(HeapMemoryProvider::default());
MySchema::register_tables(&ctx)?;

let engine = SqlEngine::new(MySchema);

engine.execute(
    &ctx,
    None,
    "INSERT INTO users (id, name) VALUES (?, ?)",
    &[Value::from(1u32), Value::from("Alice")],
)?;

let result = engine.execute(
    &ctx,
    None,
    "SELECT name FROM users WHERE id = 1",
    &[],
)?;
```

## Documentation

- [SQL guide](https://wasm-dbms.cc/guides/sql.html)
- [SQL syntax reference](https://wasm-dbms.cc/reference/sql.html)

## License

This project is licensed under the MIT License. See the
[LICENSE](https://github.com/veeso/wasm-dbms/blob/main/LICENSE) file for
details.

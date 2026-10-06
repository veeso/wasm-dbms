#![crate_name = "wasm_dbms_sql"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! # wasm-dbms-sql
//!
//! SQL front-end for the [`wasm-dbms`](https://crates.io/crates/wasm-dbms)
//! DBMS engine.
//!
//! The crate turns SQL text into operations on a `wasm-dbms` database. The
//! schema is still defined with `#[derive(Table)]`; SQL only reads and writes
//! data.
//!
//! ## Supported SQL
//!
//! - `SELECT` with `JOIN`, `WHERE`, `DISTINCT`, aggregate functions,
//!   `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`, and `OFFSET`
//! - `INSERT`, `UPDATE`, and `DELETE`; the last two require a `WHERE` clause
//! - `BEGIN`, `COMMIT`, and `ROLLBACK`, with one transaction per caller
//! - positional `?` parameters
//!
//! The full dialect is described in the
//! [SQL reference](https://wasm-dbms.cc/reference/sql.html).
//!
//! ## Usage
//!
//! [`SqlEngine`] runs statements. It takes the database schema once and the
//! [`DbmsContext`](wasm_dbms::DbmsContext) on every call:
//!
//! ```rust,ignore
//! use wasm_dbms::prelude::*;
//! use wasm_dbms_api::prelude::*;
//! use wasm_dbms_memory::prelude::HeapMemoryProvider;
//! use wasm_dbms_sql::SqlEngine;
//!
//! #[derive(Clone, DatabaseSchema)]
//! #[tables(User = "users")]
//! pub struct MySchema;
//!
//! let ctx = DbmsContext::new(HeapMemoryProvider::default());
//! MySchema::register_tables(&ctx)?;
//! let engine = SqlEngine::new(MySchema);
//!
//! engine.execute(
//!     &ctx,
//!     b"alice",
//!     "INSERT INTO users (id, name) VALUES (?, ?)",
//!     &[Value::from(1u32), Value::from("Alice")],
//! )?;
//!
//! let result = engine.execute(
//!     &ctx,
//!     b"alice",
//!     "SELECT name FROM users WHERE id = 1",
//!     &[],
//! )?;
//! ```
//!
//! [`parse`] exposes the parser on its own. It returns the syntax tree of a
//! statement, as defined in [`ast`], without touching a database.
//!
//! ## Layout
//!
//! | Module    | Role                                                              |
//! |-----------|-------------------------------------------------------------------|
//! | `lexer`   | Splits SQL text into tokens with their line and column            |
//! | `parser`  | Builds the [`ast`] from the tokens                                |
//! | `planner` | Resolves names, converts values, and builds `Query` and `Filter`  |
//! | `engine`  | Runs the plan on the database and tracks transactions             |
//!
//! The result and error types, `SqlResult` and `SqlError`, are defined in
//! `wasm-dbms-api` behind its `sql` feature, which this crate enables.

#![doc(html_playground_url = "https://play.rust-lang.org")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-128.png"
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-512.png"
)]

pub mod ast;
#[cfg(test)]
mod docs_tests;
mod engine;
mod lexer;
mod parser;
mod planner;
#[cfg(test)]
mod test_schema;

pub use self::engine::SqlEngine;
pub use self::parser::{ParsedStatement, parse};

// Rust guideline compliant 2026-02-28

//! `wasm-dbms` provides a runtime-agnostic DBMS engine for WASM
//! environments.
//!
//! This crate contains:
//! - [`DbmsContext`] — owns all database state
//! - [`WasmDbmsDatabase`] — session-scoped CRUD / transaction operations
//! - [`DatabaseSchema`] — trait for schema-driven dynamic dispatch
//! - [`JoinEngine`] — nested-loop cross-table joins
//! - Integrity validators for insert and update operations
//! - Transaction overlay for MVCC-like read-your-writes semantics

extern crate self as wasm_dbms;

mod context;
mod database;
pub mod integrity;
pub mod join;
pub mod referenced_tables;
pub mod schema;
pub mod transaction;

pub use self::context::DbmsContext;
pub use self::database::WasmDbmsDatabase;

/// Prelude re-exports for convenient use.
pub mod prelude {
    pub use super::context::DbmsContext;
    pub use super::database::WasmDbmsDatabase;
    pub use super::integrity::{InsertIntegrityValidator, UpdateIntegrityValidator};
    pub use super::join::JoinEngine;
    pub use super::referenced_tables::get_referenced_tables;
    pub use super::schema::DatabaseSchema;
    pub use super::transaction::DatabaseOverlay;
    pub use super::transaction::session::TransactionSession;
}

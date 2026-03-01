//! Re-exports all the most commonly used items from this crate.

pub use ic_dbms_api::prelude::*;
pub use ic_dbms_macros::{DatabaseSchema, DbmsCanister};
// Re-export from wasm-dbms instead of deleted IC modules.
// NOTE: Both the `DatabaseSchema` derive macro (from ic_dbms_macros, above)
// and the `DatabaseSchema` trait (from wasm_dbms, below) coexist here.
// Rust distinguishes them via separate macro/type namespaces. The generated
// code from `#[derive(DatabaseSchema)]` references the trait at this path.
pub use wasm_dbms::prelude::{
    DatabaseSchema, DbmsContext, InsertIntegrityValidator, UpdateIntegrityValidator,
    WasmDbmsDatabase, get_referenced_tables,
};
pub use wasm_dbms::transaction::session::TransactionSession;
pub use wasm_dbms_memory::prelude::{AccessControl, MemoryProvider};

pub use crate::memory::{DBMS_CONTEXT, IcAccessControlList, IcMemoryProvider};

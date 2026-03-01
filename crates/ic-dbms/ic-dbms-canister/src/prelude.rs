//! Re-exports all the most commonly used items from this crate.

pub use ic_dbms_api::prelude::*;
pub use ic_dbms_macros::DbmsCanister;

// Re-export from wasm-dbms instead of deleted IC modules
pub use wasm_dbms::prelude::{
    DatabaseSchema, DbmsContext, InsertIntegrityValidator, UpdateIntegrityValidator,
    WasmDbmsDatabase, get_referenced_tables,
};
pub use wasm_dbms::transaction::session::TransactionSession;

pub use crate::memory::{DBMS_CONTEXT, IcMemoryProvider};

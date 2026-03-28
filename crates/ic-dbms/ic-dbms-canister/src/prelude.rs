//! Re-exports all the most commonly used items from this crate.

pub use ic_dbms_api::prelude::*;
pub use ic_dbms_macros::DbmsCanister;
pub use wasm_dbms::prelude::{
    DatabaseSchema, DbmsContext, InsertIntegrityValidator, UpdateIntegrityValidator,
    WasmDbmsDatabase, get_referenced_tables,
};
pub use wasm_dbms::transaction::session::TransactionSession;
pub use wasm_dbms_macros::DatabaseSchema;
pub use wasm_dbms_memory::prelude::{AccessControl, MemoryProvider};

pub use crate::memory::{DBMS_CONTEXT, IcAccessControlList, IcMemoryProvider};

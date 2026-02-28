//! Re-exports all the most commonly used items from this crate.

pub use ic_dbms_api::prelude::*;
pub use ic_dbms_macros::DbmsCanister;

pub use crate::dbms::IcDbmsDatabase;
pub use crate::dbms::integrity::{InsertIntegrityValidator, UpdateIntegrityValidator};
pub use crate::dbms::referenced_tables::get_referenced_tables;
pub use crate::dbms::schema::DatabaseSchema;
pub use crate::dbms::transaction::TRANSACTION_SESSION;
pub use crate::memory::{ACL, MEMORY_MANAGER, SCHEMA_REGISTRY, SchemaRegistry};

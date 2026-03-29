// Rust guideline compliant 2026-02-28

//! `wasm-dbms-memory` provides runtime-agnostic memory abstraction and
//! page management for the wasm-dbms framework.
//!
//! This crate contains:
//! - [`MemoryProvider`] trait for abstracting memory backends
//! - [`HeapMemoryProvider`] for testing
//! - [`MemoryManager`] for page-level memory operations
//! - [`SchemaRegistry`] for table schema persistence
//! - [`AccessControl`] trait for pluggable access control
//! - [`AccessControlList`] for identity-based access control
//! - [`NoAccessControl`] for runtimes without access control
//! - [`TableRegistry`] for record-level storage and retrieval

extern crate self as wasm_dbms_memory;

mod acl;
mod memory_access;
mod memory_manager;
mod provider;
mod schema_registry;
pub mod table_registry;

pub use self::acl::{AccessControl, AccessControlList, NoAccessControl};
pub use self::memory_access::MemoryAccess;
pub use self::memory_manager::{MemoryManager, align_up};
pub use self::provider::{HeapMemoryProvider, MemoryProvider, WASM_PAGE_SIZE};
pub use self::schema_registry::{SchemaRegistry, TableRegistryPage};
pub use self::table_registry::{
    IndexLedger, IndexTreeWalker, NextRecord, RecordAddress, TableReader, TableRegistry,
};

/// Prelude re-exports for convenient use.
pub mod prelude {
    pub use super::acl::{AccessControl, AccessControlList, NoAccessControl};
    pub use super::memory_access::MemoryAccess;
    pub use super::memory_manager::{MemoryManager, align_up};
    pub use super::provider::{HeapMemoryProvider, MemoryProvider, WASM_PAGE_SIZE};
    pub use super::schema_registry::{SchemaRegistry, TableRegistryPage};
    pub use super::table_registry::{
        IndexLedger, IndexTreeWalker, NextRecord, RecordAddress, TableReader, TableRegistry,
    };
}

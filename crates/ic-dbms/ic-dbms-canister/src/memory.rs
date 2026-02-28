//! Memory module provides stable memory management for the IC DBMS Canister.
//!
//! This module re-exports types from `wasm-dbms-memory` and provides
//! thread-local instances for the memory manager, schema registry, and ACL.

mod provider;

use std::cell::RefCell;

pub use wasm_dbms_memory::*;

// instantiate a static memory manager with the stable memory provider
thread_local! {
    #[cfg(target_family = "wasm")]
    pub static MEMORY_MANAGER: RefCell<MemoryManager<provider::IcMemoryProvider>> = RefCell::new(MemoryManager::init(
        provider::IcMemoryProvider::default(),
    ));

    #[cfg(not(target_family = "wasm"))]
    pub static MEMORY_MANAGER: RefCell<MemoryManager<HeapMemoryProvider>> = RefCell::new(MemoryManager::init(
        HeapMemoryProvider::default()
    ));

    /// The global schema registry.
    ///
    /// We allow failing because on first initialization the schema registry might not be present yet.
    pub static SCHEMA_REGISTRY: RefCell<SchemaRegistry> = RefCell::new(
        MEMORY_MANAGER.with_borrow(|mm| SchemaRegistry::load(mm).unwrap_or_default())
    );

    /// The global ACL.
    ///
    /// We allow failing because on first initialization the ACL might not be present yet.
    pub static ACL: RefCell<AccessControlList> = RefCell::new(
        MEMORY_MANAGER.with_borrow(|mm| AccessControlList::load(mm).unwrap_or_default())
    );
}

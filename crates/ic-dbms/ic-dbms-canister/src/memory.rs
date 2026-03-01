//! Memory module provides stable memory management for the IC DBMS Canister.
//!
//! This module re-exports types from `wasm-dbms-memory` and provides
//! a thread-local [`DbmsContext`] that consolidates the memory manager,
//! schema registry, ACL, and transaction session.

mod provider;

#[cfg(target_family = "wasm")]
pub type IcMemoryProvider = provider::IcMemoryProvider;

#[cfg(not(target_family = "wasm"))]
pub type IcMemoryProvider = wasm_dbms_memory::HeapMemoryProvider;

thread_local! {
    pub static DBMS_CONTEXT: wasm_dbms::prelude::DbmsContext<IcMemoryProvider> =
        wasm_dbms::prelude::DbmsContext::new(IcMemoryProvider::default());
}

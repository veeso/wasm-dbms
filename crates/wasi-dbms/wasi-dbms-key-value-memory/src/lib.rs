// Rust guideline compliant 2026-10-09

//! Cached key-value [`MemoryProvider`](wasm_dbms_memory::MemoryProvider)
//! implementation for `wasm-dbms`.
//!
//! The provider keeps the logical database in a contiguous in-memory cache and
//! exposes a narrow [`KeyValueStore`] seam for storage adapters and tests.

mod manifest;
mod provider;
mod store;

#[cfg(target_os = "wasi")]
mod bindings;

pub use provider::WasiKeyValueMemoryProvider;
pub use store::{KeyValueStore, WasiKeyValueStore};

#[cfg(test)]
mod tests;

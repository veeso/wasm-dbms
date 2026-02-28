// Rust guideline compliant 2026-02-28

//! WIT Component Model guest example for wasm-dbms.
//!
//! This crate demonstrates how to build a wasm-dbms guest module
//! that can be loaded by any WASM runtime via the WIT Component Model.

pub mod file_provider;
pub mod schema;

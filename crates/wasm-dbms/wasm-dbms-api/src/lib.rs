#![crate_name = "wasm_dbms_api"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! # WASM DBMS API
//!
//! Runtime-agnostic API types and traits for the wasm-dbms engine.
//!
//! This crate provides all shared types, traits, and abstractions needed
//! to interact with a wasm-dbms instance. It is independent of any specific
//! WASM runtime (IC, WASI, Wasmtime, etc.).
//!
//! Import all useful types and traits via the prelude:
//!
//! ```rust
//! use wasm_dbms_api::prelude::*;
//! ```
//!
//! ## Feature flags
//!
//! - `candid`: Enables `CandidType` derives on all public types and exposes
//!   Candid-specific API boundary types (`JoinColumnDef`, `CandidDataTypeKind`).

#![doc(html_playground_url = "https://play.rust-lang.org")]

// Makes the crate accessible as `wasm_dbms_api` in macros.
extern crate self as wasm_dbms_api;

pub mod dbms;
pub mod error;
pub mod memory;
pub mod prelude;
#[cfg(test)]
mod tests;
pub mod utils;

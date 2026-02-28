#![crate_name = "ic_dbms_api"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! # IC DBMS API
//!
//! IC-specific API types for ic-dbms-canister.
//!
//! This crate re-exports all generic types from [`wasm_dbms_api`] and adds
//! IC-specific types such as [`Principal`](crate::prelude::Principal) and
//! canister init arguments.
//!
//! Import all useful types and traits via the prelude:
//!
//! ```rust
//! use ic_dbms_api::prelude::*;
//! ```

#![doc(html_playground_url = "https://play.rust-lang.org")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-128.png"
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-512.png"
)]

// Makes the crate accessible as `ic_dbms_api` in macros.
extern crate self as ic_dbms_api;

// Re-export generic modules from wasm-dbms-api for path compatibility.
pub use wasm_dbms_api::dbms;
pub use wasm_dbms_api::memory;
pub use wasm_dbms_api::utils;

mod error;
mod init;
mod principal;
pub mod prelude;
#[cfg(test)]
mod tests;

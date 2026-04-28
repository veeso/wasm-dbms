#![crate_name = "ic_dbms_macros"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! Macros and derive for ic-dbms-canister
//!
//! This crate provides procedural macros to automatically implement traits
//! required by the `ic-dbms-canister`.
//!
//! ## Provided Derive Macros
//!
//! - `DbmsCanister`: Automatically implements the API for the ic-dbms-canister.
//!
//! All other derive macros (`Encode`, `Table`, `DatabaseSchema`, `CustomDataType`)
//! are provided by `wasm-dbms-macros` and re-exported through the
//! `ic-dbms-api` and `ic-dbms-canister` preludes.

#![doc(html_playground_url = "https://play.rust-lang.org")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-128.png"
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-512.png"
)]

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod dbms_canister;

/// Automatically implements the api for the ic-dbms-canister with all the required methods to interact with the ACL and
/// the defined tables.
#[proc_macro_derive(DbmsCanister, attributes(tables))]
pub fn derive_dbms_canister(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    self::dbms_canister::dbms_canister(input)
        .expect("failed to derive `DbmsCanister`")
        .into()
}

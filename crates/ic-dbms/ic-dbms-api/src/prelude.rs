//! Prelude exposes all the types for the `ic-dbms-api` crate.

// Re-export derive macros from wasm-dbms-macros.
// Re-export everything from wasm-dbms-api prelude.
pub use wasm_dbms_api::prelude::*;
pub use wasm_dbms_macros::{CustomDataType, Encode, Table};

// IC-specific types.
pub use crate::error::{IcDbmsError, IcDbmsResult};
pub use crate::init::{IcDbmsCanisterArgs, IcDbmsCanisterInitArgs, IcDbmsCanisterUpgradeArgs};
pub use crate::principal::Principal;

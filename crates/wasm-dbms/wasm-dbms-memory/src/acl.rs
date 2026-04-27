// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Access control list with granular per-identity permissions.

mod list;
mod no_acl;
mod traits;

pub use self::list::AccessControlList;
pub use self::no_acl::NoAccessControl;
pub use self::traits::AccessControl;

//! Inspect implementation for the IC DBMS canister.
//!
//! Each DBMS canister must be called by an authorized principal, which must be in the ACL.

use super::assert_caller_is_allowed;

/// Handles an inspect call to the canister.
pub fn inspect() {
    assert_caller_is_allowed();

    ic_cdk::api::accept_message();
}

//! Inspect implementation for the IC DBMS canister.
//!
//! With granular ACL the per-call body returns `AccessDenied` for
//! unauthorized requests, so inspect-message accepts every call and lets
//! the body do the real work.

/// Handles an inspect call to the canister.
pub fn inspect() {
    ic_cdk::api::accept_message();
}

use candid::CandidType;
use ic_dbms_api::prelude::{Encode, TableSchema, Text, Uint32};
use wasm_dbms_macros::Table;

/// A simple user struct for testing purposes.
#[derive(Debug, Table, CandidType, Clone, PartialEq, Eq)]
#[candid]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    #[validate(crate::prelude::EmailValidator)]
    pub email: Text,
    #[sanitizer(crate::prelude::ClampUnsignedSanitizer, min = 0, max = 120)]
    pub age: Uint32,
}

pub const USERS_FIXTURES: &[&str] = &[
    "Alice", "Bob", "Charlie", "Diana", "Eve", "Frank", "Grace", "Heidi", "Ivan", "Judy",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_encode_decode() {
        let user = User {
            id: 42u32.into(),
            name: "Alice".into(),
            email: "alice@example.com".into(),
            age: 30.into(),
        };
        let encoded = user.encode();
        let decoded = User::decode(encoded).unwrap();
        assert_eq!(user, decoded);
    }

    #[test]
    fn test_should_have_fingerprint() {
        let fingerprint = User::fingerprint();
        assert_ne!(fingerprint, 0);
    }
}

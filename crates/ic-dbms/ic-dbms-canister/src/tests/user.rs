use candid::CandidType;
use ic_dbms_api::prelude::{Encode, TableSchema, Text, Uint32};
use ic_dbms_macros::Table;

use crate::memory::{MEMORY_MANAGER, SCHEMA_REGISTRY, TableRegistry};

/// A simple user struct for testing purposes.
#[derive(Debug, Table, CandidType, Clone, PartialEq, Eq)]
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

/// Loads fixtures into the database for testing purposes.
///
/// # Panics
///
/// Panics if any operation fails.
pub fn load_fixtures() {
    // register tables
    let user_pages = SCHEMA_REGISTRY
        .with_borrow_mut(|sr| {
            MEMORY_MANAGER.with_borrow_mut(|mm| sr.register_table::<User>(mm))
        })
        .expect("failed to register `User` table");

    MEMORY_MANAGER.with_borrow_mut(|mm| {
        let mut user_table: TableRegistry =
            TableRegistry::load(user_pages, mm).expect("failed to load `User` table registry");

        // insert users
        for (id, user) in USERS_FIXTURES.iter().enumerate() {
            let user = User {
                id: Uint32(id as u32),
                name: Text(user.to_string()),
                email: Text(format!("{}@example.com", user.to_lowercase())),
                age: (20 + id as u32).into(),
            };
            user_table.insert(user, mm).expect("failed to insert user");
        }
    });
}

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

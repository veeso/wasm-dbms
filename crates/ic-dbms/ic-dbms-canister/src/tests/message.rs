use candid::CandidType;
use ic_dbms_api::prelude::{DateTime, Nullable, Text, Uint32};
use ic_dbms_macros::Table;

use crate::memory::{MEMORY_MANAGER, SCHEMA_REGISTRY, TableRegistry};
use crate::tests::{User, UserRecord};

/// A simple message struct for testing purposes.
#[derive(Debug, Table, CandidType, Clone, PartialEq, Eq)]
#[table = "messages"]
pub struct Message {
    #[primary_key]
    pub id: Uint32,
    pub text: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub sender: Uint32,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub recipient: Uint32,
    pub read_at: Nullable<DateTime>,
}

pub const MESSAGES_FIXTURES: &[(&str, u32, u32)] = &[
    ("Hello, World!", 0, 1),
    ("How are you?", 1, 0),
    ("Goodbye!", 1, 3),
];

pub fn load_fixtures() {
    // register tables
    let messages_pages = SCHEMA_REGISTRY
        .with_borrow_mut(|sr| MEMORY_MANAGER.with_borrow_mut(|mm| sr.register_table::<Message>(mm)))
        .expect("failed to register `Message` table");

    MEMORY_MANAGER.with_borrow_mut(|mm| {
        let mut messages_table: TableRegistry = TableRegistry::load(messages_pages, mm)
            .expect("failed to load `Message` table registry");

        // insert messages
        for (id, (text, sender_id, recipient_id)) in MESSAGES_FIXTURES.iter().enumerate() {
            let post = Message {
                id: Uint32(id as u32),
                text: Text(text.to_string()),
                sender: Uint32(*sender_id),
                recipient: Uint32(*recipient_id),
                read_at: Nullable::Null,
            };
            messages_table
                .insert(post, mm)
                .expect("failed to insert message");
        }
    });
}

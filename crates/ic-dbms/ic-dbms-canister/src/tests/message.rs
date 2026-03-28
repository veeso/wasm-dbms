use candid::CandidType;
use ic_dbms_api::prelude::{DateTime, Nullable, Text, Uint32};
use wasm_dbms_macros::Table;

use crate::tests::{User, UserRecord};

/// A simple message struct for testing purposes.
#[derive(Debug, Table, CandidType, Clone, PartialEq, Eq)]
#[candid]
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

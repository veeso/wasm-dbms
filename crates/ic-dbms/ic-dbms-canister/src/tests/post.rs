//! Post mock type; 1 user has many posts.

use candid::CandidType;
use ic_dbms_api::prelude::{Text, Uint32};
use wasm_dbms_macros::Table;

use crate::tests::{User, UserRecord};

/// A simple post struct for testing purposes.
///
/// One [`super::User`] has many [`Post`]s.
#[derive(Debug, Table, CandidType, Clone, PartialEq, Eq)]
#[candid]
#[table = "posts"]
#[alignment = 64]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub content: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user: Uint32,
}

pub const POSTS_FIXTURES: &[(&str, &str, u32)] = &[
    ("First Post", "This is the content of the first post.", 0),
    ("Second Post", "This is the content of the second post.", 0),
    ("Third Post", "This is the content of the third post.", 1),
    ("Fourth Post", "This is the content of the fourth post.", 1),
    ("Fifth Post", "This is the content of the fifth post.", 2),
    ("Sixth Post", "This is the content of the sixth post.", 2),
    (
        "Seventh Post",
        "This is the content of the seventh post.",
        3,
    ),
    ("Eighth Post", "This is the content of the eighth post.", 3),
    ("Ninth Post", "This is the content of the ninth post.", 4),
    ("Tenth Post", "This is the content of the tenth post.", 4),
    (
        "Eleventh Post",
        "This is the content of the eleventh post.",
        5,
    ),
    (
        "Twelfth Post",
        "This is the content of the twelfth post.",
        5,
    ),
    (
        "Thirteenth Post",
        "This is the content of the thirteenth post.",
        6,
    ),
    (
        "Fourteenth Post",
        "This is the content of the fourteenth post.",
        6,
    ),
    (
        "Fifteenth Post",
        "This is the content of the fifteenth post.",
        7,
    ),
    (
        "Sixteenth Post",
        "This is the content of the sixteenth post.",
        7,
    ),
    (
        "Seventeenth Post",
        "This is the content of the seventeenth post.",
        8,
    ),
    (
        "Eighteenth Post",
        "This is the content of the eighteenth post.",
        8,
    ),
    (
        "Nineteenth Post",
        "This is the content of the nineteenth post.",
        9,
    ),
    (
        "Twentieth Post",
        "This is the content of the twentieth post.",
        9,
    ),
];

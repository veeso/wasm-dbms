// Rust guideline compliant 2026-03-01

//! Test types, fixtures and mocks.

mod message;
mod post;
mod user;

use ic_dbms_api::prelude::Database as _;
use wasm_dbms::prelude::WasmDbmsDatabase;

#[allow(unused_imports)]
pub use self::message::{
    MESSAGES_FIXTURES, Message, MessageInsertRequest, MessageRecord, MessageUpdateRequest,
};
#[allow(unused_imports)]
pub use self::post::{POSTS_FIXTURES, Post, PostInsertRequest, PostRecord, PostUpdateRequest};
#[allow(unused_imports)]
pub use self::user::{USERS_FIXTURES, User, UserInsertRequest, UserRecord, UserUpdateRequest};
use crate::memory::DBMS_CONTEXT;
use crate::prelude::DatabaseSchema;

#[derive(DatabaseSchema)]
#[tables(User = "users", Post = "posts", Message = "messages")]
pub struct TestDatabaseSchema;

/// Loads fixtures into the database for testing purposes.
///
/// Registers all test tables and inserts fixture data via [`WasmDbmsDatabase`].
///
/// # Panics
///
/// Panics if any operation fails.
pub fn load_fixtures() {
    DBMS_CONTEXT.with(|ctx| {
        TestDatabaseSchema::register_tables(ctx).expect("failed to register tables");

        let db = WasmDbmsDatabase::oneshot(ctx, TestDatabaseSchema);

        // Insert users
        for (id, name) in USERS_FIXTURES.iter().enumerate() {
            let record = UserInsertRequest {
                id: (id as u32).into(),
                name: name.to_string().into(),
                email: format!("{}@example.com", name.to_lowercase()).into(),
                age: (20 + id as u32).into(),
            };
            db.insert::<User>(record).expect("failed to insert user");
        }

        // Insert posts
        for (id, (title, content, user_id)) in POSTS_FIXTURES.iter().enumerate() {
            let record = PostInsertRequest {
                id: (id as u32).into(),
                title: title.to_string().into(),
                content: content.to_string().into(),
                user: (*user_id).into(),
            };
            db.insert::<Post>(record).expect("failed to insert post");
        }

        // Insert messages
        for (id, (text, sender_id, recipient_id)) in MESSAGES_FIXTURES.iter().enumerate() {
            let record = MessageInsertRequest {
                id: (id as u32).into(),
                text: text.to_string().into(),
                sender: (*sender_id).into(),
                recipient: (*recipient_id).into(),
                read_at: ic_dbms_api::prelude::Nullable::Null,
            };
            db.insert::<Message>(record)
                .expect("failed to insert message");
        }
    });
}

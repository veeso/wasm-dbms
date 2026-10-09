//! Tables and schema shared by the planner and engine tests.

use wasm_dbms_api::prelude::{
    Blob, Boolean, ColumnDef, Date, DateTime, Decimal, Int64, Json, Nullable, Table,
    TableSchema as _, Text, Uint8, Uint32, Uint64, Uuid,
};
use wasm_dbms_macros::DatabaseSchema;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub(crate) struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub email: Nullable<Text>,
    pub age: Uint8,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub(crate) struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub published: Boolean,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user_id: Uint32,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "comments"]
pub(crate) struct Comment {
    #[primary_key]
    pub id: Uint32,
    pub body: Text,
    #[foreign_key(entity = "Post", table = "posts", column = "id")]
    pub post_id: Uint32,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "sales"]
pub(crate) struct Sale {
    #[primary_key]
    pub id: Uint32,
    #[index(group = "idx_category_region")]
    pub category: Text,
    #[index(group = "idx_category_region")]
    pub region: Text,
    pub quantity: Uint32,
    #[index]
    pub bonus: Nullable<Uint32>,
    pub price: Decimal,
}

/// One column of every type that needs a string literal or a typed parameter.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "events"]
pub(crate) struct Event {
    #[primary_key]
    #[autoincrement]
    pub id: Uint64,
    pub day: Date,
    pub at: DateTime,
    pub payload: Blob,
    pub meta: Json,
    pub delta: Int64,
    pub token: Uuid,
}

#[derive(Debug, Clone, Copy, DatabaseSchema)]
#[tables(
    User = "users",
    Post = "posts",
    Comment = "comments",
    Sale = "sales",
    Event = "events"
)]
pub(crate) struct TestSchema;

/// Column definitions of the test tables, by table name.
pub(crate) fn catalog(table: &str) -> Option<&'static [ColumnDef]> {
    match table {
        "users" => Some(User::columns()),
        "posts" => Some(Post::columns()),
        "comments" => Some(Comment::columns()),
        "sales" => Some(Sale::columns()),
        "events" => Some(Event::columns()),
        _ => None,
    }
}

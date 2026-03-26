use wasm_dbms_api::prelude::{Text, Uint32};
use wasm_dbms_macros::{DatabaseSchema, Table};

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
    pub email: Text,
    pub age: Uint32,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    #[primary_key]
    pub id: Uint32,
    pub title: Text,
    pub body: Text,
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user_id: Uint32,
}

#[derive(Debug, DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct BenchDatabaseSchema;

/// SQL CREATE TABLE statement for the users table.
pub const CREATE_USERS_SQL: &str = "CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    age INTEGER NOT NULL
)";

/// SQL CREATE TABLE statement for the posts table.
pub const CREATE_POSTS_SQL: &str = "CREATE TABLE posts (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id)
)";

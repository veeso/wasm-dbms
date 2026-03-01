// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, X-NO-MOD-RS, M-CANONICAL-DOCS

//! Example table schemas using the [`DatabaseSchema`] derive macro.
//!
//! Defines `User` and `Post` tables along with a derived
//! [`ExampleDatabaseSchema`] that dispatches generic DBMS
//! operations to the correct table type.

use wasm_dbms_api::prelude::{MaxStrlenValidator, Text, TrimSanitizer, Uint32};
use wasm_dbms_macros::{DatabaseSchema, Table};

// ---------- Table definitions ----------

/// Users table.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "users"]
pub struct User {
    /// Primary key identifier.
    #[primary_key]
    pub id: Uint32,
    /// Display name (trimmed, max 20 characters).
    #[sanitizer(TrimSanitizer)]
    #[validate(MaxStrlenValidator(20))]
    pub name: Text,
    /// Email address.
    pub email: Text,
}

/// Posts table with a foreign key referencing `users`.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "posts"]
pub struct Post {
    /// Primary key identifier.
    #[primary_key]
    pub id: Uint32,
    /// Post title.
    pub title: Text,
    /// Post content body.
    pub content: Text,
    /// Foreign key to the owning user.
    #[foreign_key(entity = "User", table = "users", column = "id")]
    pub user: Uint32,
}

// ---------- DatabaseSchema implementation ----------

/// Schema implementation that dispatches operations to the
/// `User` and `Post` table types.
#[derive(Debug, DatabaseSchema)]
#[tables(User = "users", Post = "posts")]
pub struct ExampleDatabaseSchema;

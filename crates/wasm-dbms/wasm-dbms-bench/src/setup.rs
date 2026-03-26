use duckdb::Connection as DuckConnection;
use rusqlite::Connection as SqliteConnection;
use wasm_dbms::prelude::{DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{Database, Text, Uint32};
use wasm_dbms_memory::prelude::NoAccessControl;

use crate::data::{DataGenerator, PostData, UserData};
use crate::provider::HashMapMemoryProvider;
use crate::schema::{
    BenchDatabaseSchema, CREATE_POSTS_SQL, CREATE_USERS_SQL, Post, PostInsertRequest, User,
    UserInsertRequest,
};

// ── wasm-dbms ──

pub type BenchDbmsContext = DbmsContext<HashMapMemoryProvider, NoAccessControl>;

/// Creates a wasm-dbms context with registered schema (empty tables).
pub fn setup_wasm_dbms() -> BenchDbmsContext {
    let ctx: BenchDbmsContext = DbmsContext::with_acl(HashMapMemoryProvider::default());
    ctx.register_table::<User>()
        .expect("failed to register users table");
    ctx.register_table::<Post>()
        .expect("failed to register posts table");
    ctx
}

/// Inserts user records into a wasm-dbms database.
pub fn populate_wasm_dbms_users(ctx: &BenchDbmsContext, users: &[UserData]) {
    let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);
    for u in users {
        let req = UserInsertRequest {
            id: Uint32(u.id),
            name: Text(u.name.clone()),
            email: Text(u.email.clone()),
            age: Uint32(u.age),
        };
        db.insert::<User>(req)
            .expect("wasm-dbms: insert user failed");
    }
}

/// Inserts post records into a wasm-dbms database.
pub fn populate_wasm_dbms_posts(ctx: &BenchDbmsContext, posts: &[PostData]) {
    let db = WasmDbmsDatabase::oneshot(ctx, BenchDatabaseSchema);
    for p in posts {
        let req = PostInsertRequest {
            id: Uint32(p.id),
            title: Text(p.title.clone()),
            body: Text(p.body.clone()),
            user_id: Uint32(p.user_id),
        };
        db.insert::<Post>(req)
            .expect("wasm-dbms: insert post failed");
    }
}

/// Sets up a wasm-dbms context pre-populated with users.
pub fn setup_wasm_dbms_with_users(count: u32) -> BenchDbmsContext {
    let ctx = setup_wasm_dbms();
    let users = DataGenerator::new().users(count);
    populate_wasm_dbms_users(&ctx, &users);
    ctx
}

/// Sets up a wasm-dbms context pre-populated with users and posts.
pub fn setup_wasm_dbms_with_users_and_posts(num_users: u32, num_posts: u32) -> BenchDbmsContext {
    let ctx = setup_wasm_dbms();
    let mut data_gen = DataGenerator::new();
    let users = data_gen.users(num_users);
    let posts = data_gen.posts(num_posts, num_users);
    populate_wasm_dbms_users(&ctx, &users);
    populate_wasm_dbms_posts(&ctx, &posts);
    ctx
}

// ── rusqlite ──

/// Creates an in-memory SQLite database with the schema created.
pub fn setup_rusqlite() -> SqliteConnection {
    let conn = SqliteConnection::open_in_memory().expect("rusqlite: open failed");
    conn.execute_batch(CREATE_USERS_SQL)
        .expect("rusqlite: create users failed");
    conn.execute_batch(CREATE_POSTS_SQL)
        .expect("rusqlite: create posts failed");
    conn
}

/// Inserts user records into a rusqlite database.
pub fn populate_rusqlite_users(conn: &SqliteConnection, users: &[UserData]) {
    let mut stmt = conn
        .prepare("INSERT INTO users (id, name, email, age) VALUES (?1, ?2, ?3, ?4)")
        .expect("rusqlite: prepare insert users failed");
    for u in users {
        stmt.execute(rusqlite::params![u.id, u.name, u.email, u.age])
            .expect("rusqlite: insert user failed");
    }
}

/// Inserts post records into a rusqlite database.
pub fn populate_rusqlite_posts(conn: &SqliteConnection, posts: &[PostData]) {
    let mut stmt = conn
        .prepare("INSERT INTO posts (id, title, body, user_id) VALUES (?1, ?2, ?3, ?4)")
        .expect("rusqlite: prepare insert posts failed");
    for p in posts {
        stmt.execute(rusqlite::params![p.id, p.title, p.body, p.user_id])
            .expect("rusqlite: insert post failed");
    }
}

/// Sets up rusqlite pre-populated with users.
pub fn setup_rusqlite_with_users(count: u32) -> SqliteConnection {
    let conn = setup_rusqlite();
    let users = DataGenerator::new().users(count);
    populate_rusqlite_users(&conn, &users);
    conn
}

/// Sets up rusqlite pre-populated with users and posts.
pub fn setup_rusqlite_with_users_and_posts(num_users: u32, num_posts: u32) -> SqliteConnection {
    let conn = setup_rusqlite();
    let mut data_gen = DataGenerator::new();
    let users = data_gen.users(num_users);
    let posts = data_gen.posts(num_posts, num_users);
    populate_rusqlite_users(&conn, &users);
    populate_rusqlite_posts(&conn, &posts);
    conn
}

// ── duckdb ──

/// Creates an in-memory DuckDB database with the schema created.
pub fn setup_duckdb() -> DuckConnection {
    let conn = DuckConnection::open_in_memory().expect("duckdb: open failed");
    conn.execute_batch(CREATE_USERS_SQL)
        .expect("duckdb: create users failed");
    conn.execute_batch(CREATE_POSTS_SQL)
        .expect("duckdb: create posts failed");
    conn
}

/// Inserts user records into a DuckDB database.
pub fn populate_duckdb_users(conn: &DuckConnection, users: &[UserData]) {
    let mut stmt = conn
        .prepare("INSERT INTO users (id, name, email, age) VALUES (?, ?, ?, ?)")
        .expect("duckdb: prepare insert users failed");
    for u in users {
        stmt.execute(duckdb::params![u.id, u.name, u.email, u.age])
            .expect("duckdb: insert user failed");
    }
}

/// Inserts post records into a DuckDB database.
pub fn populate_duckdb_posts(conn: &DuckConnection, posts: &[PostData]) {
    let mut stmt = conn
        .prepare("INSERT INTO posts (id, title, body, user_id) VALUES (?, ?, ?, ?)")
        .expect("duckdb: prepare insert posts failed");
    for p in posts {
        stmt.execute(duckdb::params![p.id, p.title, p.body, p.user_id])
            .expect("duckdb: insert post failed");
    }
}

/// Sets up DuckDB pre-populated with users.
pub fn setup_duckdb_with_users(count: u32) -> DuckConnection {
    let conn = setup_duckdb();
    let users = DataGenerator::new().users(count);
    populate_duckdb_users(&conn, &users);
    conn
}

/// Sets up DuckDB pre-populated with users and posts.
pub fn setup_duckdb_with_users_and_posts(num_users: u32, num_posts: u32) -> DuckConnection {
    let conn = setup_duckdb();
    let mut data_gen = DataGenerator::new();
    let users = data_gen.users(num_users);
    let posts = data_gen.posts(num_posts, num_users);
    populate_duckdb_users(&conn, &users);
    populate_duckdb_posts(&conn, &posts);
    conn
}

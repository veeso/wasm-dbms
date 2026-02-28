// Rust guideline compliant 2026-02-28

//! Wasmtime host binary for the wasm-dbms WIT Component Model example.
//!
//! Loads the guest WASM component, instantiates it with WASI support,
//! and exercises every exported `database` operation: insert, select,
//! transactional commit, and transactional rollback.

use std::env;
use std::path::PathBuf;

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Result, Store};
use wasmtime_wasi::p2::add_to_linker_sync;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// Generate host-side bindings from the WIT definition.
//
// The `dbms` world exports a `database` interface with `select`, `insert`,
// `update`, `delete`, `begin-transaction`, `commit`, and `rollback`.
wasmtime::component::bindgen!({
    world: "dbms",
    path: "../../../../wit/dbms.wit",
});

use crate::wasm_dbms::dbms::types::{ColumnValue, DbmsError, OrderDirection, Query, Value};

/// Default path to the pre-built guest component.
const DEFAULT_GUEST_PATH: &str = ".artifact/wasm-dbms-example-guest.wasm";

/// Name of the database file the guest writes into the preopened directory.
const DB_FILE: &str = "wasm-dbms.db";

// ── Host state ──────────────────────────────────────────────────────

/// Holds WASI context required by the guest component.
struct HostState {
    wasi_ctx: WasiCtx,
    resource_table: ResourceTable,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Builds a single [`ColumnValue`].
fn col(name: &str, value: Value) -> ColumnValue {
    ColumnValue {
        name: name.to_string(),
        value,
    }
}

/// Builds a [`Query`] that returns all rows ordered by `column` ascending.
fn select_all_asc(column: &str) -> Query {
    Query {
        filter: None,
        order_by: Some(column.to_string()),
        order_dir: Some(OrderDirection::Asc),
        limit: None,
        offset: None,
    }
}

/// Builds a [`Query`] with a JSON-serialised equality filter.
fn select_eq(column: &str, value: &str) -> Query {
    let filter = format!(r#"{{"Eq":["{column}",{value}]}}"#);
    Query {
        filter: Some(filter),
        order_by: None,
        order_dir: None,
        limit: None,
        offset: None,
    }
}

/// Pretty-prints a list of result rows.
fn print_rows(rows: &[Vec<ColumnValue>]) {
    for row in rows {
        let fields = row
            .iter()
            .map(|cv| format_column_value(cv))
            .collect::<Vec<String>>();
        println!("  {{ {} }}", fields.join(", "));
    }
}

/// Formats a single column-value pair for display.
fn format_column_value(cv: &ColumnValue) -> String {
    let val = match &cv.value {
        Value::BoolVal(b) => b.to_string(),
        Value::U8Val(n) => n.to_string(),
        Value::U16Val(n) => n.to_string(),
        Value::U32Val(n) => n.to_string(),
        Value::U64Val(n) => n.to_string(),
        Value::I8Val(n) => n.to_string(),
        Value::I16Val(n) => n.to_string(),
        Value::I32Val(n) => n.to_string(),
        Value::I64Val(n) => n.to_string(),
        Value::F32Val(f) => f.to_string(),
        Value::F64Val(f) => f.to_string(),
        Value::TextVal(s) => format!("\"{s}\""),
        Value::BlobVal(b) => format!("<blob {} bytes>", b.len()),
        Value::NullVal => "NULL".to_string(),
    };
    format!("{}: {val}", cv.name)
}

/// Formats a [`DbmsError`] for display.
fn format_error(e: &DbmsError) -> String {
    match e {
        DbmsError::TableNotFound(t) => format!("TableNotFound({t})"),
        DbmsError::ValidationError(v) => format!("ValidationError({v})"),
        DbmsError::IntegrityError(i) => format!("IntegrityError({i})"),
        DbmsError::TransactionError(t) => format!("TransactionError({t})"),
        DbmsError::MemoryError(m) => format!("MemoryError({m})"),
        DbmsError::IoError(io) => format!("IoError({io})"),
    }
}

/// Converts a guest [`DbmsError`] into a [`wasmtime::Error`].
fn dbms_err(e: DbmsError) -> wasmtime::Error {
    wasmtime::Error::msg(format_error(&e))
}

// ── Entry point ─────────────────────────────────────────────────────

fn main() -> Result<()> {
    let guest_path = env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_GUEST_PATH.to_string());
    let guest_path = PathBuf::from(&guest_path);

    // Resolve the working directory used as the guest's preopened root.
    // The database file is written here by the guest component.
    let work_dir = env::current_dir().map_err(wasmtime::Error::new)?;

    println!("=== wasm-dbms WIT Component Model Demo ===");
    println!("Guest component : {}", guest_path.display());
    println!("Working dir     : {}", work_dir.display());
    println!();

    // 1. Engine with component-model support.
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // 2. Linker with WASI host functions.
    let mut linker: Linker<HostState> = Linker::new(&engine);
    add_to_linker_sync(&mut linker)?;

    // 3. WASI context with the current directory preopened as "/".
    let mut wasi_builder = WasiCtxBuilder::new();
    wasi_builder.inherit_stdio();
    wasi_builder.preopened_dir(&work_dir, "/", DirPerms::all(), FilePerms::all())?;
    let wasi_ctx = wasi_builder.build();

    let state = HostState {
        wasi_ctx,
        resource_table: ResourceTable::new(),
    };
    let mut store = Store::new(&engine, state);

    // 4. Load and instantiate the guest component.
    let component = Component::from_file(&engine, &guest_path)?;
    let bindings = Dbms::instantiate(&mut store, &component, &linker)?;

    let db = bindings.wasm_dbms_dbms_database();

    // ── Insert users ────────────────────────────────────────────────
    println!("--- Inserting users ---");
    let users = [
        ("Alice", "alice@example.com", 1u32),
        ("Bob", "bob@example.com", 2),
        ("Charlie", "charlie@example.com", 3),
    ];
    for (name, email, id) in &users {
        let row = vec![
            col("id", Value::U32Val(*id)),
            col("name", Value::TextVal(name.to_string())),
            col("email", Value::TextVal(email.to_string())),
        ];
        db.call_insert(&mut store, "users", &row, None)?
            .map_err(dbms_err)?;
        println!("  Inserted user {id}: {name}");
    }
    println!();

    // ── Insert posts ────────────────────────────────────────────────
    println!("--- Inserting posts ---");
    let posts = [
        (1u32, "Hello World", "First post!", 1u32),
        (2, "Rust Tips", "Use iterators.", 2),
    ];
    for (id, title, content, user) in &posts {
        let row = vec![
            col("id", Value::U32Val(*id)),
            col("title", Value::TextVal(title.to_string())),
            col("content", Value::TextVal(content.to_string())),
            col("user", Value::U32Val(*user)),
        ];
        db.call_insert(&mut store, "posts", &row, None)?
            .map_err(dbms_err)?;
        println!("  Inserted post {id}: \"{title}\" by user {user}");
    }
    println!();

    // ── Select all users ────────────────────────────────────────────
    println!("--- Select all users (ordered by id ASC) ---");
    let rows = db
        .call_select(&mut store, "users", &select_all_asc("id"))?
        .map_err(dbms_err)?;
    print_rows(&rows);
    println!();

    // ── Select posts filtered by user=1 ─────────────────────────────
    println!("--- Select posts where user = 1 ---");
    let rows = db
        .call_select(
            &mut store,
            "posts",
            &select_eq("user", r#"{"Uint32":1}"#),
        )?
        .map_err(dbms_err)?;
    print_rows(&rows);
    println!();

    // ── Transaction: commit ─────────────────────────────────────────
    println!("--- Transaction: commit (insert user 4 Diana) ---");
    let tx = db
        .call_begin_transaction(&mut store)?
        .map_err(dbms_err)?;
    println!("  Transaction started: tx={tx}");

    let diana_row = vec![
        col("id", Value::U32Val(4)),
        col("name", Value::TextVal("Diana".to_string())),
        col("email", Value::TextVal("diana@example.com".to_string())),
    ];
    db.call_insert(&mut store, "users", &diana_row, Some(tx))?
        .map_err(dbms_err)?;
    println!("  Inserted Diana (uncommitted)");

    db.call_commit(&mut store, tx)?.map_err(dbms_err)?;
    println!("  Committed.");

    // Verify Diana exists.
    let rows = db
        .call_select(
            &mut store,
            "users",
            &select_eq("id", r#"{"Uint32":4}"#),
        )?
        .map_err(dbms_err)?;
    assert!(!rows.is_empty(), "Diana should exist after commit");
    println!("  Verified: Diana exists after commit.");
    print_rows(&rows);
    println!();

    // ── Transaction: rollback ───────────────────────────────────────
    println!("--- Transaction: rollback (insert user 5 Eve) ---");
    let tx = db
        .call_begin_transaction(&mut store)?
        .map_err(dbms_err)?;
    println!("  Transaction started: tx={tx}");

    let eve_row = vec![
        col("id", Value::U32Val(5)),
        col("name", Value::TextVal("Eve".to_string())),
        col("email", Value::TextVal("eve@example.com".to_string())),
    ];
    db.call_insert(&mut store, "users", &eve_row, Some(tx))?
        .map_err(dbms_err)?;
    println!("  Inserted Eve (uncommitted)");

    db.call_rollback(&mut store, tx)?.map_err(dbms_err)?;
    println!("  Rolled back.");

    // Verify Eve does NOT exist.
    let rows = db
        .call_select(
            &mut store,
            "users",
            &select_eq("id", r#"{"Uint32":5}"#),
        )?
        .map_err(dbms_err)?;
    assert!(rows.is_empty(), "Eve should NOT exist after rollback");
    println!("  Verified: Eve does NOT exist after rollback.");
    println!();

    // ── Final state ─────────────────────────────────────────────────
    println!("--- Final users table ---");
    let rows = db
        .call_select(&mut store, "users", &select_all_asc("id"))?
        .map_err(dbms_err)?;
    print_rows(&rows);
    println!();

    // ── Cleanup ─────────────────────────────────────────────────────
    let db_path = work_dir.join(DB_FILE);
    if db_path.exists() {
        std::fs::remove_file(&db_path).ok();
        println!("Cleaned up database file: {}", db_path.display());
    }

    println!();
    println!("=== Demo complete ===");
    Ok(())
}

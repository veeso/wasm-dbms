// Rust guideline compliant 2026-02-28

//! Wasmtime host binary for the wasm-dbms WIT Component Model example.
//!
//! Loads the guest WASM component, instantiates it with WASI support,
//! and exercises every exported `database` operation: insert, select,
//! transactional commit, and transactional rollback.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, io};

use wasmtime::component::{Component, Linker, ResourceTable};
use wasmtime::{Config, Engine, Result, Store};
use wasmtime_wasi::p2::add_to_linker_sync;
use wasmtime_wasi::{FsPerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

// Generate host-side bindings from the WIT definition.
//
// The `dbms` world exports a `database` interface with `select`, `insert`,
// `update`, `delete`, `begin-transaction`, `commit`, and `rollback`.
wasmtime::component::bindgen!({
    world: "dbms",
    path: "../../../../wit/dbms.wit",
});

use crate::wasm_dbms::dbms::types::{
    ColumnValue, DbmsError, MigrationPolicy, OrderDirection, OrderKey, Query, Value,
};

/// Default path to the pre-built guest component.
const DEFAULT_GUEST_PATH: &str = ".artifact/wasm-dbms-example-guest.wasm";

/// Name of the database file the guest writes into the preopened directory.
const DB_FILE: &str = "wasm-dbms.db";

/// Name prefix of the per-invocation directories created by the demo.
const WORK_DIR_PREFIX: &str = "wasm-dbms-example";

// ── Demo storage ────────────────────────────────────────────────────

/// A directory created exclusively for one demo run.
///
/// The guest only sees this directory, preopened as its root, so the demo
/// never reads or writes files that existed before it started. The directory
/// and everything the guest wrote into it are removed when the value is
/// dropped.
struct DemoWorkDir {
    path: PathBuf,
}

impl DemoWorkDir {
    /// Creates a new, uniquely named directory inside `base`.
    ///
    /// The name combines the process ID, the current time in nanoseconds,
    /// and a per-process counter. The directory is created with
    /// [`fs::create_dir`], so an existing directory is never reused.
    ///
    /// # Errors
    ///
    /// Returns the I/O error from [`fs::create_dir`], for example when `base`
    /// does not exist or a directory with the same name already exists.
    fn create_in(base: &Path) -> io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let name = format!(
            "{WORK_DIR_PREFIX}-{pid}-{nanos}-{count}",
            pid = std::process::id(),
            count = COUNTER.fetch_add(1, Ordering::Relaxed),
        );
        let path = base.join(name);
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    /// Returns the path of the directory.
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DemoWorkDir {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.path) {
            eprintln!(
                "Failed to remove demo directory {path}: {e}",
                path = self.path.display()
            );
        }
    }
}

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

/// Builds an empty [`Query`] with no clauses set.
fn empty_query() -> Query {
    Query {
        filter: None,
        distinct_by: vec![],
        eager_relations: vec![],
        joins: vec![],
        group_by: vec![],
        having: None,
        order_by: vec![],
        limit: None,
        offset: None,
    }
}

/// Builds a [`Query`] that returns all rows ordered by `column` ascending.
fn select_all_asc(column: &str) -> Query {
    Query {
        order_by: vec![OrderKey {
            column: column.to_string(),
            direction: OrderDirection::Asc,
        }],
        ..empty_query()
    }
}

/// Builds a [`Query`] with a JSON-serialised equality filter.
fn select_eq(column: &str, value: &str) -> Query {
    let filter = format!(r#"{{"Eq":["{column}",{value}]}}"#);
    Query {
        filter: Some(filter),
        ..empty_query()
    }
}

/// Pretty-prints a list of result rows.
fn print_rows(rows: &[Vec<ColumnValue>]) {
    for row in rows {
        let fields = row.iter().map(format_column_value).collect::<Vec<String>>();
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
        Value::TextVal(s) => format!("\"{s}\""),
        Value::BlobVal(b) => format!("<blob {} bytes>", b.len()),
        Value::DecimalVal(s) => s.clone(),
        Value::DateVal(s) => s.clone(),
        Value::DatetimeVal(s) => s.clone(),
        Value::JsonVal(s) => s.clone(),
        Value::UuidVal(s) => s.clone(),
        Value::CustomVal(c) => format!("<custom {}: {}>", c.type_tag, c.display),
        Value::NullVal => "NULL".to_string(),
    };
    format!("{}: {val}", cv.name)
}

/// Formats a [`DbmsError`] for display.
fn format_error(e: &DbmsError) -> String {
    format!("{e:?}")
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

    // Create a fresh directory used as the guest's preopened root. The guest
    // writes its database file here, isolated from any existing data.
    let work_dir = DemoWorkDir::create_in(&env::temp_dir()).map_err(wasmtime::Error::new)?;

    println!("=== wasm-dbms WIT Component Model Demo ===");
    println!("Guest component : {}", guest_path.display());
    println!("Demo dir        : {}", work_dir.path().display());
    println!();

    // 1. Engine with component-model support.
    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config)?;

    // 2. Linker with WASI host functions.
    let mut linker: Linker<HostState> = Linker::new(&engine);
    add_to_linker_sync(&mut linker)?;

    // 3. WASI context with the demo directory preopened as "/".
    let mut wasi_builder = WasiCtxBuilder::new();
    wasi_builder.inherit_stdio();
    wasi_builder.preopened_dir(work_dir.path(), "/", FsPerms::ReadWrite)?;
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
        .call_select(&mut store, "posts", &select_eq("user", r#"{"Uint32":1}"#))?
        .map_err(dbms_err)?;
    print_rows(&rows);
    println!();

    // ── Transaction: commit ─────────────────────────────────────────
    println!("--- Transaction: commit (insert user 4 Diana) ---");
    let tx = db.call_begin_transaction(&mut store)?.map_err(dbms_err)?;
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
        .call_select(&mut store, "users", &select_eq("id", r#"{"Uint32":4}"#))?
        .map_err(dbms_err)?;
    assert!(!rows.is_empty(), "Diana should exist after commit");
    println!("  Verified: Diana exists after commit.");
    print_rows(&rows);
    println!();

    // ── Transaction: rollback ───────────────────────────────────────
    println!("--- Transaction: rollback (insert user 5 Eve) ---");
    let tx = db.call_begin_transaction(&mut store)?.map_err(dbms_err)?;
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
        .call_select(&mut store, "users", &select_eq("id", r#"{"Uint32":5}"#))?
        .map_err(dbms_err)?;
    assert!(rows.is_empty(), "Eve should NOT exist after rollback");
    println!("  Verified: Eve does NOT exist after rollback.");
    println!();

    // ── Schema migrations ───────────────────────────────────────────
    println!("--- Schema migrations smoke test ---");
    let drift = db.call_has_drift(&mut store)?.map_err(dbms_err)?;
    println!("  has_drift = {drift}");
    let ops = db.call_pending_migrations(&mut store)?.map_err(dbms_err)?;
    println!("  pending_migrations.len() = {}", ops.len());
    let policy = MigrationPolicy {
        allow_destructive: false,
    };
    db.call_migrate(&mut store, policy)?.map_err(dbms_err)?;
    println!("  migrate() applied (no-op)");
    println!();

    // ── Final state ─────────────────────────────────────────────────
    println!("--- Final users table ---");
    let rows = db
        .call_select(&mut store, "users", &select_all_asc("id"))?
        .map_err(dbms_err)?;
    print_rows(&rows);
    println!();

    // ── Cleanup ─────────────────────────────────────────────────────
    // Release the guest's handles before removing the directory it used.
    drop(store);
    println!(
        "Removing demo directory and its database file: {path}",
        path = work_dir.path().join(DB_FILE).display()
    );
    drop(work_dir);

    println!();
    println!("=== Demo complete ===");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;

    /// Creates a fresh base directory standing in for a user's working directory.
    fn create_base_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let base = env::temp_dir().join(format!(
            "wasm-dbms-host-test-{label}-{pid}-{nanos}",
            pid = std::process::id(),
        ));
        fs::create_dir(&base).expect("create base dir");
        base
    }

    fn remove_base_dir(base: &Path) {
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn test_demo_work_dir_preserves_preexisting_database_file() {
        let base = create_base_dir("preserve");
        let existing = base.join(DB_FILE);
        fs::write(&existing, b"preexisting database").expect("write preexisting database");

        let work_dir = DemoWorkDir::create_in(&base).expect("create demo dir");
        let work_path = work_dir.path().to_path_buf();
        assert_ne!(work_path, base);
        assert!(work_path.starts_with(&base));
        fs::write(work_path.join(DB_FILE), b"demo data").expect("write demo database");
        drop(work_dir);

        let preserved = fs::read(&existing);
        let work_path_exists = work_path.exists();
        remove_base_dir(&base);

        assert!(
            !work_path_exists,
            "demo directory must be removed on cleanup"
        );
        assert_eq!(
            preserved.expect("preexisting database must survive"),
            b"preexisting database"
        );
    }

    #[test]
    fn test_demo_work_dirs_are_distinct_per_invocation() {
        let base = create_base_dir("distinct");

        let first = DemoWorkDir::create_in(&base).expect("create first demo dir");
        let second = DemoWorkDir::create_in(&base).expect("create second demo dir");
        let distinct = first.path() != second.path();
        let both_exist = first.path().is_dir() && second.path().is_dir();
        drop(first);
        drop(second);
        remove_base_dir(&base);

        assert!(distinct, "each invocation must get its own directory");
        assert!(both_exist);
    }

    #[test]
    fn test_demo_work_dir_creation_fails_for_missing_base() {
        let base = create_base_dir("missing");
        let missing = base.join("does-not-exist");

        let outcome = DemoWorkDir::create_in(&missing);
        remove_base_dir(&base);

        assert!(
            outcome.is_err(),
            "the demo directory must be created, not reused"
        );
    }
}

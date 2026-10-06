//! End-to-end check that the demo never touches a database file in the working directory.
//!
//! Needs the guest component built by `just build_wasm_dbms_example`; when it is missing the
//! test is skipped with a message on stderr.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

const DB_FILE: &str = "wasm-dbms.db";

/// Path of the guest component built at the workspace root.
fn guest_component() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .join(".artifact/wasm-dbms-example-guest.wasm")
}

#[test]
fn test_host_demo_preserves_a_preexisting_database_file() {
    let guest = guest_component();
    if !guest.is_file() {
        eprintln!(
            "skipped: {} not found, run `just build_wasm_dbms_example`",
            guest.display()
        );
        return;
    }

    let dir = env::temp_dir().join(format!("wasm-dbms-host-it-{}", std::process::id()));
    fs::create_dir(&dir).expect("create working directory");
    let database = dir.join(DB_FILE);
    fs::write(&database, b"preexisting database").expect("write preexisting database");

    let output = Command::new(env!("CARGO_BIN_EXE_wasm-dbms-example"))
        .arg(&guest)
        .current_dir(&dir)
        .output()
        .expect("run host demo");
    let preserved = fs::read(&database);
    let _ = fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "demo failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        preserved.expect("preexisting database must survive"),
        b"preexisting database"
    );
}

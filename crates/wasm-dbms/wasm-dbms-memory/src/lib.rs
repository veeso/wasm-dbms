// Rust guideline compliant 2026-04-28
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

#![crate_name = "wasm_dbms_memory"]
#![crate_type = "lib"]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

//! # wasm-dbms-memory
//!
//! Runtime-agnostic memory abstraction and page management for the
//! [`wasm-dbms`](https://crates.io/crates/wasm-dbms) framework.
//!
//! This crate sits between the raw byte storage exposed by a WASM
//! runtime (heap, file, IC stable memory, ...) and the higher-level
//! DBMS engine. It defines the [`MemoryProvider`] trait that abstracts
//! over the storage backend and the on-top-of-it data structures that
//! the engine uses to persist tables, schema, indexes, and access
//! control state.
//!
//! All structures use 64 KiB pages so the on-disk layout is byte-for-byte
//! identical across providers — a heap-built database can be dumped and
//! reopened as IC stable memory or a WASI file with no conversion.
//!
//! ## What this crate provides
//!
//! Storage backends:
//!
//! - [`MemoryProvider`] — trait every backend implements (read, write,
//!   grow, page count).
//! - [`HeapMemoryProvider`] — in-memory provider for tests and embedded
//!   use cases.
//!
//! Access control:
//!
//! - [`AccessControl`] — pluggable access control trait.
//! - [`AccessControlList`] — identity-based per-table permissions
//!   persisted on a dedicated page.
//! - [`NoAccessControl`] — zero-overhead provider for runtimes that do
//!   not need access control.
//!
//! Engine-facing data structures:
//!
//! - [`MemoryManager`] — page allocator and low-level read/write
//!   helpers. See [`RESERVED_PAGES`] for the reserved layout.
//! - [`SchemaRegistry`] — persistent table-schema store backed by
//!   [`TableRegistryPage`].
//! - [`TableRegistry`] — record-level storage, free-segment tracking,
//!   and read iterators ([`TableReader`], [`RawTableReader`],
//!   [`NextRecord`], [`RecordAddress`], [`RawRecordBytes`]).
//! - [`IndexLedger`] / [`IndexTreeWalker`] — secondary index storage
//!   and traversal.
//! - [`table_registry::AutoincrementLedger`] — per-column
//!   autoincrement counters.
//! - [`UnclaimedPages`] — free page pool ([`UNCLAIMED_PAGES_CAPACITY`]
//!   entries per ledger page).
//! - [`align_up`] / [`WASM_PAGE_SIZE`] — alignment helpers.
//!
//! ## Memory layout
//!
//! Reserved pages followed by per-table page sets:
//!
//! ```text
//! +0:  Schema Registry            (1 page)
//! +1:  ACL Table                  (1 page)
//! +N:  Per-table Page Ledger      (1 page)
//! +N:  Per-table Free Segments    (1 page)
//! +N:  Per-table Record Pages     (grown on demand)
//! ```
//!
//! ## Quick start
//!
//! ```rust
//! use wasm_dbms_memory::prelude::*;
//!
//! let mut provider = HeapMemoryProvider::default();
//! provider.grow(1).unwrap();
//! provider.write(0, b"hello").unwrap();
//!
//! let mut buf = vec![0u8; 5];
//! provider.read(0, &mut buf).unwrap();
//! assert_eq!(&buf, b"hello");
//! ```
//!
//! Most users do not interact with this crate directly: the
//! [`wasm-dbms`](https://crates.io/crates/wasm-dbms) engine consumes a
//! [`MemoryProvider`] and exposes the higher-level CRUD/transaction
//! API on top.

#![doc(html_playground_url = "https://play.rust-lang.org")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-128.png"
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/veeso/wasm-dbms/main/assets/images/cargo/logo-512.png"
)]

extern crate self as wasm_dbms_memory;

mod acl;
mod memory_access;
mod memory_manager;
mod provider;
mod schema_registry;
pub mod table_registry;
mod unclaimed_pages;

pub use self::acl::{AccessControl, AccessControlList, NoAccessControl};
pub use self::memory_access::MemoryAccess;
pub use self::memory_manager::{MemoryManager, RESERVED_PAGES, align_up};
pub use self::provider::{HeapMemoryProvider, MemoryProvider, WASM_PAGE_SIZE};
pub use self::schema_registry::{SchemaRegistry, TableRegistryPage};
pub use self::table_registry::{
    IndexLedger, IndexTreeWalker, NextRecord, RawRecordBytes, RawTableReader, RecordAddress,
    TableReader, TableRegistry,
};
pub use self::unclaimed_pages::{UNCLAIMED_PAGES_CAPACITY, UnclaimedPages};

/// Prelude re-exports for convenient use.
pub mod prelude {
    pub use super::acl::{AccessControl, AccessControlList, NoAccessControl};
    pub use super::memory_access::MemoryAccess;
    pub use super::memory_manager::{MemoryManager, RESERVED_PAGES, align_up};
    pub use super::provider::{HeapMemoryProvider, MemoryProvider, WASM_PAGE_SIZE};
    pub use super::schema_registry::{SchemaRegistry, TableRegistryPage};
    pub use super::table_registry::{
        AutoincrementLedger, IndexLedger, IndexTreeWalker, NextRecord, RawRecordBytes,
        RawTableReader, RecordAddress, TableReader, TableRegistry,
    };
    pub use super::unclaimed_pages::{UNCLAIMED_PAGES_CAPACITY, UnclaimedPages};
}

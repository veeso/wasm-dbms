# wasm-dbms-memory

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasm-dbms-memory.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasm-dbms-memory.svg?logo=rust)](https://crates.io/crates/wasm-dbms-memory)
[![latest-version](https://img.shields.io/crates/v/wasm-dbms-memory.svg?logo=rust)](https://crates.io/crates/wasm-dbms-memory)
[![ko-fi](https://img.shields.io/badge/donate-ko--fi-red)](https://ko-fi.com/veeso)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasm-dbms-memory/badge.svg)](https://docs.rs/wasm-dbms-memory)

Runtime-agnostic memory abstraction and page management for the wasm-dbms framework.

This crate provides the storage layer used by `wasm-dbms`, handling page-level memory
operations, schema persistence, access control, and record-level storage.

## Components

- [`MemoryProvider`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/trait.MemoryProvider.html) - Trait for abstracting memory backends (stable memory, heap, etc.)
- [`HeapMemoryProvider`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/struct.HeapMemoryProvider.html) - In-memory implementation for testing
- [`MemoryManager`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/struct.MemoryManager.html) - Page-level memory operations
- [`SchemaRegistry`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/struct.SchemaRegistry.html) - Table schema persistence
- [`AccessControlList`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/struct.AccessControlList.html) - Identity-based access control
- [`TableRegistry`](https://docs.rs/wasm-dbms-memory/latest/wasm_dbms_memory/struct.TableRegistry.html) - Record-level storage and retrieval

## Memory Model

The memory is organized into 64 KiB pages:

```text
+-------------------------------------+
| Schema Registry (1 page)            |
+-------------------------------------+
| ACL Table (1 page)                  |
+-------------------------------------+
| Table XX Page Ledger (1 page)       |
| Table XX Free Segments Ledger       |
+-------------------------------------+
| Table YY Page Ledger (1 page)       |
| Table YY Free Segments Ledger       |
+-------------------------------------+
| Table XX Records - Page 1           |
| Table XX Records - Page 2           |
| Table YY Records - Page 1           |
| ...                                 |
+-------------------------------------+
```

## Usage

```rust
use wasm_dbms_memory::prelude::*;

// Use HeapMemoryProvider for testing
let provider = HeapMemoryProvider::default();
let mut manager = MemoryManager::new(&provider);
```

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

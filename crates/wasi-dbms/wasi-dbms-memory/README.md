# wasi-dbms-memory

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasi-dbms-memory.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasi-dbms-memory.svg?logo=rust)](https://crates.io/crates/wasi-dbms-memory)
[![latest-version](https://img.shields.io/crates/v/wasi-dbms-memory.svg?logo=rust)](https://crates.io/crates/wasi-dbms-memory)
[![conventional-commits](https://img.shields.io/badge/Conventional%20Commits-1.0.0-%23FE5196?logo=conventionalcommits&logoColor=white)](https://conventionalcommits.org)

[![ci](https://github.com/veeso/wasm-dbms/actions/workflows/ci.yml/badge.svg)](https://github.com/veeso/wasm-dbms/actions)
[![coveralls](https://coveralls.io/repos/github/veeso/wasm-dbms/badge.svg)](https://coveralls.io/github/veeso/wasm-dbms)
[![docs](https://docs.rs/wasi-dbms-memory/badge.svg)](https://docs.rs/wasi-dbms-memory)

WASI file-backed `MemoryProvider` implementation for the wasm-dbms framework.

This crate provides `WasiMemoryProvider`, a persistent storage backend
that uses a single flat file on the filesystem, enabling `wasm-dbms` to run
on any WASI-compliant runtime (Wasmer, Wasmtime, WasmEdge, etc.) with durable
data persistence.

## Components

- [`WasiMemoryProvider`](https://docs.rs/wasi-dbms-memory/latest/wasi_dbms_memory/struct.WasiMemoryProvider.html) - File-backed `MemoryProvider` for WASI runtimes

## How It Works

The backing file is byte-for-byte equivalent to IC stable memory: a contiguous
sequence of 64 KiB pages, zero-filled on allocation. This means database
snapshots are portable across different `MemoryProvider` implementations.

- **`new(path)`** opens or creates the file. Existing files have their page
  count inferred from file size. Non-page-aligned files are rejected.
- **`grow(n)`** extends the file by `n * 64 KiB` zero-filled bytes.
- **`read`/`write`** operate at arbitrary byte offsets with bounds checking.

## Usage

```rust
use wasi_dbms_memory::WasiMemoryProvider;
use wasm_dbms::DbmsContext;

let provider = WasiMemoryProvider::new("./data/mydb.bin").unwrap();
let ctx = DbmsContext::new(provider, acl);
// use ctx with wasm-dbms as usual
```

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

# WASI Memory Provider

- [Overview](#overview)
- [Installation](#installation)
- [Usage](#usage)
  - [Creating a Provider](#creating-a-provider)
  - [Using with DbmsContext](#using-with-dbmscontext)
  - [TryFrom Conversions](#tryfrom-conversions)
- [File Layout and Portability](#file-layout-and-portability)
- [Error Handling](#error-handling)
- [Concurrency](#concurrency)
- [Comparison with Other Providers](#comparison-with-other-providers)

---

## Overview

The `wasi-dbms-memory` crate provides `WasiMemoryProvider`, a persistent file-backed
implementation of the `MemoryProvider` trait. It enables wasm-dbms databases to run on
any WASI-compliant runtime (Wasmer, Wasmtime, WasmEdge, etc.) with durable data
persistence across process restarts.

The provider stores all database pages in a single flat file on the filesystem.
Each page is 64 KiB (65,536 bytes), matching the WASM memory page size.

---

## Installation

Add `wasi-dbms-memory` to your `Cargo.toml`:

```toml
[dependencies]
wasi-dbms-memory = "0.7"
```

The crate depends on `wasm-dbms-api` and `wasm-dbms-memory` (pulled in transitively).

---

## Usage

### Creating a Provider

```rust
use wasi_dbms_memory::WasiMemoryProvider;
use wasm_dbms_memory::MemoryProvider;

// Opens the file if it exists, or creates it empty.
let mut provider = WasiMemoryProvider::new("./data/mydb.bin").unwrap();

// Allocate pages as needed.
provider.grow(1).unwrap(); // 1 page = 64 KiB

// Read and write at arbitrary offsets.
provider.write(0, b"hello").unwrap();

let mut buf = vec![0u8; 5];
provider.read(0, &mut buf).unwrap();
assert_eq!(&buf, b"hello");
```

When opening an existing file, the page count is inferred from the file size. The file
size must be a multiple of 64 KiB; otherwise `WasiMemoryProvider::new` returns an error.

The parent directory must already exist before creating the provider.

### Using with DbmsContext

Pass the provider directly to `DbmsContext`:

```rust
use wasi_dbms_memory::WasiMemoryProvider;
use wasm_dbms::DbmsContext;

let provider = WasiMemoryProvider::new("./data/mydb.bin").unwrap();
let ctx = DbmsContext::new(provider, acl);
```

From this point on, all database operations use the file as persistent storage.

### TryFrom Conversions

The crate provides `TryFrom` implementations for convenient construction:

```rust
use std::path::{Path, PathBuf};
use wasi_dbms_memory::WasiMemoryProvider;

// From &Path
let provider = WasiMemoryProvider::try_from(Path::new("./mydb.bin")).unwrap();

// From PathBuf
let path = PathBuf::from("./mydb.bin");
let provider = WasiMemoryProvider::try_from(path).unwrap();
```

---

## File Layout and Portability

The backing file is byte-for-byte equivalent to IC stable memory: a contiguous sequence
of 64 KiB pages, zero-filled on allocation. This means database snapshots are portable
between different `MemoryProvider` implementations.

A file created by `WasiMemoryProvider` can be loaded by any provider that uses the same
page layout, and vice versa. This enables workflows such as:

- Exporting a database from an IC canister and loading it locally for debugging
- Developing and testing with WASI, then deploying to the Internet Computer
- Migrating data between different WASM runtimes

---

## Error Handling

All operations return `MemoryResult<T>` (an alias for `Result<T, MemoryError>`).
The possible errors are:

| Error | Cause |
|-------|-------|
| `MemoryError::OutOfBounds` | Read or write beyond allocated memory |
| `MemoryError::ProviderError(String)` | File I/O failure, or file size not page-aligned |

---

## Concurrency

`WasiMemoryProvider` assumes single-writer access. WASM is single-threaded by default,
so this is generally not a concern. If you run multiple instances pointing at the same
file, you are responsible for external synchronization. WASI file-lock support varies
across runtimes.

---

## Comparison with Other Providers

| Provider | Use case | Backing storage |
|----------|----------|-----------------|
| `WasiMemoryProvider` | WASI production | Single flat file on filesystem |
| `IcMemoryProvider` | IC production | IC stable memory APIs |
| `HeapMemoryProvider` | Testing | In-process `Vec<u8>` |

All three share the same page layout, so data is portable across implementations.

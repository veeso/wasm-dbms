# wasi-dbms-key-value-memory

![logo](https://wasm-dbms.cc/logo-128.png)

[![license-mit](https://img.shields.io/crates/l/wasi-dbms-key-value-memory.svg?logo=rust)](https://opensource.org/licenses/MIT)
[![repo-stars](https://img.shields.io/github/stars/veeso/wasm-dbms?style=flat)](https://github.com/veeso/wasm-dbms/stargazers)
[![downloads](https://img.shields.io/crates/d/wasi-dbms-key-value-memory.svg?logo=rust)](https://crates.io/crates/wasi-dbms-key-value-memory)
[![latest-version](https://img.shields.io/crates/v/wasi-dbms-key-value-memory.svg?logo=rust)](https://crates.io/crates/wasi-dbms-key-value-memory)

Cached WASI draft2 key-value storage for `wasm-dbms`. Use it when the host
provides `wasi:keyvalue/store@0.2.0-draft2` and
`wasi:keyvalue/batch@0.2.0-draft2`.

## Components

- `WasiKeyValueMemoryProvider`: cached `MemoryProvider` implementation.
- `KeyValueStore`: narrow native testing and embedding seam.
- Crate-local draft2 WIT bindings, isolated from the application WIT world.

## How It Works

The provider opens one bucket and stores a database namespace under
`wasm-dbms/<lowercase-hex-of-name>/`. It caches the complete logical memory in
the guest and writes raw 64 KiB pages to two alternating slots. A small head
manifest publishes the selected slot for every page only after all page batches
have succeeded. The manifest hashes every referenced page and retains the
previous checkpoint. If a new component instance observes the head before all
current pages, it rejects stale page values and loads a usable previous
checkpoint. If the current checkpoint cannot be validated, opening fails when
no non-empty, valid previous checkpoint is available. A fallback checkpoint
stays read-only for that provider instance, even if the current pages become
visible later; reopen the provider to validate the current checkpoint and
resume writes.

`flush()` is the durability boundary. A clean flush performs no host calls;
unflushed writes are visible to `export_snapshot()` but are not durable. If
head publication fails, the provider is poisoned and must be dropped and
reopened because the host may have applied the head before reporting an error.
The format assumes one writer and a host that gives the caller read-your-writes
and durable bucket backing.

The cache is bounded by host-addressable memory and the provider's `u32` page
limit. Host key-value value-size and request-size limits still apply; page
batches contain at most 16 values. `import_snapshot()` accepts an empty,
page-aligned provider and marks the imported pages dirty until an explicit
flush.

## Usage

```rust,no_run
use wasi_dbms_key_value_memory::WasiKeyValueMemoryProvider;
use wasm_dbms::DbmsContext;

let provider = WasiKeyValueMemoryProvider::new("default", "example")?;
let ctx = DbmsContext::new(provider);
// Register tables and perform committed operations.
ctx.flush()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The host must expose the draft2 store and batch interfaces and keep the named
bucket across component restarts. The provider is not a multi-writer protocol;
coordinate writers at the embedding layer.

## License

This project is licensed under the MIT License. See the [LICENSE](../../../LICENSE) file for details.

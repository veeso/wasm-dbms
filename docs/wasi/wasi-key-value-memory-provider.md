# WASI Key-Value Memory Provider

- [WASI Key-Value Memory Provider](#wasi-key-value-memory-provider)
  - [When to Use It](#when-to-use-it)
  - [Usage](#usage)
  - [Checkpoint and Restart Model](#checkpoint-and-restart-model)
  - [Limits and Host Requirements](#limits-and-host-requirements)
  - [File Provider Alternative](#file-provider-alternative)

`wasi-dbms-key-value-memory` stores the wasm-dbms page memory in a WASI
key-value bucket. It targets the draft2 interfaces
`wasi:keyvalue/store@0.2.0-draft2` and `wasi:keyvalue/batch@0.2.0-draft2`.
Use a host that implements both interfaces; a host implementing only an older
draft is not compatible.

## When to Use It

Choose this provider when the runtime owns persistent key-value storage and
does not offer a useful filesystem. The provider keeps the database engine
unchanged and supplies the host bucket as a `MemoryProvider`.

Use [`wasi-dbms-memory`](./wasi-memory-provider.md) when a single file and a
WASI filesystem preopen are the simplest durable storage choice.

## Usage

Add the crate and open a bucket/database namespace:

```rust,no_run
use wasi_dbms_key_value_memory::WasiKeyValueMemoryProvider;
use wasm_dbms::DbmsContext;

let provider = WasiKeyValueMemoryProvider::new("default", "issue-176")?;
let ctx = DbmsContext::new(provider);
// Register the schema, run committed operations, then publish a checkpoint.
ctx.flush()?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

The bucket identifier is passed unchanged to `store.open`. The database name
is encoded into the namespace, so names cannot collide through `/` or other
separator characters. A native test can inject a `KeyValueStore` with
`WasiKeyValueMemoryProvider::with_store`.

## Checkpoint and Restart Model

The provider caches the complete logical memory. It records dirty pages in
inactive slots and writes them in batches of at most 16 values. It then writes
one `head` manifest containing the magic `WDBMSKV2`, the current and previous
page counts, and a slot plus XXH3 hash for every referenced page. The head is
the commit point.

Transactions and durable checkpoints are separate boundaries:

- `commit()` makes the transaction's changes part of the DBMS committed view.
- `ctx.flush()` publishes dirty committed pages to the key-value host.
- An active transaction overlay is not materialized by `flush()`.

If a page batch fails, the old head and all dirty flags remain available for a
retry. If head publication reports an error, the provider is poisoned and the
embedder must drop and reopen it. The head may already have been applied, so a
reopen can observe the old or new complete checkpoint. A reopen validates
every current page and falls back to a usable, non-empty previous checkpoint
if replication exposes a stale or missing current page, so it never loads a
mixture. If the current checkpoint cannot be validated, opening fails when the
previous checkpoint is absent, empty, or invalid. Fallback checkpoints remain
readable but reject writes for that provider instance, even if current pages
become visible later. Reopen the provider to validate the current checkpoint
and resume writes. This prevents a stale reader from replacing a newer
checkpoint.

`export_snapshot()` includes unflushed cached bytes. `import_snapshot()` only
accepts page-aligned bytes into an empty provider and still requires `flush()`.

## Limits and Host Requirements

- The cache is the full database, not a streaming page cache.
- Page count is capped at `u32::MAX` and allocations must fit the host address space.
- Values are raw 65,536-byte pages; host value-size and batch-size limits must accommodate them.
- The host must provide draft2 store and batch with read-your-writes behavior.
- Coordinate a single writer and provide durable bucket backing across restarts.
- No filesystem preopen is required by the provider.

## File Provider Alternative

For a WASI file-backed provider, see the [WASI Memory Provider](./wasi-memory-provider.md).

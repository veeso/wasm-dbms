# Atomicity

- [Atomicity](#atomicity)
  - [Overview](#overview)
  - [The Problem](#the-problem)
  - [Write-Ahead Journal](#write-ahead-journal)
    - [Architecture](#architecture)
    - [Journal Flow](#journal-flow)
    - [What is Journaled](#what-is-journaled)
  - [Transaction Commit Atomicity](#transaction-commit-atomicity)
  - [Edge Cases](#edge-cases)
    - [Page Allocation](#page-allocation)
    - [Nested Atomic Calls](#nested-atomic-calls)
    - [Rollback Failure](#rollback-failure)

---

## Overview

A DBMS must guarantee **atomicity**: either all writes in an operation succeed, or none of them persist. Without atomicity, a crash or error mid-operation can leave the database in an inconsistent state (e.g., a record written but the page ledger not updated, or half the rows in a transaction committed while the rest are lost).

---

## The Problem

The original `atomic()` implementation relied on panic semantics:

```rust
fn atomic<F, R>(&self, f: F) -> R
where
    F: FnOnce(&WasmDbmsDatabase<M, A>) -> DbmsResult<R>,
{
    match f(self) {
        Ok(res) => res,
        Err(err) => panic!("{err}"),
    }
}
```

On the Internet Computer, a panic (trap) automatically reverts all stable-memory writes made during that call. This gave IC canisters free atomicity. However, on **non-IC WASM runtimes** (e.g., Wasmtime, Wasmer, browser WASM), a panic does **not** revert memory. The host simply sees the guest abort, and any writes already flushed to linear memory remain. This made `wasm-dbms` effectively IC-only for write operations.

---

## Write-Ahead Journal

The fix is a **write-ahead journal**. Before overwriting any bytes, the journal saves the original content at that offset. On error, the journal replays saved entries in reverse order, restoring every modified byte.

### Architecture

The journal lives in the `wasm-dbms` crate's transaction module, not in the memory layer. This separation keeps the memory crate (`wasm-dbms-memory`) focused on page-level I/O while the DBMS layer owns the transaction concern.

The key types are:

- **`MemoryAccess` trait** (in `wasm-dbms-memory`): Abstracts page-level read/write operations. `MemoryManager` implements this trait with direct writes.
- **`Journal`** (in `wasm-dbms`): A heap-only collection of `JournalEntry` records. Each entry stores the page, offset, and original bytes before a write.
- **`JournaledWriter`** (in `wasm-dbms`): Wraps a `&mut MemoryManager` and a `&mut Journal`, implementing `MemoryAccess`. Every `write_at` or `zero` call reads the original bytes first, records them in the journal, then delegates to the underlying `MemoryManager`.

All memory-crate functions that perform writes (in `TableRegistry`, `PageLedger`, `FreeSegmentsLedger`, etc.) are generic over `impl MemoryAccess`. When called with a plain `MemoryManager`, writes go directly to memory. When called with a `JournaledWriter`, writes are automatically recorded for rollback.

### Journal Flow

```txt
┌─────────────────┐
│  Journal::new() │   Creates empty journal
└────────┬────────┘
         │
         ▼
┌─────────────────────────┐
│  JournaledWriter wraps  │
│  MemoryManager + Journal│
└────────┬────────────────┘
         │
         ▼
┌──────────────┐
│   write_at   │──► Reads original bytes, records in journal, then writes new data
│     zero     │──► Reads original bytes, records in journal, then writes zeros
└──────┬───────┘
       │
       ├── success ──► journal.commit()   ──► Drops entries (no-op)
       │
       └── error   ──► journal.rollback() ──► Replays entries in reverse via MemoryManager
```

Each journal entry is:

```rust
struct JournalEntry {
    page: Page,
    offset: PageOffset,
    original_bytes: Vec<u8>,
}
```

### What is Journaled

| Operation        | Journaled? | Why                                                              |
|------------------|------------|------------------------------------------------------------------|
| `write_at`       | Yes        | Modifies existing data that must be restorable                   |
| `zero`           | Yes        | Modifies existing data (writes zeros)                            |
| `allocate_page`  | No         | Newly allocated pages are unreferenced after rollback; their content is irrelevant |

---

## Transaction Commit Atomicity

When a transaction is committed, all buffered operations (inserts, updates, deletes) are flushed to memory. Previously, each operation was wrapped in its own `atomic()` call. If operation 3 of 5 failed, operations 1 and 2 were already persisted and could not be undone.

Now, `commit()` uses a **single journal** spanning all operations:

```rust
fn commit(&mut self) -> DbmsResult<()> {
    // ... take transaction ...

    *self.ctx.journal.borrow_mut() = Some(Journal::new());

    for op in transaction.operations {
        let result = match op { /* execute insert/update/delete */ };

        if let Err(err) = result {
            if let Some(journal) = self.ctx.journal.borrow_mut().take() {
                journal
                    .rollback(&mut self.ctx.mm.borrow_mut())
                    .expect("critical: failed to rollback journal");
            }
            return Err(err);
        }
    }

    if let Some(journal) = self.ctx.journal.borrow_mut().take() {
        journal.commit();
    }
    Ok(())
}
```

This ensures that either **all** transaction operations are applied, or **none** of them persist, regardless of the WASM runtime.

---

## Edge Cases

### Page Allocation

`allocate_page` writes directly via the memory provider, bypassing the journal. This is intentional: a newly allocated page has no meaningful prior content to restore, and after a rollback, nothing references it (the page ledger update that would have pointed to it was itself journaled and rolled back). The page remains allocated but unused — a minor space leak that is acceptable since it will be reused by subsequent allocations.

### Nested Atomic Calls

During `commit()`, each transaction operation is dispatched through the `Database` trait methods (`insert`, `update`, `delete`), which internally call `atomic()`. Since `commit()` has already placed a `Journal` in `DbmsContext`, `atomic()` detects this via `self.ctx.journal.borrow().is_some()` and delegates to the outer journal instead of starting its own. This ensures a single journal spans the entire commit.

### Rollback Failure

If `journal.rollback()` itself fails (e.g., the memory provider returns an I/O error during the restore writes), the program **panics**. A failed rollback means memory is in an indeterminate state — some bytes restored, some not. There is no recovery path, so immediate termination is the only safe response (per M-PANIC-ON-BUG).

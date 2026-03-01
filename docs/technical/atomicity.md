# Atomicity

- [Atomicity](#atomicity)
  - [Overview](#overview)
  - [The Problem](#the-problem)
  - [Write-Ahead Journal](#write-ahead-journal)
    - [Journal Flow](#journal-flow)
    - [What is Journaled](#what-is-journaled)
  - [Transaction Commit Atomicity](#transaction-commit-atomicity)
  - [Edge Cases](#edge-cases)
    - [Page Allocation](#page-allocation)
    - [Nested Journals](#nested-journals)
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

The fix is a **write-ahead journal** inside `MemoryManager`. Before overwriting any bytes, the journal saves the original content at that offset. On error, the journal replays saved entries in reverse order, restoring every modified byte.

### Journal Flow

```txt
┌──────────────┐
│ begin_journal│   Activates recording
└──────┬───────┘
       │
       ▼
┌──────────────┐
│   write_at   │──► Saves original bytes, then writes new data
│     zero     │──► Saves original bytes, then writes zeros
└──────┬───────┘
       │
       ├── success ──► commit_journal()  ──► Discards entries
       │
       └── error   ──► rollback_journal() ──► Replays entries in reverse
```

Each journal entry is:

```rust
struct JournalEntry {
    offset: u64,          // absolute byte offset in memory
    original_bytes: Vec<u8>, // bytes that were there before the write
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

    self.ctx.mm.borrow_mut().begin_journal();

    for op in transaction.operations {
        let result = match op { /* execute insert/update/delete */ };

        if let Err(err) = result {
            self.ctx.mm.borrow_mut()
                .rollback_journal()
                .expect("critical: failed to rollback journal");
            return Err(err);
        }
    }

    self.ctx.mm.borrow_mut().commit_journal();
    Ok(())
}
```

This ensures that either **all** transaction operations are applied, or **none** of them persist, regardless of the WASM runtime.

---

## Edge Cases

### Page Allocation

`allocate_page` writes directly via the memory provider, bypassing the journal. This is intentional: a newly allocated page has no meaningful prior content to restore, and after a rollback, nothing references it (the page ledger update that would have pointed to it was itself journaled and rolled back). The page remains allocated but unused — a minor space leak that is acceptable since it will be reused by subsequent allocations.

### Nested Atomic Calls

During `commit()`, each transaction operation is dispatched through the `Database` trait methods (`insert`, `update`, `delete`), which internally call `atomic()`. Since `commit()` has already started a journal, `atomic()` detects this via `is_journal_active()` and delegates to the outer journal instead of starting its own. This ensures a single journal spans the entire commit, and a `debug_assert!` in `begin_journal()` guards against accidental double-begin in debug builds.

Calling `begin_journal()` directly while a journal is already active panics unconditionally (per M-PANIC-ON-BUG), since discarding an active journal would make prior writes unrollbackable.

### Rollback Failure

If `rollback_journal()` itself fails (e.g., the memory provider returns an I/O error during the restore writes), the program **panics**. A failed rollback means memory is in an indeterminate state — some bytes restored, some not. There is no recovery path, so immediate termination is the only safe response (per M-PANIC-ON-BUG).

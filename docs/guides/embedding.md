# Embedding wasm-dbms

- [Embedding wasm-dbms](#embedding-wasm-dbms)
  - [Overview](#overview)
  - [Choose a Memory Provider](#choose-a-memory-provider)
  - [Own One Context](#own-one-context)
  - [Open a Session per Call](#open-a-session-per-call)
  - [Transactions Across Calls](#transactions-across-calls)
  - [Transaction Ownership](#transaction-ownership)
  - [Add SQL](#add-sql)
  - [Restarts and Upgrades](#restarts-and-upgrades)
  - [Reference Implementations](#reference-implementations)

---

## Overview

wasm-dbms is an engine, not a server. It stores tables, runs queries, and
keeps transactions, but it does not know who is calling, how calls arrive, or
when the process restarts. The application layer that embeds the engine
answers those questions. This guide shows what that layer has to do:

1. pick a `MemoryProvider` for the target runtime;
2. own one `DbmsContext` per database for the life of the process;
3. open a `WasmDbmsDatabase` session per call;
4. run transactions across calls and decide who may use them;
5. optionally expose SQL next to the typed API;
6. know what survives a restart.

The [Architecture](../technical/architecture.md#layer-4-application-layer)
page describes where this layer sits.

---

## Choose a Memory Provider

The engine reads and writes 64 KiB pages through the `MemoryProvider` trait of
`wasm-dbms-memory`. The provider decides where those pages live:

| Provider                            | Where the pages live            | Use it for                                       |
| ----------------------------------- | ------------------------------- | ------------------------------------------------ |
| `HeapMemoryProvider`                | A `Vec<u8>` on the heap         | Tests and throwaway databases                    |
| `WasiMemoryProvider`                | A file opened through WASI      | Wasmtime, Wasmer, WasmEdge, and other WASI hosts |
| Stable memory provider of `ic-dbms` | Internet Computer stable memory | Canisters                                        |

`HeapMemoryProvider` ships with `wasm-dbms-memory`. `WasiMemoryProvider` is
described in the [WASI Memory Provider](../wasi/wasi-memory-provider.md)
page. A new runtime needs a new provider; the
[Memory Management](../technical/memory.md) page explains what the engine
expects from it.

---

## Own One Context

`DbmsContext` owns everything the engine needs: the memory manager, the schema
registry, the open transactions, and the journal. Create it once, when the
process starts or on the first call, and keep it alive until the process
ends. Creating a context per call would reload the schema registry on every
call and would lose every open transaction.

The context is single-threaded on purpose: WASM runtimes run one call at a
time, so the context uses `RefCell` instead of locks and is neither `Send` nor
`Sync`. A `thread_local!` is the natural home for it:

```rust
use std::cell::RefCell;

use wasm_dbms::prelude::*;
use wasm_dbms_api::prelude::*;

#[derive(Clone, DatabaseSchema)]
#[tables(User = "users")]
pub struct MySchema;

thread_local! {
    static DBMS: RefCell<Option<DbmsContext<MyMemoryProvider>>> = const { RefCell::new(None) };
}

/// Runs `f` on the database, opening it on the first call.
fn with_dbms<F, R>(f: F) -> R
where
    F: FnOnce(&DbmsContext<MyMemoryProvider>) -> R,
{
    DBMS.with(|cell| {
        let mut slot = cell.borrow_mut();
        let ctx = slot.get_or_insert_with(|| {
            let ctx = DbmsContext::new(MyMemoryProvider::default());
            MySchema::register_tables(&ctx).expect("failed to register tables");
            ctx
        });
        f(ctx)
    })
}
```

`register_tables` writes the schema of every table that is not already
registered and is safe to call on every start.

---

## Open a Session per Call

A `WasmDbmsDatabase` is a short-lived view on the context. It is cheap to
create, so open one per call and let it go when the call ends:

- `WasmDbmsDatabase::oneshot(&ctx, MySchema)` applies every operation
  immediately and atomically;
- `WasmDbmsDatabase::from_transaction(&ctx, MySchema, tx)` stages every
  operation in the transaction `tx`.

```rust
fn insert_user(tx: Option<TransactionId>, user: UserInsertRequest) -> Result<(), DbmsError> {
    with_dbms(|ctx| {
        let database = match tx {
            Some(tx) => WasmDbmsDatabase::from_transaction(ctx, MySchema, tx),
            None => WasmDbmsDatabase::oneshot(ctx, MySchema),
        };
        database.insert::<User>(user)
    })
}
```

`from_transaction` does not check the id. An operation on an id that is not
open fails with `DbmsError::Query(QueryError::TransactionNotFound)`.

---

## Transactions Across Calls

Call-based runtimes cannot keep a Rust value alive between two calls, so the
engine keeps transactions in the context and addresses them by
`TransactionId`. An embedder exposes three kinds of entry points:

1. one that opens a transaction and returns its id;
2. operations that take an optional id and run inside it when present;
3. one that commits and one that rolls back a given id.

```rust
fn begin() -> TransactionId {
    with_dbms(|ctx| ctx.begin_transaction())
}

fn commit(tx: TransactionId) -> Result<(), DbmsError> {
    with_dbms(|ctx| WasmDbmsDatabase::from_transaction(ctx, MySchema, tx).commit())
}

fn rollback(tx: TransactionId) -> Result<(), DbmsError> {
    with_dbms(|ctx| WasmDbmsDatabase::from_transaction(ctx, MySchema, tx).rollback())
}
```

A client then calls `begin`, passes the id to as many operations as it needs,
and ends with `commit` or `rollback`. The
[Transactions](./transactions.md) guide covers what happens inside the
engine during each step. `commit` consumes the transaction whether it
succeeds or fails; `ctx.has_transaction(&tx)` reports whether an id is still
open.

---

## Transaction Ownership

The engine identifies a transaction by its id and nothing else. Whoever holds
the id can read and write through it, commit it, or roll it back. The engine
does not record who opened it, because "who" is a platform concept: a
principal on the Internet Computer, a tenant behind a host function, a
session of a CLI.

A single-tenant embedder, such as a CLI or a program that serves one user,
needs nothing more.

An embedder that exposes transactions across a trust boundary, such as a
canister serving many principals or a host serving many tenants, keeps its
own ledger from `TransactionId` to its identity type and checks it before
every call that touches a transaction:

1. **Insert on begin.** Record the identity of the caller next to the new id.
2. **Check on every transactional call.** Before `from_transaction`, `commit`,
   and `rollback`, look the id up and compare the identity. Report a mismatch
   the same way as an unknown id, so that an outsider cannot tell whether an
   id exists.
3. **Remove on commit and rollback**, whether or not the engine call
   succeeded: the engine consumes the transaction in both cases.
4. **Evict closed entries.** A transaction can be closed through a path the
   ledger did not see, for example a typed `commit` next to a SQL `COMMIT`.
   When `has_transaction` returns `false` for an id in the ledger, drop the
   entry.

```rust
use std::cell::RefCell;
use std::collections::HashMap;

/// Whoever the runtime says made the current call.
type Identity = Vec<u8>;

thread_local! {
    static OWNERS: RefCell<HashMap<TransactionId, Identity>> = RefCell::new(HashMap::new());
}

fn begin(identity: Identity) -> TransactionId {
    let tx = with_dbms(|ctx| ctx.begin_transaction());
    OWNERS.with(|owners| owners.borrow_mut().insert(tx, identity));
    tx
}

/// Checks that `identity` opened `tx` and that `tx` is still open.
fn authorize(identity: &[u8], tx: TransactionId) -> Result<(), DbmsError> {
    let open = with_dbms(|ctx| ctx.has_transaction(&tx));
    if !open {
        // closed through a path this ledger did not see
        OWNERS.with(|owners| owners.borrow_mut().remove(&tx));
        return Err(DbmsError::Query(QueryError::TransactionNotFound));
    }
    let owned = OWNERS.with(|owners| {
        owners
            .borrow()
            .get(&tx)
            .is_some_and(|owner| owner.as_slice() == identity)
    });
    if owned {
        Ok(())
    } else {
        Err(DbmsError::Query(QueryError::TransactionNotFound))
    }
}

fn insert_user(
    identity: &[u8],
    tx: Option<TransactionId>,
    user: UserInsertRequest,
) -> Result<(), DbmsError> {
    if let Some(tx) = tx {
        authorize(identity, tx)?;
    }
    with_dbms(|ctx| {
        let database = match tx {
            Some(tx) => WasmDbmsDatabase::from_transaction(ctx, MySchema, tx),
            None => WasmDbmsDatabase::oneshot(ctx, MySchema),
        };
        database.insert::<User>(user)
    })
}

fn commit(identity: &[u8], tx: TransactionId) -> Result<(), DbmsError> {
    authorize(identity, tx)?;
    let result = with_dbms(|ctx| WasmDbmsDatabase::from_transaction(ctx, MySchema, tx).commit());
    // the engine consumed the transaction whether or not the commit succeeded
    OWNERS.with(|owners| owners.borrow_mut().remove(&tx));
    result
}
```

`rollback` follows the same shape as `commit`. The ledger lives on the heap
next to the context; see [Restarts and Upgrades](#restarts-and-upgrades) for
why it must not outlive it.

---

## Add SQL

`SqlEngine` from `wasm-dbms-sql` runs SQL text against the same context and
uses the same transaction ids. It holds only the schema, so it can live next
to the context or be created per call:

```rust
use wasm_dbms_sql::SqlEngine;

fn run_sql(
    identity: &[u8],
    tx: Option<TransactionId>,
    sql: &str,
    params: &[Value],
) -> Result<SqlResult, SqlError> {
    if let Some(tx) = tx {
        authorize(identity, tx)?;
    }
    let result = with_dbms(|ctx| SqlEngine::new(MySchema).execute(ctx, tx, sql, params));
    match &result {
        Ok(SqlResult::TxBegin(tx)) => {
            OWNERS.with(|owners| owners.borrow_mut().insert(*tx, identity.to_vec()));
        }
        Ok(SqlResult::TxCommit | SqlResult::TxRollback) => {
            if let Some(tx) = tx {
                OWNERS.with(|owners| owners.borrow_mut().remove(&tx));
            }
        }
        _ => {}
    }
    result
}
```

A transaction opened with `BEGIN` can be committed through the typed API, and
one opened with `ctx.begin_transaction()` can run SQL. The
[SQL](./sql.md#transactions) guide describes the statement semantics.

---

## Restarts and Upgrades

Two kinds of state exist:

| State                                    | Where it lives  | After a restart or upgrade |
| ---------------------------------------- | --------------- | -------------------------- |
| Committed rows, indexes, schema registry | Memory provider | Kept                       |
| Open transactions                        | Heap            | Gone                       |
| Ownership ledger of the embedder         | Heap            | Gone                       |

Committed data is written through the memory provider as soon as `commit`
returns, so it survives whatever the provider survives. A transaction
that is open when the process stops is lost together with its writes, and so
is every ledger entry that points at it. Nothing needs to be saved before a
restart or restored after one: the ledger and the open transactions disappear
together and stay consistent.

Transaction ids restart from `0` with every new context. A ledger that
survived while the context did not would therefore authorize stale ids for
transactions it never saw. Keep the ledger in the same heap as the context,
and never persist it.

---

## Reference Implementations

- [ic-dbms](https://github.com/veeso/ic-dbms) is the application layer for the
  Internet Computer: a stable-memory provider, a canister that owns the
  context, Candid entry points, and a ledger from transaction id to
  principal.
- The [Wasmtime Example](./wasmtime-example.md) in
  `crates/wasm-dbms/example/` is a single-tenant WASI embedder: a file-backed
  provider, a `thread_local!` context, and a WIT interface with
  `begin-transaction`, `commit`, and `rollback` entry points.

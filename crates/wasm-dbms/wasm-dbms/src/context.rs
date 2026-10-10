// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! DBMS context that owns all database state.
//!
//! `DbmsContext` provides a runtime-agnostic container for the full DBMS
//! state. Internal `RefCell` wrappers allow shared-reference mutation
//! through a single shared reference.

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::{Rc, Weak};

use wasm_dbms_api::prelude::{DbmsResult, TransactionId};
use wasm_dbms_memory::prelude::{MemoryManager, MemoryProvider, SchemaRegistry, TableRegistryPage};

mod access_stats;

pub(crate) use self::access_stats::AccessKind;
#[cfg(any(test, feature = "access-stats"))]
pub use self::access_stats::AccessStats;
use crate::transaction::journal::Journal;
use crate::transaction::session::TransactionSession;

/// Opaque identity of a [`DbmsContext`].
///
/// The identity remains stable when the context is moved. It does not keep the
/// context alive, so context-associated caches can discard stale entries.
#[derive(Clone, Debug)]
pub struct ContextId(Weak<()>);

impl ContextId {
    /// Returns whether the context that owns this identity still exists.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

impl PartialEq for ContextId {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for ContextId {}

impl Hash for ContextId {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.0.as_ptr().hash(state);
    }
}

/// Owns all mutable DBMS state behind interior-mutable wrappers.
///
/// Each component is wrapped in a `RefCell` so that operations
/// borrowing different components can coexist without requiring
/// `&mut self` on the context.
///
/// # Threading
///
/// `DbmsContext` is `!Send` and `!Sync` because of the `RefCell`
/// wrappers. This is intentional: WASM runtimes (both IC canisters
/// and WASI preview 1 modules) execute single-threaded, so interior
/// mutability via `RefCell` is sufficient and avoids the overhead of
/// synchronization primitives. Embedders that need multi-threaded
/// access should wrap the context in their own synchronization layer.
pub struct DbmsContext<M>
where
    M: MemoryProvider,
{
    /// Stable identity used by state that is associated with this context.
    identity: Rc<()>,

    /// Memory manager for page-level operations.
    pub(crate) mm: RefCell<MemoryManager<M>>,

    /// Schema registry mapping table names to page locations.
    pub(crate) schema_registry: RefCell<SchemaRegistry>,

    /// Active transaction sessions.
    pub(crate) transaction_session: RefCell<TransactionSession>,

    /// Active write-ahead journal for atomic operations.
    pub(crate) journal: RefCell<Option<Journal>>,

    /// Cached `(compiled_schema_hash, drifted)` pair for the most recent
    /// schema attached to a database session on this context.
    pub(crate) drift: Cell<Option<(u64, bool)>>,

    /// Set while a migration apply pass is mutating stable memory so the
    /// per-CRUD drift gate does not block the engine's own internal reads
    /// (e.g. tightening validation that scans existing rows).
    pub(crate) migrating: Cell<bool>,

    /// Storage access counters, see [`AccessStats`].
    #[cfg(any(test, feature = "access-stats"))]
    access_stats: Cell<AccessStats>,
}

impl<M> DbmsContext<M>
where
    M: MemoryProvider,
{
    /// Creates a new DBMS context, initializing the memory manager and
    /// loading persisted state.
    pub fn new(memory: M) -> Self {
        let mut mm = MemoryManager::init(memory);
        let schema_registry = SchemaRegistry::load(&mut mm).unwrap_or_default();
        Self {
            identity: Rc::new(()),
            mm: RefCell::new(mm),
            schema_registry: RefCell::new(schema_registry),
            transaction_session: RefCell::new(TransactionSession::default()),
            journal: RefCell::new(None),
            drift: Cell::new(None),
            migrating: Cell::new(false),
            #[cfg(any(test, feature = "access-stats"))]
            access_stats: Cell::new(AccessStats::default()),
        }
    }

    /// Publishes changes made through this context to the memory provider.
    ///
    /// Providers without durable storage implement this as a no-op. Active
    /// transaction overlays remain uncommitted and are not flushed.
    pub fn flush(&self) -> DbmsResult<()> {
        self.mm.borrow_mut().flush().map_err(Into::into)
    }

    /// Returns the stable identity of this context.
    pub fn id(&self) -> ContextId {
        ContextId(Rc::downgrade(&self.identity))
    }

    /// Registers a table schema, persisting it in stable memory.
    pub fn register_table<T: wasm_dbms_api::prelude::TableSchema>(
        &self,
    ) -> DbmsResult<TableRegistryPage> {
        let mut sr = self.schema_registry.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        sr.register_table::<T>(&mut *mm).map_err(Into::into)
    }

    /// Returns whether `name` resolves to a registered table.
    pub fn has_table(&self, name: &str) -> bool {
        self.schema_registry
            .borrow()
            .table_registry_page_by_name(name)
            .is_some()
    }

    /// Begins a new transaction and returns its ID.
    ///
    /// The engine does not record who opened the transaction: any code that
    /// holds the ID can use, commit, or roll it back. An embedder that serves
    /// several identities keeps its own ledger from ID to identity and checks
    /// it before every transactional call.
    pub fn begin_transaction(&self) -> TransactionId {
        self.transaction_session.borrow_mut().begin_transaction()
    }

    /// Returns whether `tx_id` names a transaction that is still open.
    ///
    /// Embedders use it to drop ledger entries whose transaction was closed
    /// through a path they did not see.
    pub fn has_transaction(&self, tx_id: &TransactionId) -> bool {
        self.transaction_session.borrow().has_transaction(tx_id)
    }

    /// Returns the cached drift flag for `compiled_hash`, if present.
    pub(crate) fn cached_drift_for(&self, compiled_hash: u64) -> Option<bool> {
        self.drift
            .get()
            .and_then(|(hash, drifted)| (hash == compiled_hash).then_some(drifted))
    }

    /// Caches the drift flag for the given compiled schema hash.
    pub(crate) fn set_drift(&self, compiled_hash: u64, value: bool) {
        self.drift.set(Some((compiled_hash, value)));
    }

    /// Clears the cached drift flag, forcing the next call to recompute.
    pub(crate) fn clear_drift(&self) {
        self.drift.set(None);
    }

    /// Returns `true` while a migration apply pass is mutating stable memory.
    pub(crate) fn is_migrating(&self) -> bool {
        self.migrating.get()
    }

    /// Sets the migration-in-progress guard.
    pub(crate) fn set_migrating(&self, value: bool) {
        self.migrating.set(value);
    }

    /// Records `count` storage accesses of `kind`.
    ///
    /// Compiles to nothing unless the `access-stats` feature is enabled.
    #[inline]
    pub(crate) fn count_access(&self, kind: AccessKind, count: u64) {
        #[cfg(any(test, feature = "access-stats"))]
        {
            let mut stats = self.access_stats.get();
            stats.add(kind, count);
            self.access_stats.set(stats);
        }
        #[cfg(not(any(test, feature = "access-stats")))]
        let _ = (kind, count);
    }

    /// Returns the storage accesses counted since the last reset.
    #[cfg(any(test, feature = "access-stats"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "access-stats")))]
    pub fn access_stats(&self) -> AccessStats {
        self.access_stats.get()
    }

    /// Resets every storage access counter to zero.
    #[cfg(any(test, feature = "access-stats"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "access-stats")))]
    pub fn reset_access_stats(&self) {
        self.access_stats.set(AccessStats::default());
    }
}

impl<M> std::fmt::Debug for DbmsContext<M>
where
    M: MemoryProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbmsContext")
            .field("schema_registry", &self.schema_registry)
            .field("transaction_session", &self.transaction_session)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use wasm_dbms_api::prelude::{MemoryError, MemoryResult};
    use wasm_dbms_memory::MemoryProvider;
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;

    struct FlushProbe {
        provider: HeapMemoryProvider,
        calls: Rc<Cell<usize>>,
        fail: Rc<Cell<bool>>,
    }

    impl FlushProbe {
        fn new(calls: Rc<Cell<usize>>, fail: Rc<Cell<bool>>) -> Self {
            Self {
                provider: HeapMemoryProvider::default(),
                calls,
                fail,
            }
        }
    }

    impl MemoryProvider for FlushProbe {
        const PAGE_SIZE: u64 = HeapMemoryProvider::PAGE_SIZE;

        fn size(&self) -> u64 {
            self.provider.size()
        }

        fn pages(&self) -> u64 {
            self.provider.pages()
        }

        fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
            self.provider.grow(new_pages)
        }

        fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
            self.provider.read(offset, buf)
        }

        fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
            self.provider.write(offset, buf)
        }

        fn flush(&mut self) -> MemoryResult<()> {
            self.calls.set(self.calls.get() + 1);
            if self.fail.get() {
                return Err(MemoryError::ProviderError("flush failed".to_owned()));
            }
            Ok(())
        }
    }

    #[test]
    fn test_should_create_context() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        assert!(!ctx.has_table("users"));
    }

    #[test]
    fn test_should_begin_transaction() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        let tx_id = ctx.begin_transaction();
        assert!(ctx.has_transaction(&tx_id));
        assert!(!ctx.has_transaction(&(tx_id + 1)));
    }

    #[test]
    fn test_transaction_ids_restart_with_the_context() {
        let first = DbmsContext::new(HeapMemoryProvider::default());
        let second = DbmsContext::new(HeapMemoryProvider::default());
        assert_eq!(first.begin_transaction(), second.begin_transaction());
    }

    #[test]
    fn test_context_identity_is_stable_and_unique() {
        let first = DbmsContext::new(HeapMemoryProvider::default());
        let first_id = first.id();
        let moved = first;
        let second = DbmsContext::new(HeapMemoryProvider::default());

        assert_eq!(first_id, moved.id());
        assert_ne!(first_id, second.id());
    }

    #[test]
    fn test_should_debug_context() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        let debug = format!("{ctx:?}");
        assert!(debug.contains("DbmsContext"));
    }

    #[test]
    fn context_flush_reaches_provider_and_preserves_error() {
        let calls = Rc::new(Cell::new(0));
        let fail = Rc::new(Cell::new(false));
        let ctx = DbmsContext::new(FlushProbe::new(calls.clone(), fail.clone()));
        assert_eq!(calls.get(), 0);
        ctx.flush().unwrap();
        assert_eq!(calls.get(), 1);
        fail.set(true);
        assert!(
            ctx.flush()
                .unwrap_err()
                .to_string()
                .contains("flush failed")
        );
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn default_flush_keeps_heap_context_and_transaction_usable() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        let tx = ctx.begin_transaction();
        ctx.flush().unwrap();
        assert!(ctx.has_transaction(&tx));
    }
}

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
        }
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

    /// Begins a new transaction for the given owner identity.
    pub fn begin_transaction(&self, owner: Vec<u8>) -> TransactionId {
        let mut ts = self.transaction_session.borrow_mut();
        ts.begin_transaction(owner)
    }

    /// Returns whether the given transaction is owned by the given identity.
    pub fn has_transaction(&self, tx_id: &TransactionId, caller: &[u8]) -> bool {
        let ts = self.transaction_session.borrow();
        ts.has_transaction(tx_id, caller)
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
    use wasm_dbms_memory::prelude::HeapMemoryProvider;

    use super::*;

    #[test]
    fn test_should_create_context() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        assert!(!ctx.has_table("users"));
    }

    #[test]
    fn test_should_begin_transaction() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        let owner = vec![1, 2, 3];
        let tx_id = ctx.begin_transaction(owner.clone());
        assert!(ctx.has_transaction(&tx_id, &owner));
        assert!(!ctx.has_transaction(&tx_id, &[4, 5, 6]));
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
}

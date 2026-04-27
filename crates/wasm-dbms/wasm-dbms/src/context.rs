// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! DBMS context that owns all database state.
//!
//! `DbmsContext` provides a runtime-agnostic container for the full DBMS
//! state. Internal `RefCell` wrappers allow shared-reference mutation
//! through a single shared reference.

use std::cell::{Cell, RefCell};

use wasm_dbms_api::prelude::{DbmsResult, TransactionId};
use wasm_dbms_memory::prelude::{
    AccessControl, AccessControlList, MemoryManager, MemoryProvider, SchemaRegistry,
    TableRegistryPage,
};

use crate::transaction::journal::Journal;
use crate::transaction::session::TransactionSession;

/// Owns all mutable DBMS state behind interior-mutable wrappers.
///
/// Each component is wrapped in a `RefCell` so that operations
/// borrowing different components can coexist without requiring
/// `&mut self` on the context.
///
/// The access-control provider `A` defaults to [`AccessControlList`].
/// Runtimes that do not need ACL can use [`NoAccessControl`](wasm_dbms_memory::NoAccessControl).
///
/// # Threading
///
/// `DbmsContext` is `!Send` and `!Sync` because of the `RefCell`
/// wrappers. This is intentional: WASM runtimes (both IC canisters
/// and WASI preview 1 modules) execute single-threaded, so interior
/// mutability via `RefCell` is sufficient and avoids the overhead of
/// synchronization primitives. Embedders that need multi-threaded
/// access should wrap the context in their own synchronization layer.
pub struct DbmsContext<M, A = AccessControlList>
where
    M: MemoryProvider,
    A: AccessControl,
{
    /// Memory manager for page-level operations.
    pub(crate) mm: RefCell<MemoryManager<M>>,

    /// Schema registry mapping table names to page locations.
    pub(crate) schema_registry: RefCell<SchemaRegistry>,

    /// Access-control provider storing allowed identities.
    pub(crate) acl: RefCell<A>,

    /// Active transaction sessions.
    pub(crate) transaction_session: RefCell<TransactionSession>,

    /// Active write-ahead journal for atomic operations.
    pub(crate) journal: RefCell<Option<Journal>>,

    /// Lazily computed drift flag: `Some(true)` when the compiled schema
    /// diverges from the snapshots persisted in stable memory, `Some(false)`
    /// when they match, `None` until the first check on this context.
    pub(crate) drift: Cell<Option<bool>>,

    /// Set while a migration apply pass is mutating stable memory so the
    /// per-CRUD drift gate does not block the engine's own internal reads
    /// (e.g. tightening validation that scans existing rows).
    pub(crate) migrating: Cell<bool>,
}

impl<M> DbmsContext<M>
where
    M: MemoryProvider,
{
    /// Creates a new DBMS context with the default [`AccessControlList`],
    /// initializing the memory manager and loading persisted state.
    pub fn new(memory: M) -> Self {
        let mut mm = MemoryManager::init(memory);
        let schema_registry = SchemaRegistry::load(&mut mm).unwrap_or_default();
        let acl = AccessControlList::load(&mut mm).unwrap_or_default();

        Self {
            mm: RefCell::new(mm),
            schema_registry: RefCell::new(schema_registry),
            acl: RefCell::new(acl),
            transaction_session: RefCell::new(TransactionSession::default()),
            journal: RefCell::new(None),
            drift: Cell::new(None),
            migrating: Cell::new(false),
        }
    }
}

impl<M, A> DbmsContext<M, A>
where
    M: MemoryProvider,
    A: AccessControl,
{
    /// Creates a new DBMS context with a custom access control provider.
    pub fn with_acl(memory: M) -> Self {
        let mut mm = MemoryManager::init(memory);
        let schema_registry = SchemaRegistry::load(&mut mm).unwrap_or_default();
        let acl = A::load(&mut mm).unwrap_or_default();

        Self {
            mm: RefCell::new(mm),
            schema_registry: RefCell::new(schema_registry),
            acl: RefCell::new(acl),
            transaction_session: RefCell::new(TransactionSession::default()),
            journal: RefCell::new(None),
            drift: Cell::new(None),
            migrating: Cell::new(false),
        }
    }

    /// Registers a table schema, persisting it in stable memory.
    pub fn register_table<T: wasm_dbms_api::prelude::TableSchema>(
        &self,
    ) -> DbmsResult<TableRegistryPage> {
        let mut sr = self.schema_registry.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        sr.register_table::<T>(&mut mm).map_err(Into::into)
    }

    /// Adds an identity to the access-control list.
    pub fn acl_add(&self, identity: A::Id) -> DbmsResult<()> {
        let mut acl = self.acl.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        acl.add_identity(identity, &mut mm).map_err(Into::into)
    }

    /// Removes an identity from the access-control list.
    pub fn acl_remove(&self, identity: &A::Id) -> DbmsResult<()> {
        let mut acl = self.acl.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        acl.remove_identity(identity, &mut mm).map_err(Into::into)
    }

    /// Returns all identities currently in the access-control list.
    pub fn acl_allowed(&self) -> Vec<A::Id> {
        let acl = self.acl.borrow();
        acl.allowed_identities()
    }

    /// Returns whether the given identity is allowed by the ACL.
    pub fn acl_is_allowed(&self, identity: &A::Id) -> bool {
        let acl = self.acl.borrow();
        acl.is_allowed(identity)
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

    /// Returns the cached drift flag, or `None` if it has not been computed yet.
    pub(crate) fn cached_drift(&self) -> Option<bool> {
        self.drift.get()
    }

    /// Caches the drift flag for the lifetime of the context (until cleared).
    pub(crate) fn set_drift(&self, value: bool) {
        self.drift.set(Some(value));
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

impl<M, A> std::fmt::Debug for DbmsContext<M, A>
where
    M: MemoryProvider,
    A: AccessControl + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbmsContext")
            .field("schema_registry", &self.schema_registry)
            .field("acl", &self.acl)
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
        assert!(ctx.acl_allowed().is_empty());
    }

    #[test]
    fn test_should_add_acl_identity() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        ctx.acl_add(vec![1, 2, 3]).unwrap();
        assert!(ctx.acl_is_allowed(&vec![1, 2, 3]));
        assert!(!ctx.acl_is_allowed(&vec![4, 5, 6]));
    }

    #[test]
    fn test_should_remove_acl_identity() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        ctx.acl_add(vec![1, 2, 3]).unwrap();
        ctx.acl_add(vec![4, 5, 6]).unwrap();
        ctx.acl_remove(&vec![1, 2, 3]).unwrap();
        assert!(!ctx.acl_is_allowed(&vec![1, 2, 3]));
        assert!(ctx.acl_is_allowed(&vec![4, 5, 6]));
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
    fn test_should_debug_context() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        let debug = format!("{ctx:?}");
        assert!(debug.contains("DbmsContext"));
    }
}

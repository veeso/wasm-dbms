// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! DBMS context that owns all database state.
//!
//! `DbmsContext` provides a runtime-agnostic container for the full DBMS
//! state. Internal `RefCell` wrappers allow shared-reference mutation
//! through a single shared reference.

use std::cell::{Cell, RefCell};

use wasm_dbms_api::prelude::{
    DbmsResult, IdentityPerms, PermGrant, PermRevoke, TableFingerprint, TablePerms, TransactionId,
};
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
        sr.register_table::<T>(&mut *mm).map_err(Into::into)
    }

    /// Returns whether `name` resolves to a registered table.
    pub fn has_table(&self, name: &str) -> bool {
        self.schema_registry
            .borrow()
            .table_registry_page_by_name(name)
            .is_some()
    }

    /// Returns whether `id` is granted `required` on `table`.
    pub fn granted(&self, id: &A::Id, table: TableFingerprint, required: TablePerms) -> bool {
        self.acl.borrow().granted(id, table, required)
    }

    /// Returns whether `id` carries the `admin` bypass flag.
    pub fn granted_admin(&self, id: &A::Id) -> bool {
        self.acl.borrow().granted_admin(id)
    }

    /// Returns whether `id` carries the `manage_acl` flag.
    pub fn granted_manage_acl(&self, id: &A::Id) -> bool {
        self.acl.borrow().granted_manage_acl(id)
    }

    /// Returns whether `id` carries the `migrate` flag.
    pub fn granted_migrate(&self, id: &A::Id) -> bool {
        self.acl.borrow().granted_migrate(id)
    }

    /// Applies a grant. **Does not** enforce `manage_acl` on the caller —
    /// callers must check `granted_manage_acl` first or use the
    /// `Dbms::grant` wrapper which self-enforces.
    pub fn acl_grant(&self, id: A::Id, grant: PermGrant) -> DbmsResult<()> {
        let mut acl = self.acl.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        acl.grant(id, grant, &mut mm).map_err(Into::into)
    }

    /// Applies a revoke. Does not enforce `manage_acl` on the caller.
    pub fn acl_revoke(&self, id: &A::Id, revoke: PermRevoke) -> DbmsResult<()> {
        let mut acl = self.acl.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        acl.revoke(id, revoke, &mut mm).map_err(Into::into)
    }

    /// Removes an identity entirely. Does not enforce `manage_acl` on the
    /// caller.
    pub fn acl_remove_identity(&self, id: &A::Id) -> DbmsResult<()> {
        let mut acl = self.acl.borrow_mut();
        let mut mm = self.mm.borrow_mut();
        acl.remove_identity(id, &mut mm).map_err(Into::into)
    }

    /// Returns the [`IdentityPerms`] currently held by `id`.
    pub fn acl_perms(&self, id: &A::Id) -> IdentityPerms {
        self.acl.borrow().perms(id)
    }

    /// Returns every identity in the ACL together with its perms.
    pub fn acl_identities(&self) -> Vec<(A::Id, IdentityPerms)> {
        self.acl.borrow().identities()
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
        assert!(ctx.acl_identities().is_empty());
    }

    #[test]
    fn test_should_grant_admin_to_identity() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        ctx.acl_grant(vec![1, 2, 3], PermGrant::Admin).unwrap();
        assert!(ctx.granted_admin(&vec![1, 2, 3]));
        assert!(!ctx.granted_admin(&vec![4, 5, 6]));
    }

    #[test]
    fn test_should_remove_identity() {
        let ctx = DbmsContext::new(HeapMemoryProvider::default());
        ctx.acl_grant(vec![1, 2, 3], PermGrant::ManageAcl).unwrap();
        ctx.acl_grant(vec![4, 5, 6], PermGrant::Admin).unwrap();
        ctx.acl_remove_identity(&vec![4, 5, 6]).unwrap();
        assert!(!ctx.granted_admin(&vec![4, 5, 6]));
        assert!(ctx.granted_manage_acl(&vec![1, 2, 3]));
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

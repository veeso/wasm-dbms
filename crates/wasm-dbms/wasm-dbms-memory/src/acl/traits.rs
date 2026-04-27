// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Granular access-control trait.

use wasm_dbms_api::prelude::{
    IdentityPerms, MemoryResult, PermGrant, PermRevoke, TableFingerprint, TablePerms,
};

use crate::{MemoryManager, MemoryProvider};

/// Trait for granular access-control providers.
///
/// Implementations gate every CRUD-relevant operation through `granted*`
/// predicates. Mutations persist via `mm`.
///
/// The `Id` associated type lets runtimes use native identity
/// representations (`Vec<u8>` for the generic layer, `Principal` for the
/// IC adapter, `()` for the no-op provider).
pub trait AccessControl: Default {
    /// Native identity type used by this provider.
    type Id;

    /// Loads ACL state from persisted memory.
    fn load<M>(mm: &mut MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
        Self: Sized;

    /// Returns whether `id` is granted `perm` on `table`.
    fn granted(&self, id: &Self::Id, table: TableFingerprint, perm: TablePerms) -> bool;

    /// Returns whether `id` carries the `admin` bypass flag.
    fn granted_admin(&self, id: &Self::Id) -> bool;

    /// Returns whether `id` carries the `manage_acl` flag.
    fn granted_manage_acl(&self, id: &Self::Id) -> bool;

    /// Returns whether `id` carries the `migrate` flag.
    fn granted_migrate(&self, id: &Self::Id) -> bool;

    /// Applies a grant to `id`, creating the entry if missing.
    fn grant<M>(
        &mut self,
        id: Self::Id,
        grant: PermGrant,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider;

    /// Applies a revoke to `id`. No-op if `id` is not present.
    fn revoke<M>(
        &mut self,
        id: &Self::Id,
        revoke: PermRevoke,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider;

    /// Removes `id` entirely from the ACL.
    fn remove_identity<M>(&mut self, id: &Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider;

    /// Returns the [`IdentityPerms`] currently held by `id`, or the
    /// default (no perms) if `id` is unknown.
    fn perms(&self, id: &Self::Id) -> IdentityPerms;

    /// Returns every identity in the ACL together with its perms.
    fn identities(&self) -> Vec<(Self::Id, IdentityPerms)>;
}

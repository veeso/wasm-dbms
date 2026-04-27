// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Memory module provides stable memory management for the IC DBMS Canister.
//!
//! This module re-exports types from `wasm-dbms-memory` and provides
//! a thread-local [`DbmsContext`] that consolidates the memory manager,
//! schema registry, ACL, and transaction session.

mod provider;

use candid::Principal;
use ic_dbms_api::prelude::{
    IdentityPerms, MemoryResult, PermGrant, PermRevoke, TableFingerprint, TablePerms,
};
use wasm_dbms_memory::prelude::{AccessControl, AccessControlList, MemoryManager, MemoryProvider};

#[cfg(target_family = "wasm")]
pub type IcMemoryProvider = provider::IcMemoryProvider;

#[cfg(not(target_family = "wasm"))]
pub type IcMemoryProvider = wasm_dbms_memory::HeapMemoryProvider;

/// Access control provider for the Internet Computer.
///
/// Wraps [`AccessControlList`] and presents identities as
/// [`Principal`] instead of raw bytes. Conversion between
/// `Principal` and `Vec<u8>` happens internally.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IcAccessControlList(AccessControlList);

impl AccessControl for IcAccessControlList {
    type Id = Principal;

    fn load<M>(mm: &mut MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
    {
        AccessControlList::load(mm).map(Self)
    }

    fn granted(&self, id: &Self::Id, table: TableFingerprint, perm: TablePerms) -> bool {
        self.0.granted(&id.as_slice().to_vec(), table, perm)
    }

    fn granted_admin(&self, id: &Self::Id) -> bool {
        self.0.granted_admin(&id.as_slice().to_vec())
    }

    fn granted_manage_acl(&self, id: &Self::Id) -> bool {
        self.0.granted_manage_acl(&id.as_slice().to_vec())
    }

    fn granted_migrate(&self, id: &Self::Id) -> bool {
        self.0.granted_migrate(&id.as_slice().to_vec())
    }

    fn grant<M>(
        &mut self,
        id: Self::Id,
        grant: PermGrant,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        self.0.grant(id.as_slice().to_vec(), grant, mm)
    }

    fn revoke<M>(
        &mut self,
        id: &Self::Id,
        revoke: PermRevoke,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        self.0.revoke(&id.as_slice().to_vec(), revoke, mm)
    }

    fn remove_identity<M>(&mut self, id: &Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        self.0.remove_identity(&id.as_slice().to_vec(), mm)
    }

    fn perms(&self, id: &Self::Id) -> IdentityPerms {
        self.0.perms(&id.as_slice().to_vec())
    }

    fn identities(&self) -> Vec<(Self::Id, IdentityPerms)> {
        self.0
            .identities()
            .into_iter()
            .filter_map(|(bytes, perms)| Principal::try_from_slice(&bytes).ok().map(|p| (p, perms)))
            .collect()
    }
}

thread_local! {
    pub static DBMS_CONTEXT: wasm_dbms::prelude::DbmsContext<IcMemoryProvider, IcAccessControlList> =
        wasm_dbms::prelude::DbmsContext::with_acl(IcMemoryProvider::default());
}

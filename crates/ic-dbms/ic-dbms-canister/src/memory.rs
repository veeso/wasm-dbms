// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Memory module provides stable memory management for the IC DBMS Canister.
//!
//! This module re-exports types from `wasm-dbms-memory` and provides
//! a thread-local [`DbmsContext`] that consolidates the memory manager,
//! schema registry, ACL, and transaction session.

mod provider;

use candid::Principal;
use ic_dbms_api::prelude::MemoryResult;
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

    fn is_allowed(&self, identity: &Self::Id) -> bool {
        let bytes = identity.as_slice().to_vec();
        self.0.is_allowed(&bytes)
    }

    fn allowed_identities(&self) -> Vec<Self::Id> {
        self.0
            .allowed_identities()
            .into_iter()
            .filter_map(|bytes| Principal::try_from_slice(&bytes).ok())
            .collect()
    }

    fn add_identity<M>(&mut self, identity: Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        self.0.add_identity(identity.as_slice().to_vec(), mm)
    }

    fn remove_identity<M>(
        &mut self,
        identity: &Self::Id,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        self.0.remove_identity(&identity.as_slice().to_vec(), mm)
    }
}

thread_local! {
    pub static DBMS_CONTEXT: wasm_dbms::prelude::DbmsContext<IcMemoryProvider, IcAccessControlList> =
        wasm_dbms::prelude::DbmsContext::with_acl(IcMemoryProvider::default());
}

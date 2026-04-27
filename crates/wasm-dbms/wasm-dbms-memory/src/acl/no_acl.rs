// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! ACL provider that grants every operation unconditionally.

use wasm_dbms_api::prelude::{
    IdentityPerms, MemoryResult, PermGrant, PermRevoke, TableFingerprint, TablePerms,
};

use super::traits::AccessControl;
use crate::{MemoryManager, MemoryProvider};

/// ACL provider that grants every operation unconditionally.
///
/// Use this for runtimes that handle authorization externally
/// or do not need access control.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct NoAccessControl;

impl AccessControl for NoAccessControl {
    type Id = ();

    fn load<M>(_mm: &mut MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
    {
        Ok(Self)
    }

    fn granted(&self, _: &Self::Id, _: TableFingerprint, _: TablePerms) -> bool {
        true
    }

    fn granted_admin(&self, _: &Self::Id) -> bool {
        true
    }

    fn granted_manage_acl(&self, _: &Self::Id) -> bool {
        true
    }

    fn granted_migrate(&self, _: &Self::Id) -> bool {
        true
    }

    fn grant<M>(&mut self, _: Self::Id, _: PermGrant, _: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        Ok(())
    }

    fn revoke<M>(
        &mut self,
        _: &Self::Id,
        _: PermRevoke,
        _: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        Ok(())
    }

    fn remove_identity<M>(&mut self, _: &Self::Id, _: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        Ok(())
    }

    fn perms(&self, _: &Self::Id) -> IdentityPerms {
        IdentityPerms::fully_permissive()
    }

    fn identities(&self) -> Vec<(Self::Id, IdentityPerms)> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::fingerprint_for_name;

    use super::*;
    use crate::HeapMemoryProvider;

    fn fp(name: &str) -> TableFingerprint {
        fingerprint_for_name(name)
    }

    #[test]
    fn test_grants_everything() {
        let acl = NoAccessControl;
        assert!(acl.granted(&(), fp("users"), TablePerms::all()));
        assert!(acl.granted_admin(&()));
        assert!(acl.granted_manage_acl(&()));
        assert!(acl.granted_migrate(&()));
    }

    #[test]
    fn test_mutations_are_noops() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let mut acl = NoAccessControl;
        acl.grant((), PermGrant::Admin, &mut mm).unwrap();
        acl.revoke(&(), PermRevoke::Admin, &mut mm).unwrap();
        acl.remove_identity(&(), &mut mm).unwrap();
        assert!(acl.identities().is_empty());
    }

    #[test]
    fn test_perms_are_fully_permissive() {
        let acl = NoAccessControl;
        assert_eq!(acl.perms(&()), IdentityPerms::fully_permissive());
    }
}

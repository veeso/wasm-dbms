// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

//! Granular access-control list backed by the ACL page.
//!
//! Identities are stored as raw bytes (`Vec<u8>`) so that the memory
//! layer remains runtime-agnostic. Runtimes that prefer a native
//! identity type wrap this list (see `IcAccessControlList`).

use std::collections::HashMap;

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, IdentityPerms, MSize, MemoryError, MemoryResult,
    PageOffset, PermGrant, PermRevoke, TableFingerprint, TablePerms,
};

use super::traits::AccessControl;
use crate::{MemoryAccess, MemoryManager, MemoryProvider};

const LAYOUT_VERSION: u8 = 2;
const FLAG_ADMIN: u8 = 0b0000_0001;
const FLAG_MANAGE_ACL: u8 = 0b0000_0010;
const FLAG_MIGRATE: u8 = 0b0000_0100;

/// Granular access-control list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessControlList {
    entries: HashMap<Vec<u8>, IdentityPerms>,
}

impl AccessControlList {
    fn save<M>(&self, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        mm.write_at(mm.acl_page(), 0, self)
    }

    fn manage_acl_count(&self) -> usize {
        self.entries.values().filter(|p| p.manage_acl).count()
    }

    fn flags_byte(p: &IdentityPerms) -> u8 {
        let mut b = 0u8;
        if p.admin {
            b |= FLAG_ADMIN;
        }
        if p.manage_acl {
            b |= FLAG_MANAGE_ACL;
        }
        if p.migrate {
            b |= FLAG_MIGRATE;
        }
        b
    }

    fn perms_from_flags(flags: u8) -> (bool, bool, bool) {
        (
            flags & FLAG_ADMIN != 0,
            flags & FLAG_MANAGE_ACL != 0,
            flags & FLAG_MIGRATE != 0,
        )
    }
}

impl AccessControl for AccessControlList {
    type Id = Vec<u8>;

    fn load<M>(mm: &mut MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
    {
        mm.read_at(mm.acl_page(), 0)
    }

    fn granted(&self, id: &Self::Id, table: TableFingerprint, perm: TablePerms) -> bool {
        match self.entries.get(id) {
            Some(p) => p.grants_table(table, perm),
            None => false,
        }
    }

    fn granted_admin(&self, id: &Self::Id) -> bool {
        self.entries.get(id).is_some_and(|p| p.admin)
    }

    fn granted_manage_acl(&self, id: &Self::Id) -> bool {
        self.entries.get(id).is_some_and(|p| p.manage_acl)
    }

    fn granted_migrate(&self, id: &Self::Id) -> bool {
        self.entries.get(id).is_some_and(|p| p.migrate)
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
        let entry = self.entries.entry(id).or_default();
        entry.apply_grant(grant);
        self.save(mm)
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
        if !self.entries.contains_key(id) {
            return Ok(());
        }
        if matches!(revoke, PermRevoke::ManageAcl)
            && self.entries.get(id).is_some_and(|p| p.manage_acl)
            && self.manage_acl_count() == 1
        {
            return Err(MemoryError::ConstraintViolation(
                "at least one identity must retain manage_acl".to_string(),
            ));
        }
        let entry = self.entries.get_mut(id).expect("checked above");
        entry.apply_revoke(revoke);
        if entry.is_empty() {
            self.entries.remove(id);
        }
        self.save(mm)
    }

    fn remove_identity<M>(&mut self, id: &Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        let Some(entry) = self.entries.get(id) else {
            return Ok(());
        };
        if entry.manage_acl && self.manage_acl_count() == 1 {
            return Err(MemoryError::ConstraintViolation(
                "at least one identity must retain manage_acl".to_string(),
            ));
        }
        self.entries.remove(id);
        self.save(mm)
    }

    fn perms(&self, id: &Self::Id) -> IdentityPerms {
        self.entries.get(id).cloned().unwrap_or_default()
    }

    fn identities(&self) -> Vec<(Self::Id, IdentityPerms)> {
        self.entries
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

impl Encode for AccessControlList {
    const SIZE: DataSize = DataSize::Dynamic;
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        bytes.push(LAYOUT_VERSION);
        let count = self.entries.len() as u32;
        bytes.extend_from_slice(&count.to_le_bytes());
        // Sorted iteration for deterministic encoding (helps tests).
        let mut sorted: Vec<_> = self.entries.iter().collect();
        sorted.sort_by_key(|(a, _)| *a);
        for (id, perms) in sorted {
            bytes.push(id.len() as u8);
            bytes.extend_from_slice(id);
            bytes.push(Self::flags_byte(perms));
            bytes.push(perms.all_tables.bits());
            let pt_count = perms.per_table.len() as u16;
            bytes.extend_from_slice(&pt_count.to_le_bytes());
            let mut pt_sorted: Vec<_> = perms.per_table.iter().collect();
            pt_sorted.sort_by_key(|(t, _)| *t);
            for (table, p) in pt_sorted {
                bytes.extend_from_slice(&table.to_le_bytes());
                bytes.push(p.bits());
            }
        }
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let mut offset = 0;
        let version = *data.get(offset).ok_or(MemoryError::AclLayoutUnsupported)?;
        offset += 1;
        if version != LAYOUT_VERSION {
            return Err(MemoryError::AclLayoutUnsupported);
        }
        let count_bytes = data
            .get(offset..offset + 4)
            .ok_or(MemoryError::AclLayoutUnsupported)?;
        offset += 4;
        let count = u32::from_le_bytes(count_bytes.try_into()?) as usize;
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            let id_len = *data.get(offset).ok_or(MemoryError::AclLayoutUnsupported)? as usize;
            offset += 1;
            let id = data
                .get(offset..offset + id_len)
                .ok_or(MemoryError::AclLayoutUnsupported)?
                .to_vec();
            offset += id_len;
            let flags = *data.get(offset).ok_or(MemoryError::AclLayoutUnsupported)?;
            offset += 1;
            let (admin, manage_acl, migrate) = Self::perms_from_flags(flags);
            let all_tables = TablePerms::from_bits_truncate(
                *data.get(offset).ok_or(MemoryError::AclLayoutUnsupported)?,
            );
            offset += 1;
            let pt_count_bytes = data
                .get(offset..offset + 2)
                .ok_or(MemoryError::AclLayoutUnsupported)?;
            offset += 2;
            let pt_count = u16::from_le_bytes(pt_count_bytes.try_into()?) as usize;
            let mut per_table = Vec::with_capacity(pt_count);
            for _ in 0..pt_count {
                let table_bytes = data
                    .get(offset..offset + 8)
                    .ok_or(MemoryError::AclLayoutUnsupported)?;
                offset += 8;
                let table = TableFingerprint::from_le_bytes(table_bytes.try_into()?);
                let p_byte = *data.get(offset).ok_or(MemoryError::AclLayoutUnsupported)?;
                offset += 1;
                per_table.push((table, TablePerms::from_bits_truncate(p_byte)));
            }
            entries.insert(
                id,
                IdentityPerms {
                    admin,
                    manage_acl,
                    migrate,
                    all_tables,
                    per_table,
                },
            );
        }
        Ok(AccessControlList { entries })
    }

    fn size(&self) -> MSize {
        let per_entry: MSize = self
            .entries
            .iter()
            .map(|(id, p)| 1 + id.len() as MSize + 1 + 1 + 2 + p.per_table.len() as MSize * (8 + 1))
            .sum();
        1 + 4 + per_entry
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::fingerprint_for_name;

    use super::*;
    use crate::HeapMemoryProvider;

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    fn fp(name: &str) -> TableFingerprint {
        fingerprint_for_name(name)
    }

    #[test]
    fn test_v2_round_trip_empty() {
        let acl = AccessControlList::default();
        let bytes = acl.encode();
        let decoded = AccessControlList::decode(bytes).unwrap();
        assert_eq!(acl, decoded);
    }

    #[test]
    fn test_v2_round_trip_mixed() {
        let mut acl = AccessControlList::default();
        let alice = IdentityPerms {
            admin: true,
            manage_acl: true,
            ..Default::default()
        };
        let mut bob = IdentityPerms::default();
        bob.all_tables = TablePerms::READ;
        bob.apply_grant(PermGrant::Table(
            fp("users"),
            TablePerms::INSERT | TablePerms::UPDATE,
        ));
        bob.apply_grant(PermGrant::Table(fp("posts"), TablePerms::READ));
        acl.entries.insert(b"alice".to_vec(), alice);
        acl.entries.insert(b"bob".to_vec(), bob);
        let bytes = acl.encode();
        let decoded = AccessControlList::decode(bytes).unwrap();
        assert_eq!(acl, decoded);
    }

    #[test]
    fn test_decode_rejects_wrong_version() {
        let bad = vec![0x99, 0, 0, 0, 0];
        let err = AccessControlList::decode(std::borrow::Cow::Owned(bad)).unwrap_err();
        assert!(matches!(err, MemoryError::AclLayoutUnsupported));
    }

    #[test]
    fn test_grant_persists_through_save_and_load() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        acl.grant(b"alice".to_vec(), PermGrant::Admin, &mut mm)
            .unwrap();
        let loaded = AccessControlList::load(&mut mm).unwrap();
        assert!(loaded.granted_admin(&b"alice".to_vec()));
    }

    #[test]
    fn test_granted_truth_table() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        acl.grant(
            b"alice".to_vec(),
            PermGrant::Table(fp("users"), TablePerms::READ | TablePerms::INSERT),
            &mut mm,
        )
        .unwrap();
        let id = b"alice".to_vec();
        assert!(acl.granted(&id, fp("users"), TablePerms::READ));
        assert!(acl.granted(&id, fp("users"), TablePerms::INSERT));
        assert!(!acl.granted(&id, fp("users"), TablePerms::DELETE));
        assert!(!acl.granted(&id, fp("posts"), TablePerms::READ));
    }

    #[test]
    fn test_revoke_partial_keeps_remaining_bits() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let id = b"alice".to_vec();
        acl.grant(
            id.clone(),
            PermGrant::Table(
                fp("users"),
                TablePerms::READ | TablePerms::INSERT | TablePerms::DELETE,
            ),
            &mut mm,
        )
        .unwrap();
        acl.revoke(
            &id,
            PermRevoke::Table(fp("users"), TablePerms::INSERT | TablePerms::DELETE),
            &mut mm,
        )
        .unwrap();
        assert!(acl.granted(&id, fp("users"), TablePerms::READ));
        assert!(!acl.granted(&id, fp("users"), TablePerms::INSERT));
    }

    #[test]
    fn test_last_manage_acl_revoke_rejected() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let id = b"alice".to_vec();
        acl.grant(id.clone(), PermGrant::ManageAcl, &mut mm)
            .unwrap();
        let err = acl.revoke(&id, PermRevoke::ManageAcl, &mut mm).unwrap_err();
        assert!(matches!(err, MemoryError::ConstraintViolation(_)));
    }

    #[test]
    fn test_remove_last_manage_acl_identity_rejected() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let id = b"alice".to_vec();
        acl.grant(id.clone(), PermGrant::ManageAcl, &mut mm)
            .unwrap();
        let err = acl.remove_identity(&id, &mut mm).unwrap_err();
        assert!(matches!(err, MemoryError::ConstraintViolation(_)));
    }

    #[test]
    fn test_remove_identity_without_manage_acl_succeeds() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        acl.grant(b"alice".to_vec(), PermGrant::ManageAcl, &mut mm)
            .unwrap();
        acl.grant(b"bob".to_vec(), PermGrant::Admin, &mut mm)
            .unwrap();
        acl.remove_identity(&b"bob".to_vec(), &mut mm).unwrap();
        assert!(acl.identities().iter().all(|(id, _)| id != b"bob"));
    }

    #[test]
    fn test_admin_does_not_imply_manage_acl_or_migrate() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let id = b"alice".to_vec();
        acl.grant(id.clone(), PermGrant::Admin, &mut mm).unwrap();
        assert!(acl.granted_admin(&id));
        assert!(!acl.granted_manage_acl(&id));
        assert!(!acl.granted_migrate(&id));
    }

    #[test]
    fn test_revoke_unknown_identity_is_noop() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        acl.revoke(&b"ghost".to_vec(), PermRevoke::Admin, &mut mm)
            .unwrap();
        assert!(acl.identities().is_empty());
    }

    #[test]
    fn test_grant_with_idempotency_does_not_duplicate() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let id = b"alice".to_vec();
        acl.grant(id.clone(), PermGrant::Admin, &mut mm).unwrap();
        acl.grant(id.clone(), PermGrant::Admin, &mut mm).unwrap();
        assert_eq!(acl.identities().len(), 1);
    }
}

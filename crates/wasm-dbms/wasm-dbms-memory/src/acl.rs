// Rust guideline compliant 2026-02-28

use wasm_dbms_api::prelude::DEFAULT_ALIGNMENT;
use wasm_dbms_api::prelude::{DataSize, Encode, MSize, MemoryResult, PageOffset};

use crate::{MemoryManager, MemoryProvider};

/// Access control list module.
///
/// Takes care of storing and retrieving the list of caller identities
/// that have access to the database.
///
/// Identities are stored as raw byte slices (`Vec<u8>`) so that the
/// memory layer stays runtime-agnostic (no dependency on `candid::Principal`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessControlList {
    allowed: Vec<Vec<u8>>,
}

impl AccessControlList {
    /// Load [`AccessControlList`] from memory.
    pub fn load(mm: &MemoryManager<impl MemoryProvider>) -> MemoryResult<Self> {
        // read memory location from MemoryManager
        mm.read_at(mm.acl_page(), 0)
    }

    /// Save [`AccessControlList`] to memory.
    pub fn save(&self, mm: &mut MemoryManager<impl MemoryProvider>) -> MemoryResult<()> {
        mm.write_at(mm.acl_page(), 0, self)
    }

    /// Get the list of allowed caller identities.
    pub fn allowed_principals(&self) -> &[Vec<u8>] {
        &self.allowed
    }

    /// Get whether a caller identity is allowed.
    pub fn is_allowed(&self, identity: &[u8]) -> bool {
        self.allowed.iter().any(|a| a.as_slice() == identity)
    }

    /// Add a caller identity to the allowed list.
    ///
    /// If the identity is already present, do nothing.
    /// Otherwise, add the identity and write the updated ACL to memory.
    pub fn add_principal(
        &mut self,
        identity: Vec<u8>,
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<()> {
        if !self.is_allowed(&identity) {
            self.allowed.push(identity);
            self.save(mm)?;
        }

        Ok(())
    }

    /// Remove a caller identity from the allowed list.
    ///
    /// If the identity is not present, do nothing.
    /// Otherwise, remove the identity and write the updated ACL to memory.
    pub fn remove_principal(
        &mut self,
        identity: &[u8],
        mm: &mut MemoryManager<impl MemoryProvider>,
    ) -> MemoryResult<()> {
        if let Some(pos) = self.allowed.iter().position(|p| p.as_slice() == identity) {
            self.allowed.swap_remove(pos);
            self.save(mm)?;
        }
        // trap if empty ACL
        if self.allowed.is_empty() {
            panic!("ACL cannot be empty");
        }

        Ok(())
    }
}

impl Encode for AccessControlList {
    const SIZE: DataSize = DataSize::Dynamic;

    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        // write the number of identities as u32 followed by each identity's bytes
        let mut bytes = Vec::with_capacity(self.size() as usize);
        let len = self.allowed.len() as u32;
        bytes.extend_from_slice(&len.to_le_bytes());
        for identity in &self.allowed {
            let identity_len = identity.len() as u8;
            bytes.extend_from_slice(&identity_len.to_le_bytes());
            bytes.extend_from_slice(identity);
        }
        std::borrow::Cow::Owned(bytes)
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        // read the number of identities as u32 followed by each identity's bytes
        let mut offset = 0;
        let len_bytes = &data[offset..offset + 4];
        offset += 4;
        let len = u32::from_le_bytes(len_bytes.try_into()?) as usize;

        // init vec
        let mut allowed = Vec::with_capacity(len);
        for _ in 0..len {
            let identity_len_bytes = &data[offset..offset + 1];
            offset += 1;
            let identity_len = u8::from_le_bytes(identity_len_bytes.try_into()?) as usize;

            let identity_bytes = data[offset..offset + identity_len].to_vec();
            offset += identity_len;

            allowed.push(identity_bytes);
        }
        Ok(AccessControlList { allowed })
    }

    fn size(&self) -> MSize {
        // 4 bytes for len + sum of each identity's length (1 byte for length + bytes)
        4 + self
            .allowed
            .iter()
            .map(|p| 1 + p.len() as MSize)
            .sum::<MSize>()
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::HeapMemoryProvider;

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    #[test]
    fn test_acl_encode_decode() {
        let acl = AccessControlList {
            allowed: vec![
                vec![0x04], // anonymous-like identity
                vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01], // management canister-like
                vec![0xDE, 0xAD, 0xBE, 0xEF],
                vec![0x01, 0x02, 0x03, 0x04, 0x05],
            ],
        };

        let encoded = acl.encode();
        let decoded = AccessControlList::decode(encoded).unwrap();

        assert_eq!(acl, decoded);
    }

    #[test]
    fn test_acl_add_remove_principal() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        assert!(!acl.is_allowed(&identity));
        acl.add_principal(identity.clone(), &mut mm).unwrap();
        let other = vec![0xDE, 0xAD, 0xBE, 0xEF];
        acl.add_principal(other.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity));
        assert!(acl.is_allowed(&other));
        assert_eq!(acl.allowed.len(), 2);
        acl.remove_principal(&other, &mut mm).unwrap();
    }

    #[test]
    #[should_panic(expected = "ACL cannot be empty")]
    fn test_remove_last_principal_traps() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        acl.add_principal(identity.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity));
        acl.remove_principal(&identity, &mut mm).unwrap(); // should panic
    }

    #[test]
    fn test_should_add_more_principals() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity1 = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        let identity2 = vec![0xDE, 0xAD, 0xBE, 0xEF];
        acl.add_principal(identity1.clone(), &mut mm).unwrap();
        acl.add_principal(identity2.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity1));
        assert!(acl.is_allowed(&identity2));
        assert_eq!(
            acl.allowed_principals(),
            &[identity1.clone(), identity2.clone()]
        );
    }

    #[test]
    fn test_add_principal_should_write_to_memory() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        acl.add_principal(identity.clone(), &mut mm).unwrap();

        // Load from memory and check if the identity is present
        let loaded_acl = AccessControlList::load(&mm).unwrap();
        assert!(loaded_acl.is_allowed(&identity));
    }
}

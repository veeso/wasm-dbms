// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-PUBLIC-DEBUG, M-CANONICAL-DOCS

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, Encode, MSize, MemoryError, MemoryResult, PageOffset,
};

use crate::{MemoryAccess, MemoryManager, MemoryProvider};

/// Trait for access control providers.
///
/// Each implementation specifies its own `Id` type so that runtimes
/// can use native identity representations.
///
/// Runtimes that need ACL use [`AccessControlList`] (the default).
/// Runtimes without ACL use [`NoAccessControl`] which allows everything.
pub trait AccessControl: Default {
    /// The identity type used by this access control provider.
    type Id;

    /// Loads ACL state from persisted memory.
    fn load<M>(mm: &MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
        Self: Sized;

    /// Checks whether an identity is allowed.
    fn is_allowed(&self, identity: &Self::Id) -> bool;

    /// Returns all allowed identities.
    fn allowed_identities(&self) -> Vec<Self::Id>;

    /// Adds an identity and persists the change.
    fn add_identity<M>(
        &mut self,
        identity: Self::Id,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider;

    /// Removes an identity and persists the change.
    fn remove_identity<M>(
        &mut self,
        identity: &Self::Id,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider;
}

/// ACL provider that allows all identities unconditionally.
///
/// Use this for runtimes that handle authorization externally
/// or do not need access control.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct NoAccessControl;

impl AccessControl for NoAccessControl {
    type Id = ();

    fn load<M>(_mm: &MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
    {
        Ok(Self)
    }

    fn is_allowed(&self, _identity: &Self::Id) -> bool {
        true
    }

    fn allowed_identities(&self) -> Vec<Self::Id> {
        vec![]
    }

    fn add_identity<M>(
        &mut self,
        _identity: Self::Id,
        _mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        Ok(())
    }

    fn remove_identity<M>(
        &mut self,
        _identity: &Self::Id,
        _mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        Ok(())
    }
}

/// Access control list storing allowed identities as raw bytes.
///
/// Identities are stored as `Vec<u8>` so that the memory layer stays
/// runtime-agnostic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AccessControlList {
    allowed: Vec<Vec<u8>>,
}

impl AccessControlList {
    /// Saves the current ACL state to memory.
    fn save<M>(&self, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        mm.write_at(mm.acl_page(), 0, self)
    }
}

impl AccessControl for AccessControlList {
    type Id = Vec<u8>;

    fn load<M>(mm: &MemoryManager<M>) -> MemoryResult<Self>
    where
        M: MemoryProvider,
    {
        mm.read_at(mm.acl_page(), 0)
    }

    fn is_allowed(&self, identity: &Self::Id) -> bool {
        self.allowed
            .iter()
            .any(|a| a.as_slice() == identity.as_slice())
    }

    fn allowed_identities(&self) -> Vec<Self::Id> {
        self.allowed.clone()
    }

    fn add_identity<M>(&mut self, identity: Self::Id, mm: &mut MemoryManager<M>) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        if !self.is_allowed(&identity) {
            self.allowed.push(identity);
            self.save(mm)?;
        }

        Ok(())
    }

    fn remove_identity<M>(
        &mut self,
        identity: &Self::Id,
        mm: &mut MemoryManager<M>,
    ) -> MemoryResult<()>
    where
        M: MemoryProvider,
    {
        if let Some(pos) = self
            .allowed
            .iter()
            .position(|p| p.as_slice() == identity.as_slice())
        {
            if self.allowed.len() == 1 {
                return Err(MemoryError::ConstraintViolation(
                    "ACL must contain at least one identity".to_string(),
                ));
            }
            self.allowed.swap_remove(pos);
            self.save(mm)?;
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
                vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x01],
                vec![0xDE, 0xAD, 0xBE, 0xEF],
                vec![0x01, 0x02, 0x03, 0x04, 0x05],
            ],
        };

        let encoded = acl.encode();
        let decoded = AccessControlList::decode(encoded).unwrap();

        assert_eq!(acl, decoded);
    }

    #[test]
    fn test_acl_add_remove_identity() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        assert!(!acl.is_allowed(&identity));
        acl.add_identity(identity.clone(), &mut mm).unwrap();
        let other = vec![0xDE, 0xAD, 0xBE, 0xEF];
        acl.add_identity(other.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity));
        assert!(acl.is_allowed(&other));
        assert_eq!(acl.allowed_identities().len(), 2);
        acl.remove_identity(&other, &mut mm).unwrap();
    }

    #[test]
    fn test_remove_last_identity_returns_error() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        acl.add_identity(identity.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity));
        let result = acl.remove_identity(&identity, &mut mm);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MemoryError::ConstraintViolation(_)
        ));
    }

    #[test]
    fn test_should_add_more_identities() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity1 = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        let identity2 = vec![0xDE, 0xAD, 0xBE, 0xEF];
        acl.add_identity(identity1.clone(), &mut mm).unwrap();
        acl.add_identity(identity2.clone(), &mut mm).unwrap();
        assert!(acl.is_allowed(&identity1));
        assert!(acl.is_allowed(&identity2));
        assert_eq!(
            acl.allowed_identities(),
            vec![identity1.clone(), identity2.clone()]
        );
    }

    #[test]
    fn test_add_identity_should_write_to_memory() {
        let mut mm = make_mm();
        let mut acl = AccessControlList::default();
        let identity = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x01];
        acl.add_identity(identity.clone(), &mut mm).unwrap();

        // Load from memory and check if the identity is present
        let loaded_acl = AccessControlList::load(&mm).unwrap();
        assert!(loaded_acl.is_allowed(&identity));
    }

    #[test]
    fn test_no_access_control_allows_everything() {
        let acl = NoAccessControl;
        assert!(acl.is_allowed(&()));
        assert!(acl.allowed_identities().is_empty());
    }
}

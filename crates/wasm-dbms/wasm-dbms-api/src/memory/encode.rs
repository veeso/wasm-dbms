use std::borrow::Cow;

use crate::memory::{MSize, MemoryResult};
use crate::prelude::PageOffset;

/// Default alignment in bytes for [`DataSize::Dynamic`] data types.
pub const DEFAULT_ALIGNMENT: MSize = 32;

/// This trait defines the encoding and decoding behaviour for data types used in the DBMS canister.
pub trait Encode: Clone {
    /// The size characteristic of the data type.
    ///
    /// The [`DataSize`] can either be a fixed size in bytes or dynamic.
    const SIZE: DataSize;

    /// The alignment requirement in bytes for the data type.
    ///
    /// If [`Self::SIZE`] is [`DataSize::Fixed`], the alignment must be equal to the size,
    /// otherwise it can be any value.
    ///
    /// This value  should never be less than 8 for [`DataSize::Dynamic`] types to ensure proper memory alignment.
    ///
    /// We should set a default value (probably 32) for dynamic types to avoid misalignment issues, but letting an expert user to
    /// override it if necessary.
    const ALIGNMENT: PageOffset;

    /// Encodes the data type into a vector of bytes.
    fn encode(&'_ self) -> Cow<'_, [u8]>;

    /// Decodes the data type from a slice of bytes.
    fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized;

    /// Returns the size in bytes of the encoded data type.
    fn size(&self) -> MSize;
}

/// Represents the size of data types used in the DBMS canister.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSize {
    /// A fixed size in bytes.
    Fixed(MSize),
    /// A variable size.
    Dynamic,
}

impl DataSize {
    /// Returns the size in bytes if the data size is fixed.
    pub fn get_fixed_size(&self) -> Option<MSize> {
        match self {
            DataSize::Fixed(size) => Some(*size),
            DataSize::Dynamic => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_get_data_size_fixed() {
        let size = DataSize::Fixed(10);
        assert_eq!(size.get_fixed_size(), Some(10));

        let variable_size = DataSize::Dynamic;
        assert_eq!(variable_size.get_fixed_size(), None);
    }
}

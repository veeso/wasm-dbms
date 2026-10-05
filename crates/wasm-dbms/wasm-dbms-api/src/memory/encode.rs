use std::borrow::Cow;

use crate::memory::{MSize, MemoryResult};
use crate::prelude::PageOffset;

/// Default alignment in bytes for [`DataSize::Dynamic`] data types.
pub const DEFAULT_ALIGNMENT: MSize = 32;

/// This trait defines the encoding and decoding behaviour for data types used in the DBMS.
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
    ///
    /// Implementations must never panic or wrap: when the encoded size does
    /// not fit an [`MSize`], they must return [`MSize::MAX`]. No record of
    /// that size fits a page together with its header, so storage rejects it
    /// with [`MemoryError::DataTooLarge`](crate::memory::MemoryError::DataTooLarge)
    /// before calling [`Self::encode`]. The output of [`Self::encode`] is
    /// unspecified for such values.
    fn size(&self) -> MSize;
}

/// Returns the encoded size of a `payload_len`-byte payload preceded by a
/// `header`-byte header, saturating at [`MSize::MAX`] when it does not fit.
pub(crate) fn saturating_size(header: MSize, payload_len: usize) -> MSize {
    MSize::try_from(payload_len).map_or(MSize::MAX, |len| header.saturating_add(len))
}

/// Encodes `len` as the 2-byte little-endian length prefix used by dynamic
/// payloads, saturating at [`u16::MAX`] when it does not fit.
///
/// A saturated prefix only occurs for values whose [`Encode::size`] is
/// [`MSize::MAX`], which storage rejects before encoding.
pub(crate) fn length_prefix(len: usize) -> [u8; 2] {
    u16::try_from(len).unwrap_or(u16::MAX).to_le_bytes()
}

/// Represents the size of data types used in the DBMS.
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
    fn test_saturating_size_is_exact_when_it_fits() {
        assert_eq!(saturating_size(2, 0), 2);
        assert_eq!(saturating_size(2, 65_533), MSize::MAX);
        assert_eq!(saturating_size(0, 65_535), MSize::MAX);
    }

    #[test]
    fn test_saturating_size_saturates_when_it_does_not_fit() {
        assert_eq!(saturating_size(2, 65_534), MSize::MAX);
        assert_eq!(saturating_size(0, 65_536), MSize::MAX);
        assert_eq!(saturating_size(2, usize::MAX), MSize::MAX);
    }

    #[test]
    fn test_length_prefix_saturates_at_u16_max() {
        assert_eq!(length_prefix(0), [0, 0]);
        assert_eq!(length_prefix(65_535), [0xFF, 0xFF]);
        assert_eq!(length_prefix(65_536), [0xFF, 0xFF]);
    }

    #[test]
    fn test_should_get_data_size_fixed() {
        let size = DataSize::Fixed(10);
        assert_eq!(size.get_fixed_size(), Some(10));

        let variable_size = DataSize::Dynamic;
        assert_eq!(variable_size.get_fixed_size(), None);
    }
}

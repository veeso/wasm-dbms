use std::array::TryFromSliceError;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::memory::{Page, PageOffset};

/// An enum representing possible memory-related errors.
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[derive(Debug, Error, Deserialize, Serialize)]
pub enum MemoryError {
    /// Error when an autoincrement column has reached its maximum value.
    #[error("Autoincrement overflow: {0} column has reached its maximum value")]
    AutoincrementOverflow(String),
    /// Error when a constraint prevents the requested operation.
    #[error("Constraint violation: {0}")]
    ConstraintViolation(String),
    /// Error when the data to be written is too large for the page.
    #[error("Data too large for page (page size: {page_size}, requested: {requested})")]
    DataTooLarge { page_size: u64, requested: u64 },
    /// Error when failing to decode data from bytes.
    #[error("Failed to decode data from bytes: {0}")]
    DecodeError(DecodeError),
    /// Error when failing to allocate a new page.
    #[error("Failed to allocate a new page")]
    FailedToAllocatePage,
    /// Error when no index exists for the requested columns.
    #[error("Index not found for columns: {0:?}")]
    IndexNotFound(Vec<String>),
    /// Error when registering a table whose name hash collides with an
    /// already-registered table of a different name.
    #[error(
        "Name hash collision: table `{candidate}` hashes to the same value as already-registered table `{existing}`"
    )]
    NameCollision {
        /// Name of the table being registered.
        candidate: String,
        /// Name of the already-registered table that produced the same hash.
        existing: String,
    },
    /// Error when an index entry cannot be located.
    #[error("Entry not found in index")]
    EntryNotFound,
    /// Error when a single key cannot fit into a node page.
    #[error("Key too large: {size} bytes exceeds maximum {max} bytes")]
    KeyTooLarge { size: u64, max: u64 },
    #[error("Offset {offset} is not aligned to {alignment} bytes")]
    OffsetNotAligned { offset: PageOffset, alignment: u16 },
    /// Error when attempting to access stable memory out of bounds.
    #[error("Stable memory access out of bounds")]
    OutOfBounds,
    /// Error when attempting to write out of the allocated page.
    #[error(
        "Tried to read or write out of the allocated page (page: {page}, offset: {offset}, data size: {data_size}, page size: {page_size})"
    )]
    SegmentationFault {
        page: Page,
        offset: PageOffset,
        data_size: u64,
        page_size: u64,
    },
    /// Error from the underlying memory provider.
    #[error("Memory provider error: {0}")]
    ProviderError(String),
}

impl From<TryFromSliceError> for MemoryError {
    fn from(err: TryFromSliceError) -> Self {
        MemoryError::DecodeError(DecodeError::from(err))
    }
}

impl From<std::string::FromUtf8Error> for MemoryError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        MemoryError::DecodeError(DecodeError::from(err))
    }
}

impl From<uuid::Error> for MemoryError {
    fn from(err: uuid::Error) -> Self {
        MemoryError::DecodeError(DecodeError::from(err))
    }
}

/// An enum representing possible decoding errors.
#[cfg_attr(feature = "candid", derive(candid::CandidType))]
#[derive(Debug, Error, Deserialize, Serialize)]
pub enum DecodeError {
    /// Error when the raw record header is invalid.
    #[error("Bad raw record header")]
    BadRawRecordHeader,
    /// Error when JSON is invalid.
    #[error("Invalid JSON: {0}")]
    InvalidJson(String),
    /// Identity decoding error.
    #[error("Identity decode error: {0}")]
    IdentityDecodeError(String),
    /// Error when failing to convert from slice.
    #[error("Failed to convert from slice: {0}")]
    TryFromSliceError(String),
    /// Error when failing to convert from UTF-8 string.
    #[error("Failed to convert from UTF-8 string: {0}")]
    Utf8Error(String),
    /// Error when the data is too short to decode.
    #[error("Data too short to decode")]
    TooShort,
    /// Error when an invalid discriminant byte is encountered.
    #[error("Invalid discriminant: {0}")]
    InvalidDiscriminant(u8),
    /// UUID error
    #[error("UUID error: {0}")]
    UuidError(String),
}

impl From<uuid::Error> for DecodeError {
    fn from(err: uuid::Error) -> Self {
        DecodeError::UuidError(err.to_string())
    }
}

impl From<std::string::FromUtf8Error> for DecodeError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        DecodeError::Utf8Error(err.to_string())
    }
}

impl From<TryFromSliceError> for DecodeError {
    fn from(err: TryFromSliceError) -> Self {
        DecodeError::TryFromSliceError(err.to_string())
    }
}

#[cfg(feature = "candid")]
impl From<candid::types::principal::PrincipalError> for DecodeError {
    fn from(err: candid::types::principal::PrincipalError) -> Self {
        DecodeError::IdentityDecodeError(err.to_string())
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_memory_error_display() {
        let error = MemoryError::DataTooLarge {
            page_size: 1024,
            requested: 2048,
        };
        assert_eq!(
            format!("{}", error),
            "Data too large for page (page size: 1024, requested: 2048)"
        );
    }

    #[test]
    fn test_decode_error_display() {
        let error = DecodeError::BadRawRecordHeader;
        assert_eq!(format!("{}", error), "Bad raw record header");
    }
}

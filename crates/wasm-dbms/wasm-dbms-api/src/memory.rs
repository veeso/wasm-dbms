mod encode;
mod error;

pub use self::encode::{DEFAULT_ALIGNMENT, DataSize, Encode};
pub use self::error::{DecodeError, MemoryError};

/// Type identifying a memory page number.
pub type Page = u32;
/// Type identifying an offset within a memory page.
pub type PageOffset = u16;
/// Size type for memory operations.
pub type MSize = u16;
/// The result type for memory operations.
pub type MemoryResult<T> = Result<T, MemoryError>;

// Rust guideline compliant 2026-03-01
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Trait abstracting page-level memory operations.
//!
//! [`MemoryAccess`] is the primary interface consumed by registry and
//! ledger code in this crate. [`MemoryManager`](crate::MemoryManager)
//! implements it with direct writes; the DBMS crate provides a
//! journaled wrapper that records original bytes before each write.

use wasm_dbms_api::prelude::{Encode, MemoryResult, Page, PageOffset};

/// Abstraction over page-level memory operations.
///
/// All table-registry and ledger functions are generic over this trait
/// so that callers can transparently add write-ahead journaling or
/// other interceptors without modifying the memory crate.
pub trait MemoryAccess {
    /// Returns the size of a single memory page.
    fn page_size(&self) -> u64;

    /// Allocates an additional page in memory and returns its number.
    fn allocate_page(&mut self) -> MemoryResult<Page>;

    /// Reads a typed value from the specified page and offset.
    fn read_at<D>(&self, page: Page, offset: PageOffset) -> MemoryResult<D>
    where
        D: Encode;

    /// Writes a typed value at the specified page and offset.
    fn write_at<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode;

    /// Zeros out the region occupied by `data` at the specified page
    /// and offset.
    fn zero<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode;

    /// Reads raw bytes into `buf` at the specified page and offset.
    ///
    /// Returns the number of bytes actually read.
    fn read_at_raw(&self, page: Page, offset: PageOffset, buf: &mut [u8]) -> MemoryResult<usize>;
}

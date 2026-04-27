// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Trait abstracting page-level memory operations.
//!
//! [`MemoryAccess`] is the primary interface consumed by registry and
//! ledger code in this crate. [`MemoryManager`](crate::MemoryManager)
//! implements it with direct writes; the DBMS crate provides a
//! journaled wrapper that records original bytes before each write.

use wasm_dbms_api::prelude::{Encode, MemoryResult, Page, PageOffset};

use crate::memory_manager::UNCLAIMED_PAGES_PAGE;
use crate::unclaimed_pages::UnclaimedPages;

/// Abstraction over page-level memory operations.
///
/// All table-registry and ledger functions are generic over this trait
/// so that callers can transparently add write-ahead journaling or
/// other interceptors without modifying the memory crate.
pub trait MemoryAccess {
    /// Returns the size of a single memory page.
    fn page_size(&self) -> u64;

    /// Grows the underlying memory by exactly one page and returns the
    /// freshly allocated page number.
    ///
    /// The returned page is zero-initialized. This primitive is **not
    /// journaled** — page growth cannot be rolled back, so a transaction
    /// that aborts after a `grow_one_page` simply leaks the new page.
    fn grow_one_page(&mut self) -> MemoryResult<Page>;

    /// Zeros out an entire page. Used by [`MemoryAccess::unclaim_page`]
    /// to scrub residual data before publishing the page to the
    /// unclaimed-pages ledger.
    fn zero_page(&mut self, page: Page) -> MemoryResult<()>;

    /// Hands out a page for use by a caller.
    ///
    /// Reuses a page from the unclaimed-pages ledger when one is
    /// available; otherwise grows the memory by one page.
    fn claim_page(&mut self) -> MemoryResult<Page> {
        let mut ledger: UnclaimedPages = self.read_at(UNCLAIMED_PAGES_PAGE, 0)?;
        if let Some(page) = ledger.pop() {
            self.write_at(UNCLAIMED_PAGES_PAGE, 0, &ledger)?;
            return Ok(page);
        }
        self.grow_one_page()
    }

    /// Returns `page` to the unclaimed-pages ledger so it can be reused
    /// by a future [`MemoryAccess::claim_page`] call.
    ///
    /// The page contents are zeroed before being published to the
    /// ledger.
    fn unclaim_page(&mut self, page: Page) -> MemoryResult<()> {
        self.zero_page(page)?;
        let mut ledger: UnclaimedPages = self.read_at(UNCLAIMED_PAGES_PAGE, 0)?;
        ledger.push(page)?;
        self.write_at(UNCLAIMED_PAGES_PAGE, 0, &ledger)
    }

    /// Reads a typed value from the specified page and offset.
    fn read_at<D>(&mut self, page: Page, offset: PageOffset) -> MemoryResult<D>
    where
        D: Encode;

    /// Writes a typed value at the specified page and offset.
    fn write_at<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode;

    /// Writes raw bytes at the specified page and offset, bypassing
    /// alignment and encoding checks.
    fn write_at_raw(&mut self, page: Page, offset: PageOffset, buf: &[u8]) -> MemoryResult<()>;

    /// Zeros out the region occupied by `data` at the specified page
    /// and offset.
    fn zero<E>(&mut self, page: Page, offset: PageOffset, data: &E) -> MemoryResult<()>
    where
        E: Encode;

    /// Zeros out `len` raw bytes at the specified page and offset.
    ///
    /// Used by the migration apply pipeline when scrubbing a record whose
    /// size is known only at runtime (from a stored snapshot).
    fn zero_raw(&mut self, page: Page, offset: PageOffset, len: PageOffset) -> MemoryResult<()>;

    /// Reads raw bytes into `buf` at the specified page and offset.
    ///
    /// Returns the number of bytes actually read.
    fn read_at_raw(
        &mut self,
        page: Page,
        offset: PageOffset,
        buf: &mut [u8],
    ) -> MemoryResult<usize>;
}

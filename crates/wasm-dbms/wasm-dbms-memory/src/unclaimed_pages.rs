// Rust guideline compliant 2026-04-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Unclaimed-pages ledger persisted on the reserved page
//! [`crate::memory_manager::UNCLAIMED_PAGES_PAGE`].
//!
//! Tracks pages that have been released by destructive operations (e.g.
//! `MigrationOp::DropTable`) so that they can be reused by future
//! [`MemoryAccess::claim_page`](crate::MemoryAccess::claim_page) calls
//! before bumping the high-water mark via the underlying provider.

use std::borrow::Cow;

use wasm_dbms_api::prelude::{
    DEFAULT_ALIGNMENT, DataSize, DecodeError, Encode, MSize, MemoryError, MemoryResult, Page,
    PageOffset,
};

/// Bytes of the on-page header (`u32` length prefix).
const HEADER_SIZE: u64 = 4;
/// Bytes of one page entry (`u32`).
const ENTRY_SIZE: u64 = 4;

/// Maximum number of entries that fit in the reserved page (64 KiB).
///
/// Computed at build time so that the encoded ledger size stays within
/// [`MSize`] (`u16`). Currently 16382 entries.
pub const UNCLAIMED_PAGES_CAPACITY: u32 = {
    let max_bytes = MSize::MAX as u64;
    let entries = (max_bytes - HEADER_SIZE) / ENTRY_SIZE;
    entries as u32
};

/// On-disk representation of the unclaimed-pages ledger.
///
/// The ledger is a LIFO stack of [`Page`] numbers. Push appends, pop
/// removes from the tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnclaimedPages {
    pages: Vec<Page>,
}

impl UnclaimedPages {
    /// Returns an empty ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of pages currently in the ledger.
    pub fn len(&self) -> usize {
        self.pages.len()
    }

    /// Returns how many additional pages can still be tracked.
    pub fn remaining_capacity(&self) -> u32 {
        UNCLAIMED_PAGES_CAPACITY - (self.pages.len() as u32)
    }

    /// Returns `true` when no pages are currently tracked.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Removes and returns the last unclaimed page, if any.
    pub fn pop(&mut self) -> Option<Page> {
        self.pages.pop()
    }

    /// Appends `page` to the ledger.
    ///
    /// # Errors
    ///
    /// [`MemoryError::UnclaimedPagesFull`] when the ledger is at capacity.
    pub fn push(&mut self, page: Page) -> MemoryResult<()> {
        if self.pages.len() as u32 >= UNCLAIMED_PAGES_CAPACITY {
            return Err(MemoryError::UnclaimedPagesFull {
                capacity: UNCLAIMED_PAGES_CAPACITY,
            });
        }
        self.pages.push(page);
        Ok(())
    }

    /// Returns a slice over the tracked pages (oldest first).
    pub fn as_slice(&self) -> &[Page] {
        &self.pages
    }
}

impl Encode for UnclaimedPages {
    const SIZE: DataSize = DataSize::Dynamic;
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    fn encode(&'_ self) -> Cow<'_, [u8]> {
        let mut buf = Vec::with_capacity(self.size() as usize);
        buf.extend_from_slice(&(self.pages.len() as u32).to_le_bytes());
        for &page in &self.pages {
            buf.extend_from_slice(&page.to_le_bytes());
        }
        Cow::Owned(buf)
    }

    fn decode(data: Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        if data.len() < HEADER_SIZE as usize {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let count = u32::from_le_bytes(data[0..4].try_into()?) as usize;
        if count > UNCLAIMED_PAGES_CAPACITY as usize {
            return Err(MemoryError::UnclaimedPagesFull {
                capacity: UNCLAIMED_PAGES_CAPACITY,
            });
        }
        // `count` is bounded by the capacity, so this cannot overflow.
        let required = HEADER_SIZE as usize + count * ENTRY_SIZE as usize;
        if data.len() < required {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }
        let pages = data[HEADER_SIZE as usize..required]
            .as_chunks::<{ ENTRY_SIZE as usize }>()
            .0
            .iter()
            .map(|entry| Page::from_le_bytes(*entry))
            .collect();
        Ok(Self { pages })
    }

    fn size(&self) -> MSize {
        (HEADER_SIZE as MSize) + (self.pages.len() as MSize) * (ENTRY_SIZE as MSize)
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::DecodeError;

    use super::*;

    #[test]
    fn test_should_be_empty_by_default() {
        let ledger = UnclaimedPages::new();
        assert!(ledger.is_empty());
        assert_eq!(ledger.len(), 0);
    }

    #[test]
    fn test_should_push_and_pop() {
        let mut ledger = UnclaimedPages::new();
        ledger.push(10).expect("push");
        ledger.push(20).expect("push");
        ledger.push(30).expect("push");

        assert_eq!(ledger.len(), 3);
        assert_eq!(ledger.pop(), Some(30));
        assert_eq!(ledger.pop(), Some(20));
        assert_eq!(ledger.pop(), Some(10));
        assert_eq!(ledger.pop(), None);
    }

    #[test]
    fn test_should_round_trip_encode_decode() {
        let mut ledger = UnclaimedPages::new();
        for page in [3u32, 5, 7, 11] {
            ledger.push(page).expect("push");
        }

        let encoded = ledger.encode();
        let decoded = UnclaimedPages::decode(encoded).expect("decode");
        assert_eq!(ledger, decoded);
    }

    #[test]
    fn test_should_decode_empty_buffer_as_empty_ledger() {
        let buf = vec![0u8; 65536];
        let ledger = UnclaimedPages::decode(Cow::Owned(buf)).expect("decode");
        assert!(ledger.is_empty());
    }

    #[test]
    fn test_should_reject_truncated_buffers() {
        // Header shorter than 4 bytes.
        let short_header = UnclaimedPages::decode(Cow::Borrowed(&[1u8, 0][..]));
        assert!(matches!(
            short_header,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));

        // Header declares one entry, but no entry bytes follow.
        let missing_entry = UnclaimedPages::decode(Cow::Borrowed(&[1u8, 0, 0, 0][..]));
        assert!(matches!(
            missing_entry,
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_should_reject_push_when_full() {
        let mut ledger = UnclaimedPages {
            pages: vec![0; UNCLAIMED_PAGES_CAPACITY as usize],
        };
        let err = ledger.push(42).expect_err("push at capacity");
        assert!(matches!(err, MemoryError::UnclaimedPagesFull { .. }));
    }

    #[test]
    fn test_size_matches_encoded_length() {
        let mut ledger = UnclaimedPages::new();
        ledger.push(1).expect("push");
        ledger.push(2).expect("push");
        let encoded = ledger.encode();
        assert_eq!(ledger.size() as usize, encoded.len());
    }
}

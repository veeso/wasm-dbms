// Rust guideline compliant 2026-03-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Forward cursor for B+ tree range scans.

use wasm_dbms_api::memory::{Encode, MemoryResult, Page};

use super::{BTreeNode, LeafEntry, NodeBody, RecordAddress};
use crate::MemoryAccess;

/// Stateful forward range-scan cursor over leaf entries.
pub struct IndexTreeWalker<K>
where
    K: Encode + Ord,
{
    entries: Vec<LeafEntry<K>>,
    cursor: usize,
    next_leaf: Option<Page>,
    end_key: Option<K>,
}

impl<K> IndexTreeWalker<K>
where
    K: Encode + Ord,
{
    pub(super) fn new(
        entries: Vec<LeafEntry<K>>,
        cursor: usize,
        next_leaf: Option<Page>,
        end_key: Option<K>,
    ) -> Self {
        Self {
            entries,
            cursor,
            next_leaf,
            end_key,
        }
    }

    /// Returns the next pointer in the scan, or `None` when exhausted.
    pub fn next(&mut self, mm: &mut impl MemoryAccess) -> MemoryResult<Option<RecordAddress>> {
        loop {
            if let Some(entry) = self.entries.get(self.cursor) {
                if self.is_past_end(&entry.key) {
                    return Ok(None);
                }

                self.cursor += 1;
                return Ok(Some(entry.pointer));
            }

            if !self.load_next_leaf(mm)? {
                return Ok(None);
            }
        }
    }

    /// Returns the next key and pointer in the scan, or `None` when exhausted.
    ///
    /// Unlike [`Self::peek_key`], which only sees the current leaf, this
    /// advances across exhausted and emptied leaves before deciding that the
    /// scan has ended.
    pub fn next_entry(
        &mut self,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Option<(K, RecordAddress)>> {
        loop {
            if let Some(entry) = self.entries.get(self.cursor) {
                if self.is_past_end(&entry.key) {
                    return Ok(None);
                }
                self.cursor += 1;
                return Ok(Some((entry.key.clone(), entry.pointer)));
            }
            if !self.load_next_leaf(mm)? {
                return Ok(None);
            }
        }
    }

    /// Peeks at the next key without advancing the cursor.
    pub fn peek_key(&self) -> Option<&K> {
        self.entries.get(self.cursor).map(|entry| &entry.key)
    }

    /// Returns whether `key` is at or after the exclusive end of the scan.
    fn is_past_end(&self, key: &K) -> bool {
        self.end_key.as_ref().is_some_and(|end_key| key >= end_key)
    }

    /// Loads the next leaf of the sibling chain, returning `false` at the end.
    fn load_next_leaf(&mut self, mm: &mut impl MemoryAccess) -> MemoryResult<bool> {
        let Some(page) = self.next_leaf else {
            return Ok(false);
        };
        match BTreeNode::<K>::read(page, mm)?.body {
            NodeBody::Leaf(leaf) => {
                self.entries = leaf.entries;
                self.cursor = 0;
                self.next_leaf = leaf.next_leaf;
                Ok(true)
            }
            NodeBody::Internal(_) => Ok(false),
        }
    }
}

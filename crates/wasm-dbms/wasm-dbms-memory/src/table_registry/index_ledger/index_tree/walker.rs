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
    pub fn next(&mut self, mm: &impl MemoryAccess) -> MemoryResult<Option<RecordAddress>> {
        loop {
            if let Some(entry) = self.entries.get(self.cursor) {
                if self
                    .end_key
                    .as_ref()
                    .is_some_and(|end_key| entry.key >= *end_key)
                {
                    return Ok(None);
                }

                self.cursor += 1;
                return Ok(Some(entry.pointer));
            }

            match self.next_leaf {
                Some(page) => {
                    let node = BTreeNode::<K>::read(page, mm)?;
                    match node.body {
                        NodeBody::Leaf(leaf) => {
                            self.entries = leaf.entries;
                            self.cursor = 0;
                            self.next_leaf = leaf.next_leaf;
                        }
                        NodeBody::Internal(_) => return Ok(None),
                    }
                }
                None => return Ok(None),
            }
        }
    }

    /// Peeks at the next key without advancing the cursor.
    pub fn peek_key(&self) -> Option<&K> {
        self.entries.get(self.cursor).map(|entry| &entry.key)
    }
}

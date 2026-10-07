// Rust guideline compliant 2026-10-07
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Read-only view over a serialized B+ tree node page.

use std::borrow::Cow;

use wasm_dbms_api::memory::{DecodeError, Encode, MemoryError, MemoryResult, Page};

use super::{
    INTERNAL_HEADER_SIZE, LEAF_HEADER_SIZE, NO_PAGE_SENTINEL, NODE_TYPE_INTERNAL, NODE_TYPE_LEAF,
    RECORD_POINTER_SIZE, RecordAddress,
};

/// Layout fields that depend on the node type.
#[derive(Debug, Clone, Copy)]
enum NodeKind {
    Internal {
        rightmost_child: Page,
    },
    Leaf {
        prev_leaf: Option<Page>,
        next_leaf: Option<Page>,
    },
}

/// Read-only view over a node page that decodes keys on demand.
///
/// Parsing walks the entry length prefixes once to record where each key
/// starts, without decoding any key. Lookups can then binary search the node
/// while decoding only `O(log n)` keys, instead of materializing every entry
/// as a full [`BTreeNode`](super::BTreeNode) does.
#[derive(Debug)]
pub(super) struct NodeView<'a> {
    buf: &'a [u8],
    kind: NodeKind,
    /// Byte offset of each entry's key, just past its two-byte length prefix.
    key_offsets: Vec<usize>,
}

impl<'a> NodeView<'a> {
    /// Parses the node stored in `buf`.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::TooShort`] when the header or an entry runs past
    /// the end of `buf`, or when the node type byte is unknown.
    pub(super) fn parse(buf: &'a [u8]) -> MemoryResult<Self> {
        if buf.len() < INTERNAL_HEADER_SIZE {
            return Err(too_short());
        }

        let num_entries = u16::from_le_bytes([buf[5], buf[6]]) as usize;
        let (kind, header_size, tail_size) = match buf[0] {
            NODE_TYPE_INTERNAL => (
                NodeKind::Internal {
                    rightmost_child: read_page(buf, 7),
                },
                INTERNAL_HEADER_SIZE,
                4,
            ),
            NODE_TYPE_LEAF if buf.len() >= LEAF_HEADER_SIZE => (
                NodeKind::Leaf {
                    prev_leaf: read_link(buf, 7),
                    next_leaf: read_link(buf, 11),
                },
                LEAF_HEADER_SIZE,
                RECORD_POINTER_SIZE,
            ),
            _ => return Err(too_short()),
        };

        let mut key_offsets = Vec::with_capacity(num_entries);
        let mut offset = header_size;
        for _ in 0..num_entries {
            if offset + 2 > buf.len() {
                return Err(too_short());
            }
            let key_size = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
            offset += 2;
            if offset + key_size + tail_size > buf.len() {
                return Err(too_short());
            }
            key_offsets.push(offset);
            offset += key_size + tail_size;
        }

        Ok(Self {
            buf,
            kind,
            key_offsets,
        })
    }

    /// Decodes the first key of the leaf stored in `buf` without parsing the node.
    ///
    /// Returns `None` for an empty leaf.
    ///
    /// # Errors
    ///
    /// Returns [`DecodeError::TooShort`] when `buf` is not a leaf page or its
    /// first entry runs past the end of `buf`, and propagates key decode errors.
    pub(super) fn first_leaf_key<K>(buf: &[u8]) -> MemoryResult<Option<K>>
    where
        K: Encode,
    {
        if buf.len() < LEAF_HEADER_SIZE || buf[0] != NODE_TYPE_LEAF {
            return Err(too_short());
        }
        if u16::from_le_bytes([buf[5], buf[6]]) == 0 {
            return Ok(None);
        }

        let start = LEAF_HEADER_SIZE + 2;
        let key_size =
            u16::from_le_bytes([buf[LEAF_HEADER_SIZE], buf[LEAF_HEADER_SIZE + 1]]) as usize;
        if start + key_size + RECORD_POINTER_SIZE > buf.len() {
            return Err(too_short());
        }
        K::decode(Cow::Borrowed(&buf[start..start + key_size])).map(Some)
    }

    /// Returns whether `buf` holds a leaf node, judging by its type byte only.
    pub(super) fn is_leaf_page(buf: &[u8]) -> bool {
        buf.first() == Some(&NODE_TYPE_LEAF)
    }

    /// Returns the number of entries in the node.
    pub(super) fn len(&self) -> usize {
        self.key_offsets.len()
    }

    /// Returns whether the node is a leaf.
    pub(super) fn is_leaf(&self) -> bool {
        matches!(self.kind, NodeKind::Leaf { .. })
    }

    /// Returns the previous leaf in the sibling chain, if any.
    pub(super) fn prev_leaf(&self) -> Option<Page> {
        match self.kind {
            NodeKind::Leaf { prev_leaf, .. } => prev_leaf,
            NodeKind::Internal { .. } => None,
        }
    }

    /// Returns the next leaf in the sibling chain, if any.
    pub(super) fn next_leaf(&self) -> Option<Page> {
        match self.kind {
            NodeKind::Leaf { next_leaf, .. } => next_leaf,
            NodeKind::Internal { .. } => None,
        }
    }

    /// Decodes the key of entry `index`.
    ///
    /// # Errors
    ///
    /// Propagates any error raised while decoding the key.
    ///
    /// # Panics
    ///
    /// Panics if `index` is not lower than [`Self::len`].
    pub(super) fn key<K>(&self, index: usize) -> MemoryResult<K>
    where
        K: Encode,
    {
        let (start, end) = self.key_range(index);
        K::decode(Cow::Borrowed(&self.buf[start..end]))
    }

    /// Returns the record pointer of leaf entry `index`.
    ///
    /// # Errors
    ///
    /// Propagates any error raised while decoding the pointer.
    ///
    /// # Panics
    ///
    /// Panics if `index` is not lower than [`Self::len`].
    pub(super) fn pointer(&self, index: usize) -> MemoryResult<RecordAddress> {
        let (_, key_end) = self.key_range(index);
        RecordAddress::decode(Cow::Borrowed(
            &self.buf[key_end..key_end + RECORD_POINTER_SIZE],
        ))
    }

    /// Returns the child page an internal node routes `key` to.
    ///
    /// Keys equal to a separator route right, matching the leaf-split
    /// promotion semantics. Returns `None` for a leaf.
    ///
    /// # Errors
    ///
    /// Propagates any error raised while decoding separator keys.
    pub(super) fn child_for<K>(&self, key: &K) -> MemoryResult<Option<Page>>
    where
        K: Encode + Ord,
    {
        let NodeKind::Internal { rightmost_child } = self.kind else {
            return Ok(None);
        };

        let index = self.partition_point(|entry: &K| entry <= key)?;
        if index < self.len() {
            let (_, key_end) = self.key_range(index);
            Ok(Some(read_page(self.buf, key_end)))
        } else {
            Ok(Some(rightmost_child))
        }
    }

    /// Returns the index of the first entry whose key does not satisfy `pred`.
    ///
    /// Entries must be partitioned by `pred`, which holds for every
    /// order-based predicate since keys are stored sorted. Decodes only
    /// `O(log n)` keys.
    ///
    /// # Errors
    ///
    /// Propagates any error raised while decoding keys.
    pub(super) fn partition_point<K, P>(&self, pred: P) -> MemoryResult<usize>
    where
        K: Encode,
        P: Fn(&K) -> bool,
    {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let mid = low + (high - low) / 2;
            if pred(&self.key::<K>(mid)?) {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low)
    }

    /// Returns the byte range of the key of entry `index`.
    fn key_range(&self, index: usize) -> (usize, usize) {
        let start = self.key_offsets[index];
        let key_size = u16::from_le_bytes([self.buf[start - 2], self.buf[start - 1]]) as usize;
        (start, start + key_size)
    }
}

fn too_short() -> MemoryError {
    MemoryError::DecodeError(DecodeError::TooShort)
}

/// Reads a little-endian page number at `offset`.
fn read_page(buf: &[u8], offset: usize) -> Page {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

/// Reads an optional sibling link at `offset`, mapping the sentinel to `None`.
fn read_link(buf: &[u8], offset: usize) -> Option<Page> {
    let page = read_page(buf, offset);
    (page != NO_PAGE_SENTINEL).then_some(page)
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{Text, Uint32};

    use super::super::{
        BTreeNode, InternalEntry, InternalNodeBody, LeafEntry, LeafNodeBody, NodeBody, NodeHeader,
    };
    use super::*;

    const PAGE_SIZE: usize = 65_536;

    fn leaf_buf(keys: &[u32], prev_leaf: Option<Page>, next_leaf: Option<Page>) -> Vec<u8> {
        let entries: Vec<LeafEntry<Uint32>> = keys
            .iter()
            .map(|&key| LeafEntry {
                key: Uint32(key),
                pointer: RecordAddress {
                    page: key + 1_000,
                    offset: 8,
                },
            })
            .collect();
        BTreeNode {
            page: 1,
            header: NodeHeader {
                parent_page: None,
                num_entries: entries.len() as u16,
            },
            body: NodeBody::Leaf(LeafNodeBody {
                entries,
                prev_leaf,
                next_leaf,
            }),
            dirty: false,
        }
        .serialize(PAGE_SIZE)
        .expect("serialize leaf failed")
    }

    fn internal_buf(separators: &[(u32, Page)], rightmost_child: Page) -> Vec<u8> {
        let entries: Vec<InternalEntry<Uint32>> = separators
            .iter()
            .map(|&(key, child_page)| InternalEntry {
                key: Uint32(key),
                child_page,
            })
            .collect();
        BTreeNode {
            page: 1,
            header: NodeHeader {
                parent_page: None,
                num_entries: entries.len() as u16,
            },
            body: NodeBody::Internal(InternalNodeBody {
                entries,
                rightmost_child,
            }),
            dirty: false,
        }
        .serialize(PAGE_SIZE)
        .expect("serialize internal failed")
    }

    #[test]
    fn test_leaf_view_exposes_entries_and_links() {
        let buf = leaf_buf(&[10, 20, 30], Some(4), None);
        let view = NodeView::parse(&buf).expect("parse failed");

        assert!(view.is_leaf());
        assert_eq!(view.len(), 3);
        assert_eq!(view.prev_leaf(), Some(4));
        assert_eq!(view.next_leaf(), None);
        assert_eq!(view.key::<Uint32>(1).expect("key"), Uint32(20));
        assert_eq!(
            view.pointer(2).expect("pointer"),
            RecordAddress {
                page: 1_030,
                offset: 8,
            }
        );
        assert_eq!(view.child_for(&Uint32(20)).expect("child_for"), None);
    }

    #[test]
    fn test_leaf_view_with_variable_size_keys() {
        let keys = ["a", "bbbbbbbb", "cc"].map(|key| Text(key.to_string()));
        let entries = keys
            .iter()
            .enumerate()
            .map(|(index, key)| LeafEntry {
                key: key.clone(),
                pointer: RecordAddress {
                    page: index as Page,
                    offset: 0,
                },
            })
            .collect();
        let buf = BTreeNode {
            page: 1,
            header: NodeHeader {
                parent_page: None,
                num_entries: 3,
            },
            body: NodeBody::Leaf(LeafNodeBody {
                entries,
                prev_leaf: None,
                next_leaf: None,
            }),
            dirty: false,
        }
        .serialize(PAGE_SIZE)
        .expect("serialize failed");

        let view = NodeView::parse(&buf).expect("parse failed");
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(&view.key::<Text>(index).expect("key"), key);
            assert_eq!(view.pointer(index).expect("pointer").page, index as Page);
        }
    }

    #[test]
    fn test_partition_point_matches_slice_partition_point() {
        let keys: Vec<u32> = (0..200).map(|key| key * 3).collect();
        let buf = leaf_buf(&keys, None, None);
        let view = NodeView::parse(&buf).expect("parse failed");

        for probe in 0..610u32 {
            let got = view
                .partition_point(|key: &Uint32| key.0 < probe)
                .expect("partition_point failed");
            assert_eq!(
                got,
                keys.partition_point(|&key| key < probe),
                "probe {probe}"
            );
        }
    }

    #[test]
    fn test_child_for_routes_equal_keys_right() {
        let buf = internal_buf(&[(10, 1), (20, 2), (30, 3)], 4);
        let view = NodeView::parse(&buf).expect("parse failed");

        assert!(!view.is_leaf());
        let route = |key: u32| view.child_for(&Uint32(key)).expect("child_for");
        assert_eq!(route(0), Some(1));
        assert_eq!(route(10), Some(2));
        assert_eq!(route(15), Some(2));
        assert_eq!(route(20), Some(3));
        assert_eq!(route(30), Some(4));
        assert_eq!(route(99), Some(4));
    }

    #[test]
    fn test_first_leaf_key_peeks_without_parsing() {
        let buf = leaf_buf(&[42, 50], None, None);
        assert_eq!(
            NodeView::first_leaf_key::<Uint32>(&buf).expect("first key"),
            Some(Uint32(42))
        );
        assert!(NodeView::is_leaf_page(&buf));

        let empty = leaf_buf(&[], Some(3), None);
        assert_eq!(
            NodeView::first_leaf_key::<Uint32>(&empty).expect("first key"),
            None
        );

        let internal = internal_buf(&[(10, 1)], 2);
        assert!(!NodeView::is_leaf_page(&internal));
        assert!(matches!(
            NodeView::first_leaf_key::<Uint32>(&internal),
            Err(MemoryError::DecodeError(DecodeError::TooShort))
        ));
    }

    #[test]
    fn test_parse_rejects_malformed_pages() {
        let too_short_error = |buf: &[u8]| {
            matches!(
                NodeView::parse(buf),
                Err(MemoryError::DecodeError(DecodeError::TooShort))
            )
        };

        assert!(too_short_error(&[NODE_TYPE_LEAF; 4]));

        let mut unknown_type = leaf_buf(&[1], None, None);
        unknown_type[0] = 7;
        assert!(too_short_error(&unknown_type));

        // Claim more entries than the page holds.
        let mut truncated = leaf_buf(&[1, 2], None, None);
        truncated.truncate(LEAF_HEADER_SIZE + 2 + 4 + RECORD_POINTER_SIZE);
        assert!(too_short_error(&truncated));
    }
}

// Rust guideline compliant 2026-03-27
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Page-backed B+ tree used by the index ledger.

mod node_view;
mod walker;

use std::borrow::Cow;
use std::marker::PhantomData;

use wasm_dbms_api::memory::{DecodeError, Encode, MemoryError, MemoryResult};
use wasm_dbms_api::prelude::Page;

use self::node_view::NodeView;
pub use self::walker::IndexTreeWalker;
use crate::{MemoryAccess, RecordAddress};

/// Node type discriminator stored in page byte 0.
const NODE_TYPE_INTERNAL: u8 = 0;
/// Node type discriminator stored in page byte 0.
const NODE_TYPE_LEAF: u8 = 1;

/// Collects every node page reachable from `root`.
///
/// Generic over no key type — the walker reads only the entry-count and
/// child-pointer fields needed to descend the tree, skipping past keys via
/// their length prefixes.
///
/// # Errors
///
/// Propagates any [`MemoryError`] surfaced while reading nodes.
pub(super) fn index_tree_pages(root: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Vec<Page>> {
    let page_size = mm.page_size() as usize;
    let mut pages: Vec<Page> = Vec::new();
    let mut stack: Vec<Page> = vec![root];
    let mut buf = vec![0u8; page_size];

    while let Some(page) = stack.pop() {
        // Read the full node page so we can iterate its entries.
        mm.read_at_raw(page, 0, &mut buf)?;
        let node_type = buf[0];
        match node_type {
            NODE_TYPE_INTERNAL => {
                let num_entries = u16::from_le_bytes(buf[5..7].try_into()?) as usize;
                let rightmost_child = u32::from_le_bytes(buf[7..11].try_into()?);
                stack.push(rightmost_child);

                let mut offset = INTERNAL_HEADER_SIZE;
                for _ in 0..num_entries {
                    let key_size = u16::from_le_bytes(buf[offset..offset + 2].try_into()?) as usize;
                    offset += 2 + key_size;
                    let child = u32::from_le_bytes(buf[offset..offset + 4].try_into()?);
                    stack.push(child);
                    offset += 4;
                }
            }
            NODE_TYPE_LEAF => {}
            other => {
                return Err(MemoryError::DecodeError(DecodeError::InvalidDiscriminant(
                    other,
                )));
            }
        }
        pages.push(page);
    }

    Ok(pages)
}

/// Walks every node page reachable from `root` and returns each page to the
/// unclaimed-pages ledger.
///
/// Used by `IndexLedger::release_pages` when a destructive op (e.g.
/// `MigrationOp::DropTable`) drops the table.
///
/// # Errors
///
/// Propagates any [`MemoryError`] surfaced while reading nodes or
/// releasing pages.
pub(super) fn release_index_tree_pages(root: Page, mm: &mut impl MemoryAccess) -> MemoryResult<()> {
    let to_release = index_tree_pages(root, mm)?;

    // Release nodes after the traversal so we never read a freshly-zeroed
    // page during descent.
    for page in to_release {
        mm.unclaim_page(page)?;
    }
    Ok(())
}
/// Sentinel page value used for missing parent/leaf links.
const NO_PAGE_SENTINEL: u32 = u32::MAX;
/// Internal node header size in bytes.
const INTERNAL_HEADER_SIZE: usize = 11;
/// Leaf node header size in bytes.
const LEAF_HEADER_SIZE: usize = 15;
/// Serialized record pointer size.
const RECORD_POINTER_SIZE: usize = 6;

/// Lightweight B+ tree handle holding the current root page.
pub struct IndexTree<K>
where
    K: Encode + Ord,
{
    root_page: Page,
    _key: PhantomData<K>,
}

#[derive(Debug, Clone)]
struct BTreeNode<K>
where
    K: Encode + Ord,
{
    page: Page,
    header: NodeHeader,
    body: NodeBody<K>,
    dirty: bool,
}

#[derive(Debug, Clone)]
struct NodeHeader {
    parent_page: Option<Page>,
    num_entries: u16,
}

#[derive(Debug, Clone)]
enum NodeBody<K>
where
    K: Encode + Ord,
{
    Internal(InternalNodeBody<K>),
    Leaf(LeafNodeBody<K>),
}

#[derive(Debug, Clone)]
struct InternalNodeBody<K>
where
    K: Encode + Ord,
{
    entries: Vec<InternalEntry<K>>,
    rightmost_child: Page,
}

#[derive(Debug, Clone)]
struct LeafNodeBody<K>
where
    K: Encode + Ord,
{
    entries: Vec<LeafEntry<K>>,
    prev_leaf: Option<Page>,
    next_leaf: Option<Page>,
}

#[derive(Debug, Clone)]
struct InternalEntry<K>
where
    K: Encode + Ord,
{
    key: K,
    child_page: Page,
}

#[derive(Debug, Clone)]
struct LeafEntry<K>
where
    K: Encode + Ord,
{
    key: K,
    pointer: RecordAddress,
}

struct Promotion<K>
where
    K: Encode + Ord,
{
    key: K,
    left_page: Page,
    right_page: Page,
}

impl<K> BTreeNode<K>
where
    K: Encode + Ord,
{
    fn new_leaf(page: Page, parent_page: Option<Page>) -> Self {
        Self {
            page,
            header: NodeHeader {
                parent_page,
                num_entries: 0,
            },
            body: NodeBody::Leaf(LeafNodeBody {
                entries: Vec::new(),
                prev_leaf: None,
                next_leaf: None,
            }),
            dirty: true,
        }
    }

    fn serialize(&self, page_size: usize) -> MemoryResult<Vec<u8>> {
        let mut buf = vec![0u8; page_size];
        match &self.body {
            NodeBody::Internal(internal) => {
                buf[0] = NODE_TYPE_INTERNAL;
                let parent = self.header.parent_page.unwrap_or(NO_PAGE_SENTINEL);
                buf[1..5].copy_from_slice(&parent.to_le_bytes());
                buf[5..7].copy_from_slice(&self.header.num_entries.to_le_bytes());
                buf[7..11].copy_from_slice(&internal.rightmost_child.to_le_bytes());

                let mut offset = INTERNAL_HEADER_SIZE;
                for entry in &internal.entries {
                    let key_bytes = entry.key.encode();
                    let entry_size =
                        Self::internal_entry_size_from_len(key_bytes.len(), page_size)?;
                    if offset + entry_size > page_size {
                        return Err(MemoryError::KeyTooLarge {
                            size: entry_size as u64,
                            max: (page_size - INTERNAL_HEADER_SIZE) as u64,
                        });
                    }

                    let key_size =
                        u16::try_from(key_bytes.len()).map_err(|_| MemoryError::KeyTooLarge {
                            size: key_bytes.len() as u64,
                            max: u16::MAX as u64,
                        })?;
                    buf[offset..offset + 2].copy_from_slice(&key_size.to_le_bytes());
                    offset += 2;
                    buf[offset..offset + key_bytes.len()].copy_from_slice(&key_bytes);
                    offset += key_bytes.len();
                    buf[offset..offset + 4].copy_from_slice(&entry.child_page.to_le_bytes());
                    offset += 4;
                }
            }
            NodeBody::Leaf(leaf) => {
                buf[0] = NODE_TYPE_LEAF;
                let parent = self.header.parent_page.unwrap_or(NO_PAGE_SENTINEL);
                let prev = leaf.prev_leaf.unwrap_or(NO_PAGE_SENTINEL);
                let next = leaf.next_leaf.unwrap_or(NO_PAGE_SENTINEL);
                buf[1..5].copy_from_slice(&parent.to_le_bytes());
                buf[5..7].copy_from_slice(&self.header.num_entries.to_le_bytes());
                buf[7..11].copy_from_slice(&prev.to_le_bytes());
                buf[11..15].copy_from_slice(&next.to_le_bytes());

                let mut offset = LEAF_HEADER_SIZE;
                for entry in &leaf.entries {
                    let key_bytes = entry.key.encode();
                    let entry_size = Self::leaf_entry_size_from_len(key_bytes.len(), page_size)?;
                    if offset + entry_size > page_size {
                        return Err(MemoryError::KeyTooLarge {
                            size: entry_size as u64,
                            max: (page_size - LEAF_HEADER_SIZE) as u64,
                        });
                    }

                    let key_size =
                        u16::try_from(key_bytes.len()).map_err(|_| MemoryError::KeyTooLarge {
                            size: key_bytes.len() as u64,
                            max: u16::MAX as u64,
                        })?;
                    buf[offset..offset + 2].copy_from_slice(&key_size.to_le_bytes());
                    offset += 2;
                    buf[offset..offset + key_bytes.len()].copy_from_slice(&key_bytes);
                    offset += key_bytes.len();
                    let pointer_bytes = entry.pointer.encode();
                    buf[offset..offset + RECORD_POINTER_SIZE].copy_from_slice(&pointer_bytes);
                    offset += RECORD_POINTER_SIZE;
                }
            }
        }
        Ok(buf)
    }

    fn deserialize(page: Page, buf: &[u8]) -> MemoryResult<Self> {
        if buf.len() < INTERNAL_HEADER_SIZE {
            return Err(MemoryError::DecodeError(DecodeError::TooShort));
        }

        let node_type = buf[0];
        let parent_raw = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
        let parent_page = (parent_raw != NO_PAGE_SENTINEL).then_some(parent_raw);
        let num_entries = u16::from_le_bytes([buf[5], buf[6]]);

        match node_type {
            NODE_TYPE_INTERNAL => {
                let rightmost_child = u32::from_le_bytes([buf[7], buf[8], buf[9], buf[10]]);
                let mut offset = INTERNAL_HEADER_SIZE;
                let mut entries = Vec::with_capacity(num_entries as usize);
                for _ in 0..num_entries {
                    if offset + 2 > buf.len() {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }
                    let key_size = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
                    offset += 2;
                    if offset + key_size + 4 > buf.len() {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }

                    let key = K::decode(Cow::Borrowed(&buf[offset..offset + key_size]))?;
                    offset += key_size;
                    let child_page = u32::from_le_bytes([
                        buf[offset],
                        buf[offset + 1],
                        buf[offset + 2],
                        buf[offset + 3],
                    ]);
                    offset += 4;
                    entries.push(InternalEntry { key, child_page });
                }

                Ok(Self {
                    page,
                    header: NodeHeader {
                        parent_page,
                        num_entries,
                    },
                    body: NodeBody::Internal(InternalNodeBody {
                        entries,
                        rightmost_child,
                    }),
                    dirty: false,
                })
            }
            NODE_TYPE_LEAF => {
                if buf.len() < LEAF_HEADER_SIZE {
                    return Err(MemoryError::DecodeError(DecodeError::TooShort));
                }

                let prev_raw = u32::from_le_bytes([buf[7], buf[8], buf[9], buf[10]]);
                let next_raw = u32::from_le_bytes([buf[11], buf[12], buf[13], buf[14]]);
                let mut offset = LEAF_HEADER_SIZE;
                let mut entries = Vec::with_capacity(num_entries as usize);
                for _ in 0..num_entries {
                    if offset + 2 > buf.len() {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }
                    let key_size = u16::from_le_bytes([buf[offset], buf[offset + 1]]) as usize;
                    offset += 2;
                    if offset + key_size + RECORD_POINTER_SIZE > buf.len() {
                        return Err(MemoryError::DecodeError(DecodeError::TooShort));
                    }

                    let key = K::decode(Cow::Borrowed(&buf[offset..offset + key_size]))?;
                    offset += key_size;
                    let pointer = RecordAddress::decode(Cow::Borrowed(
                        &buf[offset..offset + RECORD_POINTER_SIZE],
                    ))?;
                    offset += RECORD_POINTER_SIZE;
                    entries.push(LeafEntry { key, pointer });
                }

                Ok(Self {
                    page,
                    header: NodeHeader {
                        parent_page,
                        num_entries,
                    },
                    body: NodeBody::Leaf(LeafNodeBody {
                        entries,
                        prev_leaf: (prev_raw != NO_PAGE_SENTINEL).then_some(prev_raw),
                        next_leaf: (next_raw != NO_PAGE_SENTINEL).then_some(next_raw),
                    }),
                    dirty: false,
                })
            }
            _ => Err(MemoryError::DecodeError(DecodeError::TooShort)),
        }
    }

    fn read(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        let mut buf = vec![0u8; mm.page_size() as usize];
        mm.read_at_raw(page, 0, &mut buf)?;
        Self::deserialize(page, &buf)
    }

    fn flush(&mut self, mm: &mut impl MemoryAccess) -> MemoryResult<()> {
        if !self.dirty {
            return Ok(());
        }

        let buf = self.serialize(mm.page_size() as usize)?;
        mm.write_at_raw(self.page, 0, &buf)?;
        self.dirty = false;
        Ok(())
    }

    fn entries_byte_size(&self, page_size: usize) -> MemoryResult<usize> {
        match &self.body {
            NodeBody::Internal(internal) => {
                internal.entries.iter().try_fold(0usize, |acc, entry| {
                    Ok(acc
                        + Self::internal_entry_size_from_len(entry.key.encode().len(), page_size)?)
                })
            }
            NodeBody::Leaf(leaf) => leaf.entries.iter().try_fold(0usize, |acc, entry| {
                Ok(acc + Self::leaf_entry_size_from_len(entry.key.encode().len(), page_size)?)
            }),
        }
    }

    fn would_overflow(&self, page_size: usize) -> MemoryResult<bool> {
        let header_size = match self.body {
            NodeBody::Internal(_) => INTERNAL_HEADER_SIZE,
            NodeBody::Leaf(_) => LEAF_HEADER_SIZE,
        };
        Ok(header_size + self.entries_byte_size(page_size)? > page_size)
    }

    fn first_key(&self) -> Option<&K> {
        match &self.body {
            NodeBody::Internal(internal) => internal.entries.first().map(|entry| &entry.key),
            NodeBody::Leaf(leaf) => leaf.entries.first().map(|entry| &entry.key),
        }
    }

    fn leaf_entry_size_from_len(key_len: usize, page_size: usize) -> MemoryResult<usize> {
        if key_len > u16::MAX as usize {
            return Err(MemoryError::KeyTooLarge {
                size: key_len as u64,
                max: u16::MAX as u64,
            });
        }

        let entry_size = 2 + key_len + RECORD_POINTER_SIZE;
        if LEAF_HEADER_SIZE + entry_size > page_size {
            return Err(MemoryError::KeyTooLarge {
                size: entry_size as u64,
                max: (page_size - LEAF_HEADER_SIZE) as u64,
            });
        }
        Ok(entry_size)
    }

    fn internal_entry_size_from_len(key_len: usize, page_size: usize) -> MemoryResult<usize> {
        if key_len > u16::MAX as usize {
            return Err(MemoryError::KeyTooLarge {
                size: key_len as u64,
                max: u16::MAX as u64,
            });
        }

        let entry_size = 2 + key_len + 4;
        if INTERNAL_HEADER_SIZE + entry_size > page_size {
            return Err(MemoryError::KeyTooLarge {
                size: entry_size as u64,
                max: (page_size - INTERNAL_HEADER_SIZE) as u64,
            });
        }
        Ok(entry_size)
    }
}

impl<K> IndexTree<K>
where
    K: Encode + Ord,
{
    /// Initializes a new empty tree with a single root leaf page.
    pub fn init(mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        let root_page = mm.claim_page()?;
        let mut root = BTreeNode::<K>::new_leaf(root_page, None);
        root.flush(mm)?;
        Ok(Self::load(root_page))
    }

    /// Loads an existing tree from its root page.
    pub fn load(root_page: Page) -> Self {
        Self {
            root_page,
            _key: PhantomData,
        }
    }

    /// Returns the current root page.
    pub const fn root_page(&self) -> Page {
        self.root_page
    }

    /// Inserts a key-pointer pair into the tree.
    pub fn insert(
        &mut self,
        key: K,
        pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        let page_size = mm.page_size() as usize;
        let mut path = self.read_path(&key, mm)?;
        let leaf = path
            .last_mut()
            .expect("read_path always returns at least the root");

        if let NodeBody::Leaf(leaf_body) = &mut leaf.body {
            let position = leaf_body.entries.partition_point(|entry| {
                entry.key < key || (entry.key == key && entry.pointer < pointer)
            });
            leaf_body
                .entries
                .insert(position, LeafEntry { key, pointer });
            leaf.header.num_entries = leaf_body.entries.len() as u16;
            leaf.dirty = true;
        }

        let mut level = path.len();
        while level > 0 {
            let current_index = level - 1;
            if !path[current_index].would_overflow(page_size)? {
                level -= 1;
                continue;
            }

            let promotion = self.split_node(&mut path, current_index, mm)?;
            if let Some(parent) = current_index
                .checked_sub(1)
                .and_then(|idx| path.get_mut(idx))
            {
                Self::promote_into_internal(parent, promotion);
            } else {
                let new_root_page = mm.claim_page()?;
                path[current_index].header.parent_page = Some(new_root_page);
                path[current_index].dirty = true;

                let mut new_root = BTreeNode {
                    page: new_root_page,
                    header: NodeHeader {
                        parent_page: None,
                        num_entries: 1,
                    },
                    body: NodeBody::Internal(InternalNodeBody {
                        entries: vec![InternalEntry {
                            key: promotion.key,
                            child_page: promotion.left_page,
                        }],
                        rightmost_child: promotion.right_page,
                    }),
                    dirty: true,
                };
                new_root.flush(mm)?;
                self.root_page = new_root_page;
            }

            level -= 1;
        }

        for node in &mut path {
            node.flush(mm)?;
        }

        Ok(())
    }

    /// Looks up all pointers matching `key`.
    ///
    /// Nodes are read as [`NodeView`]s, so only the keys visited by binary
    /// search and the matching entries are decoded.
    pub fn search(&self, key: &K, mm: &mut impl MemoryAccess) -> MemoryResult<Vec<RecordAddress>> {
        self.search_limited(key, usize::MAX, mm)
            .map(Option::unwrap_or_default)
    }

    /// Looks up the pointers matching `key`, or returns `None` as soon as
    /// more than `limit` entries match.
    ///
    /// Reads like [`Self::search`], but stops early, so a dense key costs at
    /// most `limit + 1` decoded entries.
    pub fn search_limited(
        &self,
        key: &K,
        limit: usize,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Option<Vec<RecordAddress>>> {
        let mut buf = vec![0u8; mm.page_size() as usize];
        self.find_search_start_page(key, &mut buf, mm)?;
        let mut results = Vec::new();

        loop {
            let next_page = {
                let view = NodeView::parse(&buf)?;
                if !view.is_leaf() {
                    return Ok(Some(results));
                }
                let start = view.partition_point(|entry: &K| entry < key)?;
                for index in start..view.len() {
                    if view.key::<K>(index)? != *key {
                        return Ok(Some(results));
                    }
                    if results.len() == limit {
                        return Ok(None);
                    }
                    results.push(view.pointer(index)?);
                }
                view.next_leaf()
            };

            match next_page {
                Some(page) => {
                    mm.read_at_raw(page, 0, &mut buf)?;
                }
                None => return Ok(Some(results)),
            }
        }
    }

    /// Deletes a specific key-pointer pair.
    pub fn delete(
        &mut self,
        key: &K,
        pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        let mut leaf = self.find_search_start_leaf(key, mm)?;

        loop {
            let (position, next_page) = match &leaf.body {
                NodeBody::Leaf(leaf_body) => (
                    leaf_body
                        .entries
                        .iter()
                        .position(|entry| entry.key == *key && entry.pointer == pointer),
                    leaf_body.next_leaf,
                ),
                NodeBody::Internal(_) => return Err(MemoryError::EntryNotFound),
            };

            if let Some(index) = position {
                // Emptied leaves stay linked into the sibling chain: their parent
                // still routes keys to them, so unlinking would hide later
                // reinserts from range scans.
                if let NodeBody::Leaf(leaf_body) = &mut leaf.body {
                    leaf_body.entries.remove(index);
                    leaf.header.num_entries = leaf_body.entries.len() as u16;
                    leaf.dirty = true;
                }

                leaf.flush(mm)?;
                return Ok(());
            }

            match next_page {
                Some(page) => {
                    let next_leaf = BTreeNode::<K>::read(page, mm)?;
                    if next_leaf.first_key().is_some_and(|next_key| next_key > key) {
                        return Err(MemoryError::EntryNotFound);
                    }
                    leaf = next_leaf;
                }
                None => return Err(MemoryError::EntryNotFound),
            }
        }
    }

    /// Updates an existing key-pointer pair.
    pub fn update(
        &mut self,
        key: &K,
        old_pointer: RecordAddress,
        new_pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        self.delete(key, old_pointer, mm)?;
        self.insert(key.clone(), new_pointer, mm)
    }

    /// Opens a forward cursor returning entries in `[start_key, end_key)`.
    pub fn range_scan(
        &self,
        start_key: &K,
        end_key: Option<&K>,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<IndexTreeWalker<K>> {
        let leaf = self.find_search_start_leaf(start_key, mm)?;
        match leaf.body {
            NodeBody::Leaf(leaf_body) => {
                let cursor = leaf_body
                    .entries
                    .partition_point(|entry| entry.key < *start_key);
                Ok(IndexTreeWalker::new(
                    leaf_body.entries,
                    cursor,
                    leaf_body.next_leaf,
                    end_key.cloned(),
                ))
            }
            NodeBody::Internal(_) => unreachable!("find_search_start_leaf always returns a leaf"),
        }
    }

    fn read_path(&self, key: &K, mm: &mut impl MemoryAccess) -> MemoryResult<Vec<BTreeNode<K>>> {
        let mut page = self.root_page;
        let mut path = Vec::new();
        loop {
            let node = BTreeNode::<K>::read(page, mm)?;
            let next_page = match &node.body {
                NodeBody::Internal(internal) => Some(Self::find_child(internal, key)),
                NodeBody::Leaf(_) => None,
            };
            path.push(node);

            if let Some(next) = next_page {
                page = next;
            } else {
                return Ok(path);
            }
        }
    }

    fn find_search_start_leaf(
        &self,
        key: &K,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<BTreeNode<K>> {
        let mut buf = vec![0u8; mm.page_size() as usize];
        let page = self.find_search_start_page(key, &mut buf, mm)?;
        BTreeNode::deserialize(page, &buf)
    }

    /// Finds the leftmost leaf that may hold `key` and leaves its bytes in `buf`.
    ///
    /// Duplicates of `key` may continue into earlier leaves, and emptied
    /// leaves stay in the sibling chain, so the previous leaves are checked
    /// whenever the routed leaf does not start strictly below `key`.
    fn find_search_start_page(
        &self,
        key: &K,
        buf: &mut [u8],
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Page> {
        let mut page = self.find_leaf_page_into(key, buf, mm)?;

        loop {
            if NodeView::first_leaf_key::<K>(buf)?.is_some_and(|first| first < *key) {
                return Ok(page);
            }
            let prev_page = NodeView::parse(buf)?.prev_leaf();

            // Skip emptied leaves while looking for an earlier leaf that may
            // still hold `key`.
            let mut candidate = prev_page;
            let found = loop {
                let Some(prev) = candidate else {
                    break None;
                };
                mm.read_at_raw(prev, 0, buf)?;
                let view = NodeView::parse(buf)?;
                match view.len().checked_sub(1) {
                    Some(last) if view.key::<K>(last)? >= *key => break Some(prev),
                    Some(_) => break None,
                    None => candidate = view.prev_leaf(),
                }
            };

            match found {
                Some(prev) => page = prev,
                None => {
                    if prev_page.is_some() {
                        mm.read_at_raw(page, 0, buf)?;
                    }
                    return Ok(page);
                }
            }
        }
    }

    #[cfg(test)]
    fn find_leaf_page(&self, key: &K, mm: &mut impl MemoryAccess) -> MemoryResult<Page> {
        let mut buf = vec![0u8; mm.page_size() as usize];
        self.find_leaf_page_into(key, &mut buf, mm)
    }

    /// Descends from the root to the leaf `key` routes to, leaving its bytes in `buf`.
    fn find_leaf_page_into(
        &self,
        key: &K,
        buf: &mut [u8],
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Page> {
        let mut page = self.root_page;
        loop {
            mm.read_at_raw(page, 0, buf)?;
            if NodeView::is_leaf_page(buf) {
                return Ok(page);
            }
            match NodeView::parse(buf)?.child_for(key)? {
                Some(child) => page = child,
                None => return Ok(page),
            }
        }
    }

    fn find_child(internal: &InternalNodeBody<K>, key: &K) -> Page {
        // Binary search: find the first entry whose key is strictly greater than
        // the search key. Keys equal to the search key route right (toward
        // `rightmost_child`), matching the leaf-split promotion semantics.
        let i = internal.entries.partition_point(|entry| entry.key <= *key);
        if i < internal.entries.len() {
            internal.entries[i].child_page
        } else {
            internal.rightmost_child
        }
    }

    fn split_node(
        &self,
        path: &mut [BTreeNode<K>],
        index: usize,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Promotion<K>> {
        let page_size = mm.page_size() as usize;
        let node = &mut path[index];
        match &mut node.body {
            NodeBody::Leaf(leaf) => {
                let sizes = leaf
                    .entries
                    .iter()
                    .map(|entry| {
                        BTreeNode::<K>::leaf_entry_size_from_len(
                            entry.key.encode().len(),
                            page_size,
                        )
                    })
                    .collect::<MemoryResult<Vec<_>>>()?;
                let split_at =
                    byte_balanced_split_index(&sizes, LEAF_HEADER_SIZE, page_size, false)
                        .ok_or_else(|| MemoryError::KeyTooLarge {
                            size: sizes.iter().copied().max().unwrap_or(0) as u64,
                            max: (page_size - LEAF_HEADER_SIZE) as u64,
                        })?;
                let right_entries = leaf.entries.split_off(split_at);
                let promote_key = right_entries
                    .first()
                    .map(|entry| entry.key.clone())
                    .expect("overflowing leaf must contain entries");
                let right_page = mm.claim_page()?;

                let old_next = leaf.next_leaf;
                let mut right_leaf = BTreeNode {
                    page: right_page,
                    header: NodeHeader {
                        parent_page: node.header.parent_page,
                        num_entries: right_entries.len() as u16,
                    },
                    body: NodeBody::Leaf(LeafNodeBody {
                        entries: right_entries,
                        prev_leaf: Some(node.page),
                        next_leaf: old_next,
                    }),
                    dirty: true,
                };

                if let Some(next_page) = old_next {
                    let mut next_leaf = BTreeNode::<K>::read(next_page, mm)?;
                    if let NodeBody::Leaf(next_body) = &mut next_leaf.body {
                        next_body.prev_leaf = Some(right_page);
                        next_leaf.dirty = true;
                        next_leaf.flush(mm)?;
                    }
                }

                leaf.next_leaf = Some(right_page);
                node.header.num_entries = leaf.entries.len() as u16;
                node.dirty = true;

                right_leaf.flush(mm)?;
                Ok(Promotion {
                    key: promote_key,
                    left_page: node.page,
                    right_page,
                })
            }
            NodeBody::Internal(internal) => {
                let sizes = internal
                    .entries
                    .iter()
                    .map(|entry| {
                        BTreeNode::<K>::internal_entry_size_from_len(
                            entry.key.encode().len(),
                            page_size,
                        )
                    })
                    .collect::<MemoryResult<Vec<_>>>()?;
                let mid = byte_balanced_split_index(&sizes, INTERNAL_HEADER_SIZE, page_size, true)
                    .ok_or_else(|| MemoryError::KeyTooLarge {
                        size: sizes.iter().copied().max().unwrap_or(0) as u64,
                        max: (page_size - INTERNAL_HEADER_SIZE) as u64,
                    })?;
                let median = internal.entries.remove(mid);
                let left_rightmost_child = median.child_page;
                let right_entries = internal.entries.split_off(mid);
                let old_rightmost_child = internal.rightmost_child;
                internal.rightmost_child = left_rightmost_child;

                let right_page = mm.claim_page()?;
                let mut right_node = BTreeNode {
                    page: right_page,
                    header: NodeHeader {
                        parent_page: node.header.parent_page,
                        num_entries: right_entries.len() as u16,
                    },
                    body: NodeBody::Internal(InternalNodeBody {
                        entries: right_entries,
                        rightmost_child: old_rightmost_child,
                    }),
                    dirty: true,
                };

                node.header.num_entries = internal.entries.len() as u16;
                node.dirty = true;
                right_node.flush(mm)?;

                Ok(Promotion {
                    key: median.key,
                    left_page: node.page,
                    right_page,
                })
            }
        }
    }

    fn promote_into_internal(parent: &mut BTreeNode<K>, promotion: Promotion<K>) {
        let NodeBody::Internal(internal) = &mut parent.body else {
            unreachable!("parent of a split node must be internal");
        };

        if internal.rightmost_child == promotion.left_page {
            internal.entries.push(InternalEntry {
                key: promotion.key,
                child_page: promotion.left_page,
            });
            internal.rightmost_child = promotion.right_page;
        } else if let Some(position) = internal
            .entries
            .iter()
            .position(|entry| entry.child_page == promotion.left_page)
        {
            internal.entries.insert(
                position,
                InternalEntry {
                    key: promotion.key,
                    child_page: promotion.left_page,
                },
            );
            internal.entries[position + 1].child_page = promotion.right_page;
        } else {
            unreachable!("split child must be referenced by its parent");
        }

        parent.header.num_entries = internal.entries.len() as u16;
        parent.dirty = true;
    }
}

/// Picks where to split an overflowing node, using encoded entry sizes.
///
/// `entry_sizes` holds the serialized size of every entry. For leaves
/// (`promote_median == false`) the entry at the returned index becomes the
/// first entry of the right node; for internal nodes it is promoted to the
/// parent and belongs to neither half.
///
/// Returns the index that keeps both halves non-empty and within
/// `page_size - header_size` bytes while balancing their byte sizes, or
/// `None` if no such index exists.
fn byte_balanced_split_index(
    entry_sizes: &[usize],
    header_size: usize,
    page_size: usize,
    promote_median: bool,
) -> Option<usize> {
    let capacity = page_size.checked_sub(header_size)?;
    let total: usize = entry_sizes.iter().sum();
    let mut left = 0usize;
    let mut best: Option<(usize, usize)> = None;
    for (index, &size) in entry_sizes.iter().enumerate() {
        let right = if promote_median {
            total - left - size
        } else {
            total - left
        };
        let right_non_empty = !promote_median || index + 1 < entry_sizes.len();
        if index > 0 && right_non_empty && left <= capacity && right <= capacity {
            let imbalance = left.abs_diff(right);
            if best.is_none_or(|(_, best_imbalance)| imbalance < best_imbalance) {
                best = Some((index, imbalance));
            }
        }
        left += size;
    }
    best.map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::{Text, Uint32};

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    fn make_mm() -> MemoryManager<HeapMemoryProvider> {
        MemoryManager::init(HeapMemoryProvider::default())
    }

    fn padded_text_key(index: usize, payload_len: usize) -> Text {
        let prefix = format!("{index:05}:");
        let fill_len = payload_len.saturating_sub(prefix.len());
        Text(format!("{prefix}{}", "x".repeat(fill_len)))
    }

    #[test]
    fn test_serialize_deserialize_leaf_with_entries() {
        let node = BTreeNode {
            page: 10,
            header: NodeHeader {
                parent_page: Some(2),
                num_entries: 2,
            },
            body: NodeBody::Leaf(LeafNodeBody {
                entries: vec![
                    LeafEntry {
                        key: Uint32(10),
                        pointer: RecordAddress {
                            page: 100,
                            offset: 32,
                        },
                    },
                    LeafEntry {
                        key: Uint32(20),
                        pointer: RecordAddress {
                            page: 101,
                            offset: 64,
                        },
                    },
                ],
                prev_leaf: Some(9),
                next_leaf: Some(11),
            }),
            dirty: false,
        };

        let buf = node.serialize(65_536).expect("serialize leaf failed");
        let deserialized =
            BTreeNode::<Uint32>::deserialize(10, &buf).expect("deserialize leaf failed");
        match deserialized.body {
            NodeBody::Leaf(leaf) => {
                assert_eq!(leaf.entries.len(), 2);
                assert_eq!(leaf.entries[0].key, Uint32(10));
                assert_eq!(leaf.entries[1].key, Uint32(20));
                assert_eq!(leaf.prev_leaf, Some(9));
                assert_eq!(leaf.next_leaf, Some(11));
            }
            NodeBody::Internal(_) => panic!("expected leaf node"),
        }
    }

    #[test]
    fn test_serialize_deserialize_internal_node() {
        let node = BTreeNode {
            page: 3,
            header: NodeHeader {
                parent_page: None,
                num_entries: 2,
            },
            body: NodeBody::Internal(InternalNodeBody {
                entries: vec![
                    InternalEntry {
                        key: Uint32(50),
                        child_page: 4,
                    },
                    InternalEntry {
                        key: Uint32(100),
                        child_page: 5,
                    },
                ],
                rightmost_child: 6,
            }),
            dirty: false,
        };

        let buf = node.serialize(65_536).expect("serialize internal failed");
        let deserialized =
            BTreeNode::<Uint32>::deserialize(3, &buf).expect("deserialize internal failed");
        match deserialized.body {
            NodeBody::Internal(internal) => {
                assert_eq!(internal.entries.len(), 2);
                assert_eq!(internal.entries[0].key, Uint32(50));
                assert_eq!(internal.entries[0].child_page, 4);
                assert_eq!(internal.entries[1].key, Uint32(100));
                assert_eq!(internal.entries[1].child_page, 5);
                assert_eq!(internal.rightmost_child, 6);
            }
            NodeBody::Leaf(_) => panic!("expected internal node"),
        }
    }

    #[test]
    fn test_key_too_large_for_leaf_is_rejected() {
        let node = BTreeNode {
            page: 1,
            header: NodeHeader {
                parent_page: None,
                num_entries: 1,
            },
            body: NodeBody::Leaf(LeafNodeBody {
                entries: vec![LeafEntry {
                    key: Text("x".repeat(65_520)),
                    pointer: RecordAddress { page: 1, offset: 0 },
                }],
                prev_leaf: None,
                next_leaf: None,
            }),
            dirty: false,
        };

        let error = node.serialize(65_536).expect_err("oversized key must fail");
        assert!(matches!(error, MemoryError::KeyTooLarge { .. }));
    }

    #[test]
    fn test_index_tree_init_creates_empty_root_leaf() {
        let mut mm = make_mm();
        let tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let root = BTreeNode::<Uint32>::read(tree.root_page(), &mut mm).expect("root read failed");
        match root.body {
            NodeBody::Leaf(leaf) => assert!(leaf.entries.is_empty()),
            NodeBody::Internal(_) => panic!("expected leaf root"),
        }
    }

    #[test]
    fn test_insert_and_search_small_sorted_workload() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for value in (0..64u32).rev() {
            tree.insert(
                Uint32(value),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("insert failed");
        }

        for value in 0..64u32 {
            let hits = tree.search(&Uint32(value), &mut mm).expect("search failed");
            assert_eq!(
                hits,
                vec![RecordAddress {
                    page: value,
                    offset: 0
                }]
            );
        }
    }

    #[test]
    fn test_search_returns_empty_for_missing_key() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        tree.insert(Uint32(42), RecordAddress { page: 9, offset: 1 }, &mut mm)
            .expect("insert failed");

        let hits = tree.search(&Uint32(99), &mut mm).expect("search failed");
        assert!(hits.is_empty());
    }

    #[test]
    fn test_duplicate_keys_across_leaf_splits_remain_searchable() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let duplicate_count = 6_000u32;

        for value in 0..duplicate_count {
            tree.insert(
                Uint32(7),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("duplicate insert failed");
        }

        let hits = tree
            .search(&Uint32(7), &mut mm)
            .expect("duplicate search failed");
        assert_eq!(hits.len(), duplicate_count as usize);
        assert_eq!(hits.first(), Some(&RecordAddress { page: 0, offset: 0 }));
        assert_eq!(
            hits.last(),
            Some(&RecordAddress {
                page: duplicate_count - 1,
                offset: 0,
            })
        );
    }

    #[test]
    fn test_dynamic_keys_force_multiple_levels_and_still_search() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Text>::init(&mut mm).expect("tree init failed");

        for index in 0..140usize {
            tree.insert(
                padded_text_key(index, 7_000),
                RecordAddress {
                    page: index as u32,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("text insert failed");
        }

        for index in [0usize, 1, 7, 32, 89, 139] {
            let key = padded_text_key(index, 7_000);
            let hits = tree.search(&key, &mut mm).expect("text search failed");
            assert_eq!(
                hits,
                vec![RecordAddress {
                    page: index as u32,
                    offset: 0,
                }]
            );
        }

        let root = BTreeNode::<Text>::read(tree.root_page(), &mut mm).expect("root read failed");
        assert!(matches!(root.body, NodeBody::Internal(_)));
    }

    #[test]
    fn test_leaf_split_uses_encoded_size_for_skewed_keys() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Text>::init(&mut mm).expect("tree init failed");

        for n in 0..8u32 {
            tree.insert(
                Text(format!("{n}:{}", "x".repeat(3_000))),
                RecordAddress { page: n, offset: 0 },
                &mut mm,
            )
            .expect("small key insert failed");
        }
        let big_key = Text("z".repeat(60_000));
        let big_address = RecordAddress {
            page: 99,
            offset: 0,
        };
        tree.insert(big_key.clone(), big_address, &mut mm)
            .expect("big key fits a leaf on its own");

        assert_eq!(
            tree.search(&big_key, &mut mm).expect("search big"),
            vec![big_address]
        );
        for n in 0..8u32 {
            let key = Text(format!("{n}:{}", "x".repeat(3_000)));
            assert_eq!(
                tree.search(&key, &mut mm).expect("search small"),
                vec![RecordAddress { page: n, offset: 0 }]
            );
        }
    }

    #[test]
    fn test_internal_split_uses_encoded_size_for_skewed_keys() {
        let mut mm = make_mm();
        let tree = IndexTree::<Text>::init(&mut mm).expect("tree init failed");
        let page = mm.claim_page().expect("claim internal page");

        let mut entries: Vec<InternalEntry<Text>> = (0..8usize)
            .map(|index| InternalEntry {
                key: padded_text_key(index, 3_000),
                child_page: 100 + index as u32,
            })
            .collect();
        entries.push(InternalEntry {
            key: Text("z".repeat(60_000)),
            child_page: 200,
        });
        let mut path = vec![BTreeNode {
            page,
            header: NodeHeader {
                parent_page: None,
                num_entries: entries.len() as u16,
            },
            body: NodeBody::Internal(InternalNodeBody {
                entries,
                rightmost_child: 201,
            }),
            dirty: true,
        }];

        let promotion = tree
            .split_node(&mut path, 0, &mut mm)
            .expect("size-aware internal split");

        // Only splitting before the big key keeps both halves on one page.
        assert_eq!(promotion.key, padded_text_key(7, 3_000));
        let NodeBody::Internal(left) = &path[0].body else {
            panic!("left half must stay internal");
        };
        assert_eq!(left.entries.len(), 7);
        assert_eq!(left.rightmost_child, 107);
        let right = BTreeNode::<Text>::read(promotion.right_page, &mut mm).expect("read right");
        let NodeBody::Internal(right_body) = right.body else {
            panic!("right half must be internal");
        };
        assert_eq!(right_body.entries.len(), 1);
        assert_eq!(right_body.rightmost_child, 201);
    }

    #[test]
    fn test_byte_balanced_split_index_returns_none_when_no_partition_fits() {
        // Two entries of 40 bytes each cannot be split with a 30-byte budget.
        assert_eq!(byte_balanced_split_index(&[40, 40], 0, 30, false), None);
        // Promoting the median from three entries leaves 60 + 60 > 50.
        assert_eq!(byte_balanced_split_index(&[60, 10, 60], 0, 50, true), None);
    }

    #[test]
    fn test_delete_single_entry() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let ptr = RecordAddress {
            page: 10,
            offset: 20,
        };

        tree.insert(Uint32(42), ptr, &mut mm)
            .expect("insert failed");
        tree.delete(&Uint32(42), ptr, &mut mm)
            .expect("delete failed");

        let hits = tree.search(&Uint32(42), &mut mm).expect("search failed");
        assert!(hits.is_empty());
    }

    #[test]
    fn test_delete_one_of_many_duplicate_entries() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let count = 6_000u32;

        for value in 0..count {
            tree.insert(
                Uint32(11),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("insert duplicate failed");
        }

        let removed = RecordAddress {
            page: count / 2,
            offset: 0,
        };
        tree.delete(&Uint32(11), removed, &mut mm)
            .expect("delete duplicate failed");

        let hits = tree.search(&Uint32(11), &mut mm).expect("search failed");
        assert_eq!(hits.len(), count as usize - 1);
        assert!(!hits.contains(&removed));
    }

    #[test]
    fn test_delete_missing_entry_returns_error() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let error = tree
            .delete(
                &Uint32(1),
                RecordAddress {
                    page: 99,
                    offset: 0,
                },
                &mut mm,
            )
            .expect_err("missing delete must fail");
        assert!(matches!(error, MemoryError::EntryNotFound));
    }

    #[test]
    fn test_update_replaces_pointer() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let old = RecordAddress {
            page: 10,
            offset: 20,
        };
        let new = RecordAddress {
            page: 50,
            offset: 60,
        };

        tree.insert(Uint32(42), old.clone(), &mut mm)
            .expect("insert failed");
        tree.update(&Uint32(42), old, new.clone(), &mut mm)
            .expect("update failed");

        let hits = tree.search(&Uint32(42), &mut mm).expect("search failed");
        assert_eq!(hits, vec![new]);
    }

    #[test]
    fn test_range_scan_returns_expected_window() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for value in 0..100u32 {
            tree.insert(
                Uint32(value),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("insert failed");
        }

        let mut walker = tree
            .range_scan(&Uint32(20), Some(&Uint32(30)), &mut mm)
            .expect("range scan failed");

        for expected in 20..30u32 {
            let next = walker.next(&mut mm).expect("walker next failed");
            assert_eq!(
                next,
                Some(RecordAddress {
                    page: expected,
                    offset: 0,
                })
            );
        }
        assert_eq!(
            walker.next(&mut mm).expect("walker exhaustion failed"),
            None
        );
    }

    #[test]
    fn test_find_child_binary_search_routes_correctly() {
        // Build an internal node manually and verify find_child picks the right page.
        let internal = InternalNodeBody {
            entries: vec![
                InternalEntry {
                    key: Uint32(10),
                    child_page: 100,
                },
                InternalEntry {
                    key: Uint32(20),
                    child_page: 200,
                },
                InternalEntry {
                    key: Uint32(30),
                    child_page: 300,
                },
            ],
            rightmost_child: 400,
        };

        // Keys strictly less than the first entry route to entries[0].child_page
        assert_eq!(IndexTree::find_child(&internal, &Uint32(5)), 100);
        // Keys equal to an entry route right (past that entry)
        assert_eq!(IndexTree::find_child(&internal, &Uint32(10)), 200);
        // Keys between entries route to the next entry's child_page
        assert_eq!(IndexTree::find_child(&internal, &Uint32(15)), 200);
        assert_eq!(IndexTree::find_child(&internal, &Uint32(20)), 300);
        assert_eq!(IndexTree::find_child(&internal, &Uint32(25)), 300);
        // Keys equal to last entry route to rightmost_child
        assert_eq!(IndexTree::find_child(&internal, &Uint32(30)), 400);
        // Keys greater than all entries route to rightmost_child
        assert_eq!(IndexTree::find_child(&internal, &Uint32(99)), 400);
    }

    #[test]
    fn test_cascading_internal_splits() {
        // Force enough data to create 3+ tree levels, triggering cascading
        // internal node splits. With 65536-byte pages and 7000-byte text keys,
        // each leaf holds ~8 entries and each internal node holds ~9 entries.
        // With 9 internal entries per node, a second-level split occurs around
        // 9 * 8 = 72 entries. We need more than 9 internal children to force
        // the internal level to split, which requires > 72 leaf nodes, which
        // requires > 72 * 8 ≈ 576 entries. Use 800 to be safe and ensure
        // at least one cascading internal split.
        let mut mm = make_mm();
        let mut tree = IndexTree::<Text>::init(&mut mm).expect("tree init failed");

        let count = 800usize;
        for i in 0..count {
            tree.insert(
                padded_text_key(i, 7_000),
                RecordAddress {
                    page: i as u32,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("cascading insert failed");
        }

        // Verify the root is internal (tree has > 1 level)
        let root = BTreeNode::<Text>::read(tree.root_page(), &mut mm)
            .expect("root read after cascade failed");
        assert!(
            matches!(root.body, NodeBody::Internal(_)),
            "root must be internal after cascading splits"
        );

        // Verify the root has internal children (at least 3 levels)
        if let NodeBody::Internal(ref internal) = root.body {
            let first_child_page = internal.entries[0].child_page;
            let first_child = BTreeNode::<Text>::read(first_child_page, &mut mm)
                .expect("first child read failed");
            assert!(
                matches!(first_child.body, NodeBody::Internal(_)),
                "root's children must be internal (3+ levels)"
            );
        }

        // Spot-check a selection of keys across the entire range
        for i in [0, 1, 50, 100, 200, 400, 600, 799] {
            let key = padded_text_key(i, 7_000);
            let hits = tree
                .search(&key, &mut mm)
                .unwrap_or_else(|_| panic!("search failed for key {i}"));
            assert_eq!(
                hits.len(),
                1,
                "expected 1 hit for key {i}, got {}",
                hits.len()
            );
            assert_eq!(hits[0].page, i as u32);
        }

        // Verify range scan across the full tree works correctly
        let mut walker = tree
            .range_scan(&padded_text_key(0, 7_000), None, &mut mm)
            .expect("range scan after cascade failed");
        let mut scanned = 0usize;
        while walker.next(&mut mm).expect("walker next failed").is_some() {
            scanned += 1;
        }
        assert_eq!(scanned, count, "range scan must return all {count} entries");
    }

    #[test]
    fn test_delete_all_entries_after_cascading_splits() {
        // Insert enough to cause cascading splits, then delete everything.
        // This exercises delete across multi-level trees that leave empty leaves.
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        let count = 20_000u32;
        for i in 0..count {
            tree.insert(Uint32(i), RecordAddress { page: i, offset: 0 }, &mut mm)
                .expect("insert failed");
        }

        // Delete all entries
        for i in 0..count {
            tree.delete(&Uint32(i), RecordAddress { page: i, offset: 0 }, &mut mm)
                .unwrap_or_else(|_| panic!("delete failed for key {i}"));
        }

        // Search should return empty for every key
        for i in [0, 1, 100, 5000, 10000, 19999] {
            let hits = tree.search(&Uint32(i), &mut mm).expect("search failed");
            assert!(hits.is_empty(), "key {i} should have been deleted");
        }
    }

    #[test]
    fn test_insert_after_delete_reuses_tree_correctly() {
        // Insert, delete half, insert new entries. Ensures the tree remains
        // navigable after partial deletes that may leave empty leaves.
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for i in 0..10_000u32 {
            tree.insert(Uint32(i), RecordAddress { page: i, offset: 0 }, &mut mm)
                .expect("initial insert failed");
        }

        // Delete the first half
        for i in 0..5_000u32 {
            tree.delete(&Uint32(i), RecordAddress { page: i, offset: 0 }, &mut mm)
                .expect("delete failed");
        }

        // Insert new entries in the deleted range
        for i in 0..5_000u32 {
            tree.insert(
                Uint32(i),
                RecordAddress {
                    page: i + 20_000,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("re-insert failed");
        }

        // Verify all 10_000 entries are searchable
        for i in 0..5_000u32 {
            let hits = tree.search(&Uint32(i), &mut mm).expect("search failed");
            assert_eq!(hits.len(), 1, "key {i} should have exactly 1 hit");
            assert_eq!(hits[0].page, i + 20_000);
        }
        for i in 5_000..10_000u32 {
            let hits = tree.search(&Uint32(i), &mut mm).expect("search failed");
            assert_eq!(hits.len(), 1, "key {i} should have exactly 1 hit");
            assert_eq!(hits[0].page, i);
        }
    }

    #[test]
    fn test_range_scan_includes_duplicates_spanning_multiple_leaves() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for value in 0..6_000u32 {
            tree.insert(
                Uint32(15),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("duplicate insert failed");
        }
        tree.insert(
            Uint32(16),
            RecordAddress {
                page: 9_999,
                offset: 0,
            },
            &mut mm,
        )
        .expect("sentinel insert failed");

        let mut walker = tree
            .range_scan(&Uint32(15), Some(&Uint32(16)), &mut mm)
            .expect("range scan failed");
        let mut count = 0usize;
        while let Some(pointer) = walker.next(&mut mm).expect("walker next failed") {
            assert_eq!(pointer.page, count as u32);
            count += 1;
        }

        assert_eq!(count, 6_000);
    }

    /// Collects the keys of every leaf reachable from the leftmost leaf by
    /// following `next_leaf` links, in chain order.
    fn leaf_chain_keys(
        tree: &IndexTree<Uint32>,
        mm: &mut MemoryManager<HeapMemoryProvider>,
    ) -> Vec<(Page, Vec<Uint32>)> {
        let mut page = tree
            .find_leaf_page(&Uint32(0), mm)
            .expect("leftmost leaf lookup failed");
        let mut chain = Vec::new();
        loop {
            let node = BTreeNode::<Uint32>::read(page, mm).expect("leaf read failed");
            let NodeBody::Leaf(leaf) = node.body else {
                panic!("leaf chain reached an internal node");
            };
            chain.push((page, leaf.entries.iter().map(|entry| entry.key).collect()));
            match leaf.next_leaf {
                Some(next) => page = next,
                None => return chain,
            }
        }
    }

    #[test]
    fn test_reinsert_into_emptied_middle_leaf_is_visible_to_range_scans() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for value in 0..12_000u32 {
            tree.insert(
                Uint32(value),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("insert failed");
        }

        let chain = leaf_chain_keys(&tree, &mut mm);
        assert!(
            chain.len() >= 3,
            "workload must produce at least three leaves"
        );
        let (middle_page, middle_keys) = chain[1].clone();

        // Empty the middle leaf entirely.
        for key in &middle_keys {
            tree.delete(
                key,
                RecordAddress {
                    page: key.0,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("delete failed");
        }

        // Reinsert a key that routes back into the emptied leaf.
        let reinserted_key = middle_keys[0];
        let reinserted = RecordAddress {
            page: 99_999,
            offset: 0,
        };
        tree.insert(reinserted_key, reinserted, &mut mm)
            .expect("reinsert failed");
        assert_eq!(
            tree.find_leaf_page(&reinserted_key, &mut mm)
                .expect("leaf lookup failed"),
            middle_page,
            "reinserted key must route to the emptied middle leaf"
        );

        assert_eq!(
            tree.search(&reinserted_key, &mut mm)
                .expect("search failed"),
            vec![reinserted]
        );

        let mut walker = tree
            .range_scan(&Uint32(0), None, &mut mm)
            .expect("range scan failed");
        let mut scanned = Vec::new();
        while let Some(pointer) = walker.next(&mut mm).expect("walker next failed") {
            scanned.push(pointer);
        }
        assert!(
            scanned.contains(&reinserted),
            "covering range scan must return the reinserted entry"
        );
        let expected_len = 12_000 - middle_keys.len() + 1;
        assert_eq!(scanned.len(), expected_len);
    }

    #[test]
    fn test_next_entry_returns_keys_across_duplicate_runs_and_emptied_leaves() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        for page in 0..6_000u32 {
            tree.insert(Uint32(15), RecordAddress { page, offset: 0 }, &mut mm)
                .expect("duplicate insert failed");
        }
        for value in 16..6_016u32 {
            tree.insert(
                Uint32(value),
                RecordAddress {
                    page: value,
                    offset: 1,
                },
                &mut mm,
            )
            .expect("insert failed");
        }

        let chain = leaf_chain_keys(&tree, &mut mm);
        let (_, emptied_keys) = chain
            .iter()
            .find(|(_, keys)| keys.first().is_some_and(|key| key.0 > 15))
            .cloned()
            .expect("a leaf holding only keys above the duplicate run");
        for key in &emptied_keys {
            tree.delete(
                key,
                RecordAddress {
                    page: key.0,
                    offset: 1,
                },
                &mut mm,
            )
            .expect("delete failed");
        }

        let mut walker = tree
            .range_scan(&Uint32(15), None, &mut mm)
            .expect("range scan failed");
        let mut entries = Vec::new();
        while let Some(entry) = walker.next_entry(&mut mm).expect("next entry failed") {
            entries.push(entry);
        }

        let mut expected: Vec<(Uint32, RecordAddress)> = (0..6_000u32)
            .map(|page| (Uint32(15), RecordAddress { page, offset: 0 }))
            .collect();
        expected.extend(
            (16..6_016u32)
                .filter(|value| !emptied_keys.contains(&Uint32(*value)))
                .map(|value| {
                    (
                        Uint32(value),
                        RecordAddress {
                            page: value,
                            offset: 1,
                        },
                    )
                }),
        );
        assert_eq!(entries, expected);
    }

    #[test]
    fn test_next_entry_on_empty_tree_and_beyond_the_last_key() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        let mut walker = tree
            .range_scan(&Uint32(0), None, &mut mm)
            .expect("range scan failed");
        assert_eq!(walker.next_entry(&mut mm).expect("next entry failed"), None);

        for value in 0..100u32 {
            tree.insert(
                Uint32(value),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("insert failed");
        }
        let mut walker = tree
            .range_scan(&Uint32(500), None, &mut mm)
            .expect("range scan failed");
        assert_eq!(walker.next_entry(&mut mm).expect("next entry failed"), None);

        let mut walker = tree
            .range_scan(&Uint32(98), Some(&Uint32(99)), &mut mm)
            .expect("range scan failed");
        assert_eq!(
            walker.next_entry(&mut mm).expect("next entry failed"),
            Some((
                Uint32(98),
                RecordAddress {
                    page: 98,
                    offset: 0
                }
            ))
        );
        assert_eq!(walker.next_entry(&mut mm).expect("next entry failed"), None);
    }

    #[test]
    fn test_search_limited_stops_once_the_limit_is_exceeded() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");
        for page in 0..6_000u32 {
            tree.insert(Uint32(15), RecordAddress { page, offset: 0 }, &mut mm)
                .expect("duplicate insert failed");
        }
        tree.insert(
            Uint32(16),
            RecordAddress {
                page: 9_999,
                offset: 0,
            },
            &mut mm,
        )
        .expect("insert failed");

        let all = tree
            .search_limited(&Uint32(15), 6_000, &mut mm)
            .expect("search failed");
        assert_eq!(all.map(|hits| hits.len()), Some(6_000));
        assert_eq!(
            tree.search_limited(&Uint32(15), 5_999, &mut mm)
                .expect("search failed"),
            None
        );
        assert_eq!(
            tree.search_limited(&Uint32(16), 0, &mut mm)
                .expect("search failed"),
            None
        );
        assert_eq!(
            tree.search_limited(&Uint32(17), 0, &mut mm)
                .expect("search failed"),
            Some(vec![])
        );
    }

    #[test]
    fn test_duplicate_search_skips_emptied_middle_leaf() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        for value in 0..12_000u32 {
            tree.insert(
                Uint32(15),
                RecordAddress {
                    page: value,
                    offset: 0,
                },
                &mut mm,
            )
            .expect("duplicate insert failed");
        }

        let chain = leaf_chain_keys(&tree, &mut mm);
        assert!(
            chain.len() >= 3,
            "workload must produce at least three leaves"
        );
        let middle_page = chain[1].0;
        let NodeBody::Leaf(middle) = BTreeNode::<Uint32>::read(middle_page, &mut mm)
            .expect("leaf read failed")
            .body
        else {
            panic!("middle page must be a leaf");
        };
        let removed = middle.entries.len();
        for entry in &middle.entries {
            tree.delete(&entry.key, entry.pointer, &mut mm)
                .expect("delete failed");
        }

        let hits = tree.search(&Uint32(15), &mut mm).expect("search failed");
        assert_eq!(hits.len(), 12_000 - removed);

        let mut walker = tree
            .range_scan(&Uint32(15), Some(&Uint32(16)), &mut mm)
            .expect("range scan failed");
        let mut scanned = 0usize;
        while walker.next(&mut mm).expect("walker next failed").is_some() {
            scanned += 1;
        }
        assert_eq!(scanned, 12_000 - removed);
    }

    #[test]
    fn test_search_matches_brute_force_on_multi_level_tree_with_duplicates() {
        let mut mm = make_mm();
        let mut tree = IndexTree::<Uint32>::init(&mut mm).expect("tree init failed");

        // Even keys only, so odd keys are gaps; every seventh key is stored
        // twice. Keys are inserted out of order to exercise splits everywhere.
        let mut expected: std::collections::BTreeMap<u32, Vec<RecordAddress>> =
            std::collections::BTreeMap::new();
        for step in 0..15_000u32 {
            let key = ((step * 7_919) % 15_000) * 2;
            let copies = if key % 7 == 0 { 2 } else { 1 };
            for copy in 0..copies {
                let pointer = RecordAddress {
                    page: key,
                    offset: copy,
                };
                tree.insert(Uint32(key), pointer, &mut mm)
                    .expect("insert failed");
                expected.entry(key).or_default().push(pointer);
            }
        }
        let root = BTreeNode::<Uint32>::read(tree.root_page(), &mut mm).expect("root read failed");
        assert!(matches!(root.body, NodeBody::Internal(_)));

        for key in (0..30_003u32).step_by(3) {
            let mut hits = tree.search(&Uint32(key), &mut mm).expect("search failed");
            hits.sort();
            let want = expected.get(&key).cloned().unwrap_or_default();
            assert_eq!(hits, want, "search mismatch for key {key}");
        }
    }
}

// Rust guideline compliant 2026-03-27

//! The index ledger is responsible for keeping track of all the indexes in the database.
//! It allows for efficient lookup of indexes by name and provides a way to add and remove indexes from the ledger.
//!
//! The memory structure consists in having at root level a ledger which associates a name to an index,
//! and the index itself is a structure that contains the information about the index,
//! such as the columns it indexes and the type of index (e.g. B-tree, hash, etc.).

mod index_tree;

use std::collections::HashMap;

use wasm_dbms_api::memory::{DEFAULT_ALIGNMENT, MSize, Page};
use wasm_dbms_api::prelude::{Encode, IndexDef, MemoryError, MemoryResult, PageOffset};

use self::index_tree::IndexTree;
pub use self::index_tree::IndexTreeWalker;
use super::RecordAddress;
use crate::MemoryAccess;

/// The [`IndexLedger`] struct is responsible for managing and providing access to the indexes in the database.
pub struct IndexLedger {
    /// Page where the index ledger is stored.
    ledger_page: Page,
    /// Table mapping index names to their corresponding **root** page in memory.
    /// Since indexes are stored as B-trees, the page associated with each index name is the root page of the B-tree structure representing the index.
    tables: IndexLedgerTables,
}

/// The [`IndexLedgerTables`] struct is a wrapper around a `HashMap` that maps index names (as `Vec<String>`) to their corresponding root page in memory.
#[derive(Debug, Clone)]
struct IndexLedgerTables(HashMap<Vec<String>, Page>);

impl IndexLedger {
    /// Initializes the index ledger by creating an empty ledger page in memory and setting up the initial structure.
    pub fn init(
        ledger_page: Page,
        indexes: &[IndexDef],
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        Self::init_from_keys(
            ledger_page,
            indexes
                .iter()
                .map(|index| index.0.iter().map(ToString::to_string).collect::<Vec<_>>()),
            mm,
        )
    }

    /// Initializes the index ledger from owned column-key vectors.
    ///
    /// Used by the migration engine, which materialises a table from a
    /// [`TableSchemaSnapshot`] and therefore has no `'static` slice handy.
    pub fn init_from_keys<I>(
        ledger_page: Page,
        index_keys: I,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        I: IntoIterator<Item = Vec<String>>,
    {
        let mut tables = HashMap::new();
        for key in index_keys {
            let root_page = IndexTree::<wasm_dbms_api::prelude::Uint32>::init(mm)?.root_page();
            tables.insert(key, root_page);
        }

        let ledger = IndexLedger {
            ledger_page,
            tables: IndexLedgerTables(tables),
        };

        mm.write_at(ledger_page, 0, &ledger.tables)
    }

    /// Load the page ledger from memory at the given [`Page`].
    pub fn load(page: Page, mm: &mut impl MemoryAccess) -> MemoryResult<Self> {
        Ok(Self {
            tables: mm.read_at(page, 0)?,
            ledger_page: page,
        })
    }

    /// Inserts a key-pointer pair into the index identified by `columns`.
    pub fn insert<K>(
        &mut self,
        columns: &[&str],
        key: K,
        pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        K: Encode + Ord,
    {
        let root_page = self.lookup_root_page(columns)?;
        let mut tree = IndexTree::<K>::load(root_page);
        tree.insert(key, pointer, mm)?;
        self.persist_root_if_changed(columns, root_page, tree.root_page(), mm)
    }

    /// Looks up all pointers matching `key` in the index identified by `columns`.
    pub fn search<K>(
        &self,
        columns: &[&str],
        key: &K,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<Vec<RecordAddress>>
    where
        K: Encode + Ord,
    {
        let root_page = self.lookup_root_page(columns)?;
        IndexTree::<K>::load(root_page).search(key, mm)
    }

    /// Deletes a specific key-pointer pair from the index identified by `columns`.
    pub fn delete<K>(
        &mut self,
        columns: &[&str],
        key: &K,
        pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        K: Encode + Ord,
    {
        let root_page = self.lookup_root_page(columns)?;
        IndexTree::<K>::load(root_page).delete(key, pointer, mm)
    }

    /// Updates a specific key-pointer pair in the index identified by `columns`.
    pub fn update<K>(
        &mut self,
        columns: &[&str],
        key: &K,
        old_pointer: RecordAddress,
        new_pointer: RecordAddress,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()>
    where
        K: Encode + Ord,
    {
        let root_page = self.lookup_root_page(columns)?;
        let mut tree = IndexTree::<K>::load(root_page);
        tree.update(key, old_pointer, new_pointer, mm)?;
        self.persist_root_if_changed(columns, root_page, tree.root_page(), mm)
    }

    /// Opens a forward range scan on the index identified by `columns`.
    pub fn range_scan<K>(
        &self,
        columns: &[&str],
        start_key: &K,
        end_key: Option<&K>,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<IndexTreeWalker<K>>
    where
        K: Encode + Ord,
    {
        let root_page = self.lookup_root_page(columns)?;
        IndexTree::<K>::load(root_page).range_scan(start_key, end_key, mm)
    }

    fn lookup_root_page(&self, columns: &[&str]) -> MemoryResult<Page> {
        let key = columns.iter().map(ToString::to_string).collect::<Vec<_>>();
        self.tables
            .0
            .get(&key)
            .copied()
            .ok_or(MemoryError::IndexNotFound(key))
    }

    fn persist_root_if_changed(
        &mut self,
        columns: &[&str],
        old_root_page: Page,
        new_root_page: Page,
        mm: &mut impl MemoryAccess,
    ) -> MemoryResult<()> {
        if old_root_page == new_root_page {
            return Ok(());
        }

        let key = columns.iter().map(ToString::to_string).collect::<Vec<_>>();
        self.tables.0.insert(key, new_root_page);
        mm.write_at(self.ledger_page, 0, &self.tables)
    }
}

impl Encode for IndexLedgerTables {
    const ALIGNMENT: PageOffset = DEFAULT_ALIGNMENT;

    const SIZE: wasm_dbms_api::prelude::DataSize = wasm_dbms_api::prelude::DataSize::Dynamic;

    fn size(&self) -> wasm_dbms_api::prelude::MSize {
        // - 8: len of the ledger (number of entries)
        // - for each entry:
        //   - 8: len of the number of columns in the index (the key is a Vec<String>)
        //   - for each column name:
        //     - 1: len of the column name
        //     - (name.len() as MSize): the column name itself
        //   - 4: root page number (Page = u32)
        8 + self
            .0
            .keys()
            .map(|columns| {
                8 + columns
                    .iter()
                    .map(|col_name| 1 + (col_name.len() as MSize))
                    .sum::<MSize>()
                    + 4
            })
            .sum::<MSize>()
    }

    fn encode(&'_ self) -> std::borrow::Cow<'_, [u8]> {
        let mut bytes = Vec::with_capacity(self.size() as usize);
        // Encode the number of entries in the ledger
        bytes.extend_from_slice(&(self.0.len() as u64).to_le_bytes());
        for (columns, root_page) in &self.0 {
            // Encode the number of columns in the index
            bytes.extend_from_slice(&(columns.len() as u64).to_le_bytes());
            for col_name in columns {
                // Encode the length of the column name and the column name itself
                bytes.push(col_name.len() as u8);
                bytes.extend_from_slice(col_name.as_bytes());
            }
            // Encode the root page number (4 bytes, Page = u32)
            bytes.extend_from_slice(&root_page.to_le_bytes());
        }
        bytes.into()
    }

    fn decode(data: std::borrow::Cow<[u8]>) -> MemoryResult<Self>
    where
        Self: Sized,
    {
        let mut offset = 0;
        // Decode the number of entries in the ledger
        let num_entries = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;
        let mut tables = HashMap::with_capacity(num_entries);
        for _ in 0..num_entries {
            // Decode the number of columns in the index
            let num_columns = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
            offset += 8;
            let mut columns = Vec::with_capacity(num_columns);
            for _ in 0..num_columns {
                // Decode the length of the column name and the column name itself
                let col_name_len = data[offset] as usize;
                offset += 1;
                let col_name = String::from_utf8(data[offset..offset + col_name_len].to_vec())?;
                offset += col_name_len;
                columns.push(col_name);
            }
            // Decode the root page number (4 bytes, Page = u32)
            let root_page = Page::from_le_bytes(data[offset..offset + 4].try_into()?);
            offset += 4;
            tables.insert(columns, root_page);
        }
        Ok(Self(tables))
    }
}

#[cfg(test)]
mod tests {
    use wasm_dbms_api::prelude::Uint32;

    use super::*;
    use crate::{HeapMemoryProvider, MemoryManager};

    #[test]
    fn test_encode_decode_empty_ledger() {
        let tables = IndexLedgerTables(HashMap::new());
        let encoded = tables.encode();
        let decoded = IndexLedgerTables::decode(encoded).expect("decode failed");

        assert!(decoded.0.is_empty());
    }

    #[test]
    fn test_encode_decode_single_column_index() {
        let mut map = HashMap::new();
        map.insert(vec!["name".to_string()], 42u32);
        let tables = IndexLedgerTables(map);

        let encoded = tables.encode();
        let decoded = IndexLedgerTables::decode(encoded).expect("decode failed");

        assert_eq!(decoded.0.len(), 1);
        assert_eq!(decoded.0[&vec!["name".to_string()]], 42);
    }

    #[test]
    fn test_encode_decode_composite_index() {
        let mut map = HashMap::new();
        map.insert(
            vec!["first_name".to_string(), "last_name".to_string()],
            99u32,
        );
        let tables = IndexLedgerTables(map);

        let encoded = tables.encode();
        let decoded = IndexLedgerTables::decode(encoded).expect("decode failed");

        assert_eq!(decoded.0.len(), 1);
        assert_eq!(
            decoded.0[&vec!["first_name".to_string(), "last_name".to_string()]],
            99
        );
    }

    #[test]
    fn test_encode_decode_multiple_indexes() {
        let mut map = HashMap::new();
        map.insert(vec!["id".to_string()], 10u32);
        map.insert(vec!["email".to_string()], 20u32);
        map.insert(vec!["city".to_string(), "zip".to_string()], 30u32);
        let tables = IndexLedgerTables(map.clone());

        let encoded = tables.encode();
        let decoded = IndexLedgerTables::decode(encoded).expect("decode failed");

        assert_eq!(decoded.0.len(), 3);
        assert_eq!(decoded.0[&vec!["id".to_string()]], 10);
        assert_eq!(decoded.0[&vec!["email".to_string()]], 20);
        assert_eq!(decoded.0[&vec!["city".to_string(), "zip".to_string()]], 30);
    }

    #[test]
    fn test_size_matches_encoded_length() {
        let mut map = HashMap::new();
        map.insert(vec!["id".to_string()], 10u32);
        map.insert(vec!["first".to_string(), "last".to_string()], 20u32);
        let tables = IndexLedgerTables(map);

        let size = tables.size() as usize;
        let encoded = tables.encode();

        assert_eq!(
            encoded.len(),
            size,
            "size() returned {size} but encode() produced {} bytes",
            encoded.len()
        );
    }

    #[test]
    fn test_size_empty_ledger() {
        let tables = IndexLedgerTables(HashMap::new());
        // Only the 8-byte entry count
        assert_eq!(tables.size(), 8);
    }

    #[test]
    fn test_encode_decode_large_page_number() {
        let mut map = HashMap::new();
        // Use a page number > u16::MAX to verify 4-byte encoding
        map.insert(vec!["col".to_string()], 70_000u32);
        let tables = IndexLedgerTables(map);

        let encoded = tables.encode();
        let decoded = IndexLedgerTables::decode(encoded).expect("decode failed");

        assert_eq!(decoded.0[&vec!["col".to_string()]], 70_000);
    }

    #[test]
    fn test_init_no_indexes() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");

        IndexLedger::init(ledger_page, &[], &mut mm).expect("init failed");

        let loaded = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        assert!(loaded.tables.0.is_empty());
    }

    #[test]
    fn test_init_and_load_single_index() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");

        let indexes = [IndexDef(&["email"])];
        IndexLedger::init(ledger_page, &indexes, &mut mm).expect("init failed");

        let loaded = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        assert_eq!(loaded.tables.0.len(), 1);
        assert!(loaded.tables.0.contains_key(&vec!["email".to_string()]));
    }

    #[test]
    fn test_init_and_load_multiple_indexes() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");

        let indexes = [IndexDef(&["id"]), IndexDef(&["first_name", "last_name"])];
        IndexLedger::init(ledger_page, &indexes, &mut mm).expect("init failed");

        let loaded = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        assert_eq!(loaded.tables.0.len(), 2);
        assert!(loaded.tables.0.contains_key(&vec!["id".to_string()]));
        assert!(
            loaded
                .tables
                .0
                .contains_key(&vec!["first_name".to_string(), "last_name".to_string()])
        );
    }

    #[test]
    fn test_init_allocates_distinct_root_pages() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");

        let indexes = [IndexDef(&["a"]), IndexDef(&["b"]), IndexDef(&["c"])];
        IndexLedger::init(ledger_page, &indexes, &mut mm).expect("init failed");

        let loaded = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        let pages: Vec<Page> = loaded.tables.0.values().copied().collect();
        // All root pages should be distinct
        let mut unique_pages = pages.clone();
        unique_pages.sort();
        unique_pages.dedup();
        assert_eq!(pages.len(), unique_pages.len());
    }

    #[test]
    fn test_ledger_insert_search_delete_update_and_range_scan() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");
        let indexes = [IndexDef(&["email"])];
        IndexLedger::init(ledger_page, &indexes, &mut mm).expect("init failed");
        let mut ledger = IndexLedger::load(ledger_page, &mut mm).expect("load failed");

        let first = RecordAddress { page: 1, offset: 0 };
        let second = RecordAddress { page: 2, offset: 0 };
        ledger
            .insert(&["email"], Uint32(10), first.clone(), &mut mm)
            .expect("first insert failed");
        ledger
            .insert(&["email"], Uint32(10), second.clone(), &mut mm)
            .expect("second insert failed");
        ledger
            .insert(
                &["email"],
                Uint32(11),
                RecordAddress { page: 3, offset: 0 },
                &mut mm,
            )
            .expect("third insert failed");

        let hits = ledger
            .search(&["email"], &Uint32(10), &mut mm)
            .expect("search failed");
        assert_eq!(hits, vec![first.clone(), second.clone()]);

        ledger
            .delete(&["email"], &Uint32(10), first, &mut mm)
            .expect("delete failed");
        let hits = ledger
            .search(&["email"], &Uint32(10), &mut mm)
            .expect("search after delete failed");
        assert_eq!(hits, vec![second.clone()]);

        let replacement = RecordAddress { page: 4, offset: 0 };
        ledger
            .update(
                &["email"],
                &Uint32(10),
                second,
                replacement.clone(),
                &mut mm,
            )
            .expect("update failed");
        let hits = ledger
            .search(&["email"], &Uint32(10), &mut mm)
            .expect("search after update failed");
        assert_eq!(hits, vec![replacement]);

        let mut walker = ledger
            .range_scan(&["email"], &Uint32(10), Some(&Uint32(12)), &mut mm)
            .expect("range scan failed");
        assert_eq!(
            walker.next(&mut mm).expect("walker next failed"),
            Some(RecordAddress { page: 4, offset: 0 })
        );
        assert_eq!(
            walker.next(&mut mm).expect("walker next failed"),
            Some(RecordAddress { page: 3, offset: 0 })
        );
        assert_eq!(walker.next(&mut mm).expect("walker end failed"), None);
    }

    #[test]
    fn test_ledger_missing_index_returns_error() {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.allocate_page().expect("failed to allocate page");
        IndexLedger::init(ledger_page, &[], &mut mm).expect("init failed");
        let mut ledger = IndexLedger::load(ledger_page, &mut mm).expect("load failed");

        let error = ledger
            .insert(
                &["missing"],
                Uint32(1),
                RecordAddress { page: 1, offset: 0 },
                &mut mm,
            )
            .expect_err("missing index insert must fail");
        assert!(
            matches!(error, MemoryError::IndexNotFound(columns) if columns == vec!["missing".to_string()])
        );
    }
}

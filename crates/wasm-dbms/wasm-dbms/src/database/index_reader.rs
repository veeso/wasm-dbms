// Rust guideline compliant 2026-03-29
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Unified index reader for persistent indexes plus transaction overlay changes.

use std::collections::{BTreeSet, HashSet};

use wasm_dbms_api::prelude::{MemoryResult, Value};
use wasm_dbms_memory::prelude::MemoryAccess;
use wasm_dbms_memory::{IndexLedger, RecordAddress};

use crate::transaction::IndexOverlay;

/// Result of an index search.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct IndexSearchResult {
    /// Base-table record addresses returned by the persistent index.
    pub addresses: Vec<RecordAddress>,
    /// Primary keys added by the transaction overlay for the searched key space.
    pub overlay_pks: HashSet<Value>,
    /// Primary keys removed by the transaction overlay for the searched key space.
    pub removed_pks: HashSet<Value>,
}

/// A reader for indexes that abstracts over the base index ledger and any transaction overlay.
pub struct IndexReader<'a> {
    ledger: &'a IndexLedger,
    overlay: Option<&'a IndexOverlay>,
}

impl<'a> IndexReader<'a> {
    /// Creates a new `IndexReader` with the given ledger and optional overlay.
    pub fn new(ledger: &'a IndexLedger, overlay: Option<&'a IndexOverlay>) -> Self {
        Self { ledger, overlay }
    }
}

impl IndexReader<'_> {
    fn unique_addresses(addresses: impl IntoIterator<Item = RecordAddress>) -> Vec<RecordAddress> {
        addresses
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Performs an exact-match lookup on the index.
    pub fn search_eq<MA>(
        &self,
        columns: &[&'static str],
        key: &[Value],
        mm: &mut MA,
    ) -> MemoryResult<IndexSearchResult>
    where
        MA: MemoryAccess,
    {
        Ok(IndexSearchResult {
            addresses: Self::unique_addresses(self.ledger.search(columns, &key.to_vec(), mm)?),
            overlay_pks: self
                .overlay
                .map_or_else(HashSet::new, |overlay| overlay.added_pks(columns, key)),
            removed_pks: self
                .overlay
                .map_or_else(HashSet::new, |overlay| overlay.removed_pks(columns, key)),
        })
    }

    /// Performs an inclusive range lookup on the index.
    pub fn search_range<MA>(
        &self,
        columns: &[&'static str],
        start: Option<&[Value]>,
        end: Option<&[Value]>,
        mm: &mut MA,
    ) -> MemoryResult<IndexSearchResult>
    where
        MA: MemoryAccess,
    {
        let empty_start = Vec::new();
        let start_key = start.unwrap_or(empty_start.as_slice()).to_vec();
        let end_key = end.map(ToOwned::to_owned);
        let mut walker = self
            .ledger
            .range_scan(columns, &start_key, end_key.as_ref(), mm)?;

        let mut addresses = Vec::new();
        while let Some(address) = walker.next(mm)? {
            addresses.push(address);
        }

        if let Some(end_key) = end {
            addresses.extend(self.ledger.search(columns, &end_key.to_vec(), mm)?);
        }

        Ok(IndexSearchResult {
            addresses: Self::unique_addresses(addresses),
            overlay_pks: self.overlay.map_or_else(HashSet::new, |overlay| {
                overlay.added_pks_in_range(columns, start, end)
            }),
            removed_pks: self.overlay.map_or_else(HashSet::new, |overlay| {
                overlay.removed_pks_in_range(columns, start, end)
            }),
        })
    }

    /// Performs an IN lookup by merging exact-match lookups.
    pub fn search_in<MA>(
        &self,
        columns: &[&'static str],
        values: &[Vec<Value>],
        mm: &mut MA,
    ) -> MemoryResult<IndexSearchResult>
    where
        MA: MemoryAccess,
    {
        let mut merged = IndexSearchResult::default();
        let mut addresses = BTreeSet::new();

        for key in values {
            let result = self.search_eq(columns, key, mm)?;
            addresses.extend(result.addresses);
            merged.overlay_pks.extend(result.overlay_pks);
            merged.removed_pks.extend(result.removed_pks);
        }

        merged.addresses = addresses.into_iter().collect();
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashSet;

    use wasm_dbms_api::prelude::{IndexDef, Value};
    use wasm_dbms_memory::prelude::{HeapMemoryProvider, MemoryAccess, MemoryManager};
    use wasm_dbms_memory::{IndexLedger, RecordAddress};

    use super::{IndexReader, IndexSearchResult};
    use crate::transaction::IndexOverlay;

    fn setup_ledger() -> (MemoryManager<HeapMemoryProvider>, IndexLedger) {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.claim_page().expect("failed to allocate page");
        IndexLedger::init(ledger_page, &[IndexDef(&["name"])], &mut mm).expect("init failed");
        let ledger = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        (mm, ledger)
    }

    fn name_key(value: &str) -> Vec<Value> {
        vec![Value::Text(value.to_string().into())]
    }

    fn pk(id: u32) -> Value {
        Value::Uint32(id.into())
    }

    #[test]
    fn test_search_eq_base_only() {
        let (mut mm, mut ledger) = setup_ledger();
        let addr1 = RecordAddress::new(10, 0);
        let addr2 = RecordAddress::new(11, 0);
        ledger
            .insert(&["name"], name_key("alice"), addr1, &mut mm)
            .expect("insert failed");
        ledger
            .insert(&["name"], name_key("bob"), addr2, &mut mm)
            .expect("insert failed");

        let result = IndexReader::new(&ledger, None)
            .search_eq(&["name"], &name_key("alice"), &mut mm)
            .expect("search failed");

        assert_eq!(
            result,
            IndexSearchResult {
                addresses: vec![addr1],
                overlay_pks: HashSet::new(),
                removed_pks: HashSet::new(),
            }
        );
    }

    #[test]
    fn test_search_eq_with_overlay_additions() {
        let (mut mm, ledger) = setup_ledger();
        let mut overlay = IndexOverlay::default();
        overlay.insert(&["name"], name_key("alice"), pk(1));

        let result = IndexReader::new(&ledger, Some(&overlay))
            .search_eq(&["name"], &name_key("alice"), &mut mm)
            .expect("search failed");

        assert!(result.addresses.is_empty());
        assert!(result.overlay_pks.contains(&pk(1)));
        assert!(result.removed_pks.is_empty());
    }

    #[test]
    fn test_search_range_with_overlay() {
        let (mut mm, mut ledger) = setup_ledger();
        let addr1 = RecordAddress::new(10, 0);
        let addr2 = RecordAddress::new(11, 0);
        let addr3 = RecordAddress::new(12, 0);
        ledger
            .insert(&["name"], name_key("alice"), addr1, &mut mm)
            .expect("insert failed");
        ledger
            .insert(&["name"], name_key("bob"), addr2, &mut mm)
            .expect("insert failed");
        ledger
            .insert(&["name"], name_key("charlie"), addr3, &mut mm)
            .expect("insert failed");

        let mut overlay = IndexOverlay::default();
        overlay.insert(&["name"], name_key("carol"), pk(4));
        overlay.delete(&["name"], name_key("bob"), pk(2));

        let result = IndexReader::new(&ledger, Some(&overlay))
            .search_range(
                &["name"],
                Some(name_key("alice").as_slice()),
                Some(name_key("carol").as_slice()),
                &mut mm,
            )
            .expect("range search failed");

        assert_eq!(result.addresses, vec![addr1, addr2]);
        assert!(result.overlay_pks.contains(&pk(4)));
        assert!(result.removed_pks.contains(&pk(2)));
    }

    #[test]
    fn test_search_in_merges_results() {
        let (mut mm, mut ledger) = setup_ledger();
        let addr1 = RecordAddress::new(10, 0);
        let addr2 = RecordAddress::new(11, 0);
        ledger
            .insert(&["name"], name_key("alice"), addr1, &mut mm)
            .expect("insert failed");
        ledger
            .insert(&["name"], name_key("bob"), addr2, &mut mm)
            .expect("insert failed");

        let result = IndexReader::new(&ledger, None)
            .search_in(
                &["name"],
                &[name_key("alice"), name_key("bob"), name_key("alice")],
                &mut mm,
            )
            .expect("search failed");

        assert_eq!(result.addresses, vec![addr1, addr2]);
        assert!(result.overlay_pks.is_empty());
        assert!(result.removed_pks.is_empty());
    }
}

// Rust guideline compliant 2026-03-29
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Persistent index reads: exact keys, prefix ranges, and key-bearing entry streams.

use std::ops::{Bound, ControlFlow};

use wasm_dbms_api::prelude::{ColumnDef, MemoryError, MemoryResult, Value};
use wasm_dbms_memory::prelude::MemoryAccess;
use wasm_dbms_memory::{IndexLedger, RecordAddress};

/// One contiguous key range of an index: an equality prefix on the leading
/// index columns plus optional bounds on the next column.
///
/// A scan whose prefix covers every index column is an exact key lookup and
/// ignores its bounds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexScan {
    /// Ordered columns of the scanned index.
    pub columns: &'static [&'static str],
    /// Values of the leading index columns, compared by equality.
    pub prefix: Vec<Value>,
    /// Lower bound on the column after the prefix.
    pub lower: Bound<Value>,
    /// Upper bound on the column after the prefix.
    pub upper: Bound<Value>,
}

impl IndexScan {
    /// Creates a scan of every key starting with `prefix`.
    pub fn prefix(columns: &'static [&'static str], prefix: Vec<Value>) -> Self {
        Self::range(columns, prefix, Bound::Unbounded, Bound::Unbounded)
    }

    /// Creates a scan of the keys starting with `prefix` whose next component
    /// lies between `lower` and `upper`.
    pub fn range(
        columns: &'static [&'static str],
        prefix: Vec<Value>,
        lower: Bound<Value>,
        upper: Bound<Value>,
    ) -> Self {
        Self {
            columns,
            prefix,
            lower,
            upper,
        }
    }

    /// Returns whether the scan looks up one complete key.
    pub fn is_exact(&self) -> bool {
        self.prefix.len() >= self.columns.len()
    }

    /// Returns whether the row's visible index key lies in this scan.
    pub fn matches_row(&self, row: &[(ColumnDef, Value)]) -> bool {
        let value = |position: usize| {
            row.iter()
                .find(|(column, _)| column.name == self.columns[position])
                .map(|(_, value)| value)
        };
        if !self
            .prefix
            .iter()
            .enumerate()
            .all(|(position, expected)| value(position) == Some(expected))
        {
            return false;
        }
        self.is_exact()
            || value(self.prefix.len())
                .is_some_and(|value| self.admits_lower(value) && self.admits_upper(value))
    }

    /// Returns the columns whose changes can move a row into this scan.
    pub fn constrained_columns(&self) -> &[&'static str] {
        let has_range = !matches!(
            (&self.lower, &self.upper),
            (Bound::Unbounded, Bound::Unbounded)
        );
        let depth = (self.prefix.len() + usize::from(has_range)).min(self.columns.len());
        &self.columns[..depth]
    }

    /// Returns the key the cursor starts from.
    ///
    /// A shorter vector sorts before every longer vector sharing its
    /// elements, so the prefix alone is a valid lower bound.
    fn seek_key(&self) -> Vec<Value> {
        let mut key = self.prefix.clone();
        if let Bound::Included(value) | Bound::Excluded(value) = &self.lower {
            key.push(value.clone());
        }
        key
    }

    /// Returns whether `component` satisfies the lower bound.
    fn admits_lower(&self, component: &Value) -> bool {
        match &self.lower {
            Bound::Included(lower) => component >= lower,
            Bound::Excluded(lower) => component > lower,
            Bound::Unbounded => true,
        }
    }

    /// Returns whether `component` satisfies the upper bound.
    fn admits_upper(&self, component: &Value) -> bool {
        match &self.upper {
            Bound::Included(upper) => component <= upper,
            Bound::Excluded(upper) => component < upper,
            Bound::Unbounded => true,
        }
    }
}

/// Reads persistent index entries; transaction changes are reconciled by the
/// caller.
pub struct IndexReader<'a> {
    ledger: &'a IndexLedger,
}

impl<'a> IndexReader<'a> {
    /// Creates a reader over `ledger`.
    pub fn new(ledger: &'a IndexLedger) -> Self {
        Self { ledger }
    }
}

impl IndexReader<'_> {
    /// Returns the addresses stored under exactly `key`.
    ///
    /// Uses the serialized-node search, which decodes only visited keys.
    pub fn exact<MA>(
        &self,
        columns: &[&str],
        key: &[Value],
        mm: &mut MA,
    ) -> MemoryResult<Vec<RecordAddress>>
    where
        MA: MemoryAccess,
    {
        self.ledger.search(columns, &key.to_vec(), mm)
    }

    /// Collects the record addresses of `scan`, or returns `None` when it
    /// holds more than `limit` entries.
    ///
    /// A range stops reading at the first entry past `limit`.
    pub fn addresses<MA>(
        &self,
        scan: &IndexScan,
        limit: usize,
        mm: &mut MA,
    ) -> MemoryResult<Option<Vec<RecordAddress>>>
    where
        MA: MemoryAccess,
    {
        if scan.is_exact() {
            return self
                .ledger
                .search_limited(scan.columns, &scan.prefix, limit, mm);
        }

        let mut addresses = Vec::new();
        let mut over_limit = false;
        self.for_each_entry(scan, mm, |_, address| {
            if addresses.len() == limit {
                over_limit = true;
                return Ok::<_, MemoryError>(ControlFlow::Break(()));
            }
            addresses.push(address);
            Ok(ControlFlow::Continue(()))
        })?;
        Ok((!over_limit).then_some(addresses))
    }

    /// Visits the key and record address of every entry of `scan` in key
    /// order, until `visit` returns [`ControlFlow::Break`].
    ///
    /// The cursor stops as soon as the prefix changes or the upper bound is
    /// passed, so a scan reads the tree descent plus the matching entries.
    pub fn for_each_entry<MA, F, E>(
        &self,
        scan: &IndexScan,
        mm: &mut MA,
        mut visit: F,
    ) -> Result<(), E>
    where
        MA: MemoryAccess,
        F: FnMut(Vec<Value>, RecordAddress) -> Result<ControlFlow<()>, E>,
        E: From<MemoryError>,
    {
        if scan.is_exact() {
            for address in self.exact(scan.columns, &scan.prefix, mm)? {
                if visit(scan.prefix.clone(), address)?.is_break() {
                    break;
                }
            }
            return Ok(());
        }

        let depth = scan.prefix.len();
        let mut walker =
            self.ledger
                .range_scan::<Vec<Value>>(scan.columns, &scan.seek_key(), None, mm)?;
        while let Some((key, address)) = walker.next_entry(mm)? {
            if key.len() <= depth || key[..depth] != scan.prefix[..] {
                break;
            }
            let component = &key[depth];
            if !scan.admits_lower(component) {
                continue;
            }
            if !scan.admits_upper(component) {
                break;
            }
            if visit(key, address)?.is_break() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::ops::{Bound, ControlFlow};

    use wasm_dbms_api::prelude::{IndexDef, MemoryError, Text, Uint32, Value};
    use wasm_dbms_memory::prelude::{HeapMemoryProvider, MemoryAccess, MemoryManager};
    use wasm_dbms_memory::{IndexLedger, RecordAddress};

    use super::{IndexReader, IndexScan};

    const PAIR: &[&str] = &["category", "brand"];

    fn setup_pair_ledger() -> (MemoryManager<HeapMemoryProvider>, IndexLedger) {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let ledger_page = mm.claim_page().expect("failed to allocate page");
        IndexLedger::init(ledger_page, &[IndexDef(PAIR)], &mut mm).expect("init failed");
        let ledger = IndexLedger::load(ledger_page, &mut mm).expect("load failed");
        (mm, ledger)
    }

    fn uint(value: u32) -> Value {
        Value::Uint32(Uint32(value))
    }

    fn text(value: &str) -> Value {
        Value::Text(Text(value.to_string()))
    }

    fn insert_pair(
        ledger: &mut IndexLedger,
        mm: &mut MemoryManager<HeapMemoryProvider>,
        key: Vec<Value>,
        page: u32,
    ) {
        ledger
            .insert(PAIR, key, RecordAddress::new(page, 0), mm)
            .expect("insert failed");
    }

    /// Collects every key of `scan` in cursor order.
    fn scan_keys(
        ledger: &IndexLedger,
        mm: &mut MemoryManager<HeapMemoryProvider>,
        scan: &IndexScan,
    ) -> Vec<Vec<Value>> {
        let mut keys = Vec::new();
        IndexReader::new(ledger)
            .for_each_entry(scan, mm, |key, _| {
                keys.push(key);
                Ok::<_, MemoryError>(ControlFlow::Continue(()))
            })
            .expect("scan failed");
        keys
    }

    /// Inserts `[c, b]` for `c` in `0..30` and `b` in `0..200`: 6,000 entries
    /// over many leaves.
    fn seed_grid(ledger: &mut IndexLedger, mm: &mut MemoryManager<HeapMemoryProvider>) {
        for category in 0..30 {
            for brand in 0..200 {
                insert_pair(
                    ledger,
                    mm,
                    vec![uint(category), uint(brand)],
                    category * 1_000 + brand,
                );
            }
        }
    }

    #[test]
    fn test_prefix_scan_stops_when_the_prefix_changes() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        seed_grid(&mut ledger, &mut mm);

        let keys = scan_keys(&ledger, &mut mm, &IndexScan::prefix(PAIR, vec![uint(7)]));
        let expected: Vec<_> = (0..200).map(|brand| vec![uint(7), uint(brand)]).collect();
        assert_eq!(keys, expected);

        let addresses = IndexReader::new(&ledger)
            .addresses(&IndexScan::prefix(PAIR, vec![uint(29)]), 200, &mut mm)
            .expect("scan failed")
            .expect("within budget");
        assert_eq!(addresses.len(), 200);
        assert!(addresses.iter().all(|address| address.page / 1_000 == 29));
    }

    #[test]
    fn test_prefix_range_honours_inclusive_and_exclusive_bounds() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        seed_grid(&mut ledger, &mut mm);

        let brands = |lower, upper, mm: &mut MemoryManager<HeapMemoryProvider>| {
            scan_keys(
                &ledger,
                mm,
                &IndexScan::range(PAIR, vec![uint(7)], lower, upper),
            )
            .into_iter()
            .map(|key| key[1].clone())
            .collect::<Vec<_>>()
        };
        assert_eq!(
            brands(Bound::Included(uint(3)), Bound::Excluded(uint(5)), &mut mm),
            vec![uint(3), uint(4)]
        );
        assert_eq!(
            brands(Bound::Excluded(uint(3)), Bound::Included(uint(5)), &mut mm),
            vec![uint(4), uint(5)]
        );
        assert_eq!(
            brands(Bound::Excluded(uint(197)), Bound::Unbounded, &mut mm),
            vec![uint(198), uint(199)]
        );
        assert_eq!(
            brands(Bound::Unbounded, Bound::Excluded(uint(2)), &mut mm),
            vec![uint(0), uint(1)]
        );
        assert!(brands(Bound::Included(uint(250)), Bound::Unbounded, &mut mm).is_empty());
    }

    #[test]
    fn test_prefix_scan_keeps_duplicate_runs_spanning_leaves() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        for page in 0..3_000 {
            insert_pair(&mut ledger, &mut mm, vec![uint(7), uint(1)], page);
        }
        insert_pair(&mut ledger, &mut mm, vec![uint(7), uint(2)], 5_000);
        insert_pair(&mut ledger, &mut mm, vec![uint(8), uint(0)], 6_000);

        let reader = IndexReader::new(&ledger);
        let run = reader
            .addresses(
                &IndexScan::range(
                    PAIR,
                    vec![uint(7)],
                    Bound::Included(uint(1)),
                    Bound::Included(uint(1)),
                ),
                4_096,
                &mut mm,
            )
            .expect("scan failed")
            .expect("within budget");
        assert_eq!(run.len(), 3_000);
        let after_run = scan_keys(
            &ledger,
            &mut mm,
            &IndexScan::range(
                PAIR,
                vec![uint(7)],
                Bound::Excluded(uint(1)),
                Bound::Unbounded,
            ),
        );
        assert_eq!(after_run, vec![vec![uint(7), uint(2)]]);
        let exact = reader
            .addresses(
                &IndexScan::prefix(PAIR, vec![uint(7), uint(1)]),
                4_096,
                &mut mm,
            )
            .expect("lookup failed")
            .expect("within budget");
        assert_eq!(exact.len(), 3_000);
    }

    #[test]
    fn test_range_scan_crosses_emptied_leaves() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        for category in 0..120 {
            for brand in 0..100 {
                insert_pair(
                    &mut ledger,
                    &mut mm,
                    vec![uint(category), uint(brand)],
                    category * 1_000 + brand,
                );
            }
        }
        // 4,000 contiguous entries are more than any leaf holds, so at least one
        // leaf in the middle becomes empty but stays linked.
        for category in 40..80 {
            for brand in 0..100 {
                ledger
                    .delete(
                        PAIR,
                        &vec![uint(category), uint(brand)],
                        RecordAddress::new(category * 1_000 + brand, 0),
                        &mut mm,
                    )
                    .expect("delete failed");
            }
        }

        let keys = scan_keys(
            &ledger,
            &mut mm,
            &IndexScan::range(
                PAIR,
                vec![],
                Bound::Included(uint(39)),
                Bound::Included(uint(80)),
            ),
        );
        assert_eq!(keys.len(), 200);
        assert_eq!(keys.first(), Some(&vec![uint(39), uint(0)]));
        assert_eq!(keys.last(), Some(&vec![uint(80), uint(99)]));
        assert_eq!(
            scan_keys(&ledger, &mut mm, &IndexScan::prefix(PAIR, vec![uint(60)])),
            Vec::<Vec<Value>>::new()
        );
        assert_eq!(
            scan_keys(&ledger, &mut mm, &IndexScan::prefix(PAIR, vec![uint(80)])).len(),
            100
        );
    }

    #[test]
    fn test_scans_on_an_empty_index_and_beyond_the_last_key() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        assert!(scan_keys(&ledger, &mut mm, &IndexScan::prefix(PAIR, vec![])).is_empty());
        seed_grid(&mut ledger, &mut mm);
        assert!(scan_keys(&ledger, &mut mm, &IndexScan::prefix(PAIR, vec![uint(999)])).is_empty());
        assert!(
            scan_keys(
                &ledger,
                &mut mm,
                &IndexScan::range(
                    PAIR,
                    vec![uint(29)],
                    Bound::Excluded(uint(199)),
                    Bound::Unbounded,
                ),
            )
            .is_empty()
        );
    }

    #[test]
    fn test_prefix_scan_with_empty_unicode_and_null_components() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        let firsts = [text(""), text("a"), text("é"), text("b"), Value::Null];
        for (position, first) in firsts.iter().enumerate() {
            for brand in 0..3 {
                insert_pair(
                    &mut ledger,
                    &mut mm,
                    vec![first.clone(), uint(brand)],
                    (position * 10 + brand as usize) as u32,
                );
            }
        }
        for first in &firsts {
            let keys = scan_keys(
                &ledger,
                &mut mm,
                &IndexScan::prefix(PAIR, vec![first.clone()]),
            );
            assert_eq!(keys.len(), 3, "prefix {first:?}");
            assert!(keys.iter().all(|key| key[0] == *first));
        }
        let firsts_in_range: Vec<_> = scan_keys(
            &ledger,
            &mut mm,
            &IndexScan::range(
                PAIR,
                vec![],
                Bound::Included(text("")),
                Bound::Excluded(text("b")),
            ),
        )
        .into_iter()
        .map(|key| key[0].clone())
        .collect();
        assert_eq!(
            firsts_in_range,
            vec![
                text(""),
                text(""),
                text(""),
                text("a"),
                text("a"),
                text("a")
            ]
        );
    }

    #[test]
    fn test_addresses_returns_none_above_the_limit() {
        let (mut mm, mut ledger) = setup_pair_ledger();
        for brand in 0..100 {
            insert_pair(&mut ledger, &mut mm, vec![uint(7), uint(brand)], brand);
        }
        let reader = IndexReader::new(&ledger);
        let scan = IndexScan::prefix(PAIR, vec![uint(7)]);
        assert_eq!(
            reader.addresses(&scan, 99, &mut mm).expect("scan failed"),
            None
        );
        assert_eq!(
            reader
                .addresses(&scan, 100, &mut mm)
                .expect("scan failed")
                .map(|addresses| addresses.len()),
            Some(100)
        );
        let exact = IndexScan::prefix(PAIR, vec![uint(7), uint(3)]);
        assert_eq!(
            reader.addresses(&exact, 0, &mut mm).expect("lookup failed"),
            None
        );
        assert_eq!(
            reader
                .exact(PAIR, &[uint(7), uint(3)], &mut mm)
                .expect("lookup failed"),
            vec![RecordAddress::new(3, 0)]
        );
    }
}

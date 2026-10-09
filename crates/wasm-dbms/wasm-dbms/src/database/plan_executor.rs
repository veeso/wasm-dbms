// Rust guideline compliant 2026-10-09
// X-WHERE-CLAUSE, M-CANONICAL-DOCS

//! Chooses between the access-path planner and the legacy planner, and turns
//! access paths into persistent record addresses under a budget.

use std::ops::Bound;

use wasm_dbms_api::prelude::{ColumnDef, Filter, IndexDef, MemoryResult, Value};
use wasm_dbms_memory::RecordAddress;
use wasm_dbms_memory::prelude::MemoryAccess;

use super::access_planner::{AccessPath, is_fallible, plan_filter};
use super::filter_analyzer::{IndexPlan, analyze_filter};
use super::index_reader::{IndexReader, IndexScan};

/// Largest number of candidate addresses one index range may produce.
pub const SCAN_BUDGET: usize = 4_096;
/// Largest number of candidate addresses one composed plan may produce.
pub const TOTAL_BUDGET: usize = 16_384;
/// Intersections stop reading further indexes once at most this many
/// candidates remain; fetching them is cheaper than another index read.
pub const SMALL_CANDIDATE_SET: usize = 8;

/// An access path plus the filter its candidates must still satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAccess {
    /// Candidate-producing path.
    pub path: AccessPath,
    /// Filter checked on every stored, unchanged candidate row.
    pub verify: Option<Filter>,
}

/// Plans `filter` for index access.
///
/// Filters containing LIKE or JSON predicates can fail during evaluation, so
/// they keep the legacy single-column plan and residual: evaluating them only
/// on a different candidate set could hide or reveal errors. Every other
/// filter uses the access-path planner and is fully re-checked.
pub fn plan_access(
    filter: &Filter,
    indexes: &'static [IndexDef],
    columns: &'static [ColumnDef],
) -> Option<PlannedAccess> {
    if is_fallible(filter) {
        let analyzed = analyze_filter(filter, indexes)?;
        let path = legacy_path(&analyzed.plan, indexes)?;
        return Some(PlannedAccess {
            path,
            verify: analyzed.remaining_filter,
        });
    }
    plan_filter(filter, indexes, columns).map(|path| PlannedAccess {
        path,
        verify: Some(filter.clone()),
    })
}

/// Converts a legacy single-column plan into an access path.
fn legacy_path(plan: &IndexPlan, indexes: &'static [IndexDef]) -> Option<AccessPath> {
    let columns = indexes
        .iter()
        .map(IndexDef::columns)
        .find(|columns| *columns == [plan.column()])?;
    let path = match plan {
        IndexPlan::Eq { value, .. } => {
            AccessPath::Scan(IndexScan::prefix(columns, vec![value.clone()]))
        }
        IndexPlan::Range { start, end, .. } => {
            let bound =
                |value: &Option<Value>| value.clone().map_or(Bound::Unbounded, Bound::Included);
            AccessPath::Scan(IndexScan::range(
                columns,
                Vec::new(),
                bound(start),
                bound(end),
            ))
        }
        IndexPlan::In { values, .. } => {
            let mut values = values.clone();
            values.sort();
            values.dedup();
            AccessPath::Union(
                values
                    .into_iter()
                    .map(|value| AccessPath::Scan(IndexScan::prefix(columns, vec![value])))
                    .collect(),
            )
        }
    };
    Some(path)
}

/// Remaining candidate-address budget of one plan execution.
#[derive(Debug)]
pub struct Budget {
    per_scan: usize,
    remaining: usize,
    entries_read: u64,
}

impl Budget {
    /// Creates a budget of `per_scan` addresses per index range and `total`
    /// addresses overall.
    pub fn new(per_scan: usize, total: usize) -> Self {
        Self {
            per_scan,
            remaining: total,
            entries_read: 0,
        }
    }

    /// Creates the default budget: [`SCAN_BUDGET`] and [`TOTAL_BUDGET`].
    pub fn default_limits() -> Self {
        Self::new(SCAN_BUDGET, TOTAL_BUDGET)
    }

    /// Returns how many index entries the execution read.
    pub fn entries_read(&self) -> u64 {
        self.entries_read
    }
}

/// Returns the sorted, distinct persistent addresses produced by `path`, or
/// `None` when the budget runs out. Never returns a partial candidate set.
pub fn materialize<MA>(
    reader: &IndexReader<'_>,
    path: &AccessPath,
    budget: &mut Budget,
    mm: &mut MA,
) -> MemoryResult<Option<Vec<RecordAddress>>>
where
    MA: MemoryAccess,
{
    match path {
        AccessPath::Empty => Ok(Some(Vec::new())),
        AccessPath::Scan(scan) => {
            let limit = budget.per_scan.min(budget.remaining);
            match reader.addresses(scan, limit, mm)? {
                Some(mut addresses) => {
                    budget.remaining -= addresses.len();
                    budget.entries_read += addresses.len() as u64;
                    addresses.sort_unstable();
                    addresses.dedup();
                    Ok(Some(addresses))
                }
                None => {
                    // The cursor read `limit + 1` entries before giving up.
                    budget.remaining -= limit;
                    budget.entries_read += limit as u64 + 1;
                    Ok(None)
                }
            }
        }
        AccessPath::Union(paths) => {
            let mut union = Vec::new();
            for path in paths {
                match materialize(reader, path, budget, mm)? {
                    Some(addresses) => union.extend(addresses),
                    None => return Ok(None),
                }
            }
            union.sort_unstable();
            union.dedup();
            Ok(Some(union))
        }
        AccessPath::Intersection(paths) => {
            let mut current: Option<Vec<RecordAddress>> = None;
            for path in paths {
                if current
                    .as_ref()
                    .is_some_and(|set| set.len() <= SMALL_CANDIDATE_SET)
                {
                    break;
                }
                // An over-budget member is skipped: the others still bound a
                // superset of the matching rows.
                let Some(addresses) = materialize(reader, path, budget, mm)? else {
                    continue;
                };
                current = Some(match current {
                    Some(set) => intersect_sorted(&set, &addresses),
                    None => addresses,
                });
            }
            Ok(current)
        }
    }
}

/// Intersects two sorted, distinct address lists.
fn intersect_sorted(left: &[RecordAddress], right: &[RecordAddress]) -> Vec<RecordAddress> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(left.len().min(right.len()));
    while i < left.len() && j < right.len() {
        match left[i].cmp(&right[j]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                out.push(left[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use wasm_dbms_api::prelude::{ColumnDef, DataTypeKind, Filter, IndexDef, Text, Uint32, Value};
    use wasm_dbms_memory::prelude::{HeapMemoryProvider, MemoryAccess, MemoryManager};
    use wasm_dbms_memory::{IndexLedger, RecordAddress};

    use super::{Budget, PlannedAccess, materialize, plan_access};
    use crate::database::access_planner::AccessPath;
    use crate::database::index_reader::{IndexReader, IndexScan};

    const SHARD: &[&str] = &["shard"];
    const COHORT: &[&str] = &["cohort"];

    fn uint(value: u32) -> Value {
        Value::Uint32(Uint32(value))
    }

    /// 1,000 rows: `shard = i % 10`, `cohort = i / 100`; address page = `i`.
    fn setup() -> (MemoryManager<HeapMemoryProvider>, IndexLedger) {
        let mut mm = MemoryManager::init(HeapMemoryProvider::default());
        let page = mm.claim_page().unwrap();
        IndexLedger::init(page, &[IndexDef(SHARD), IndexDef(COHORT)], &mut mm).unwrap();
        let mut ledger = IndexLedger::load(page, &mut mm).unwrap();
        for i in 0..1_000u32 {
            let address = RecordAddress::new(i, 0);
            ledger
                .insert(SHARD, vec![uint(i % 10)], address, &mut mm)
                .unwrap();
            ledger
                .insert(COHORT, vec![uint(i / 100)], address, &mut mm)
                .unwrap();
        }
        (mm, ledger)
    }

    fn shard(value: u32) -> AccessPath {
        AccessPath::Scan(IndexScan::prefix(SHARD, vec![uint(value)]))
    }

    fn cohort(value: u32) -> AccessPath {
        AccessPath::Scan(IndexScan::prefix(COHORT, vec![uint(value)]))
    }

    fn shard_below(value: u32) -> AccessPath {
        AccessPath::Scan(IndexScan::range(
            SHARD,
            vec![],
            Bound::Unbounded,
            Bound::Excluded(uint(value)),
        ))
    }

    fn pages(addresses: Option<Vec<RecordAddress>>) -> Option<Vec<u32>> {
        addresses.map(|addresses| addresses.into_iter().map(|address| address.page).collect())
    }

    #[test]
    fn test_empty_union_and_intersection_materialize_sorted_distinct_addresses() {
        let (mut mm, ledger) = setup();
        let reader = IndexReader::new(&ledger);
        let mut budget = Budget::default_limits();
        assert_eq!(
            materialize(&reader, &AccessPath::Empty, &mut budget, &mut mm).unwrap(),
            Some(vec![])
        );

        let union = AccessPath::Union(vec![cohort(3), shard(1), cohort(3)]);
        let expected: Vec<u32> = (0..1_000).filter(|i| i / 100 == 3 || i % 10 == 1).collect();
        assert_eq!(
            pages(materialize(&reader, &union, &mut budget, &mut mm).unwrap()),
            Some(expected)
        );

        let intersection = AccessPath::Intersection(vec![shard(1), cohort(3)]);
        let expected: Vec<u32> = (300..400).filter(|i| i % 10 == 1).collect();
        assert_eq!(
            pages(materialize(&reader, &intersection, &mut budget, &mut mm).unwrap()),
            Some(expected)
        );
    }

    #[test]
    fn test_intersection_skips_an_over_budget_member_and_keeps_a_selective_one() {
        let (mut mm, ledger) = setup();
        let reader = IndexReader::new(&ledger);
        let mut budget = Budget::new(150, 1_000);
        let path = AccessPath::Intersection(vec![shard_below(5), cohort(3)]);
        let expected: Vec<u32> = (300..400).collect();
        assert_eq!(
            pages(materialize(&reader, &path, &mut budget, &mut mm).unwrap()),
            Some(expected)
        );
    }

    #[test]
    fn test_over_budget_paths_return_none_instead_of_partial_sets() {
        let (mut mm, ledger) = setup();
        let reader = IndexReader::new(&ledger);
        assert_eq!(
            materialize(
                &reader,
                &shard_below(5),
                &mut Budget::new(499, 10_000),
                &mut mm
            )
            .unwrap(),
            None
        );
        assert_eq!(
            materialize(
                &reader,
                &AccessPath::Union(vec![shard(1), shard(2)]),
                &mut Budget::new(1_000, 150),
                &mut mm
            )
            .unwrap(),
            None
        );
        assert_eq!(
            materialize(
                &reader,
                &AccessPath::Intersection(vec![shard_below(5), shard_below(6)]),
                &mut Budget::new(100, 10_000),
                &mut mm
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn test_intersection_stops_reading_once_the_candidate_set_is_small() {
        let (mut mm, ledger) = setup();
        let reader = IndexReader::new(&ledger);
        let selective = AccessPath::Scan(IndexScan::prefix(SHARD, vec![uint(99)]));
        let mut budget = Budget::default_limits();
        let path = AccessPath::Intersection(vec![selective, cohort(3)]);
        assert_eq!(
            materialize(&reader, &path, &mut budget, &mut mm).unwrap(),
            Some(vec![])
        );
        assert_eq!(budget.entries_read(), 0);
    }

    const COLUMNS: &[ColumnDef] = &[
        ColumnDef {
            name: "id",
            data_type: DataTypeKind::Uint32,
            auto_increment: false,
            nullable: false,
            primary_key: true,
            unique: false,
            foreign_key: None,
            default: None,
            renamed_from: &[],
        },
        ColumnDef {
            name: "name",
            data_type: DataTypeKind::Text,
            auto_increment: false,
            nullable: false,
            primary_key: false,
            unique: false,
            foreign_key: None,
            default: None,
            renamed_from: &[],
        },
    ];
    const INDEXES: &[IndexDef] = &[IndexDef(&["id"]), IndexDef(&["name"])];

    #[test]
    fn test_plan_access_keeps_the_legacy_plan_for_fallible_filters() {
        let name = Filter::eq("name", Value::Text(Text("a".to_string())));
        let like = Filter::like("name", "a%");

        let planned =
            plan_access(&like.clone().and(name.clone()), INDEXES, COLUMNS).expect("legacy plan");
        assert_eq!(
            planned,
            PlannedAccess {
                path: AccessPath::Scan(IndexScan::prefix(
                    &["name"],
                    vec![Value::Text(Text("a".to_string()))]
                )),
                verify: Some(like.clone()),
            }
        );
        assert_eq!(
            plan_access(&like.clone().or(name.clone()), INDEXES, COLUMNS),
            None
        );

        let plain = name.clone().and(Filter::eq("id", uint(1)));
        assert_eq!(
            plan_access(&plain, INDEXES, COLUMNS),
            Some(PlannedAccess {
                path: AccessPath::Scan(IndexScan::prefix(&["id"], vec![uint(1)])),
                verify: Some(plain.clone()),
            })
        );
    }
}

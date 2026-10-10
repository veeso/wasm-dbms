//! Deterministic fixtures and workloads for the index-planning benchmarks.
//!
//! The same fixtures and timed queries run before and after the planner
//! changes of issue #69, so every value is derived from fixed formulas.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_dbms::prelude::{DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DeleteBehavior, Filter, InsertRecord as _, Int32, MemoryResult,
    Nullable, Query, TableSchema, Text, TransactionId, Uint32, UpdateRecord as _, Value,
};
use wasm_dbms_macros::{DatabaseSchema, Table};
use wasm_dbms_memory::prelude::MemoryProvider;

use crate::provider::HashMapMemoryProvider;

/// Row counts measured by every read workload.
pub const ROW_COUNTS: [u32; 3] = [100, 10_000, 100_000];
/// Row count of the transaction and mutation fixtures.
pub const MUTATION_ROWS: u32 = 10_000;
/// Rows changed inside the transaction workloads.
pub const OVERLAY_CHANGES: [u32; 3] = [0, 10, 100];
/// Length of the non-indexed payload of every fixture row.
pub const PAYLOAD_LEN: usize = 512;

/// Composite-index fixture: `category` and `brand` share one index.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "catalog_items"]
pub struct CatalogItem {
    #[primary_key]
    pub id: Uint32,
    #[index(group = "idx_category_brand")]
    pub category: Uint32,
    #[index(group = "idx_category_brand")]
    pub brand: Uint32,
    pub payload: Text,
}

/// Union and intersection fixture: `shard` and `cohort` have separate indexes.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "shard_items"]
pub struct ShardItem {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub shard: Uint32,
    #[index]
    pub cohort: Uint32,
    pub payload: Text,
}

/// NULL fixture: nullable signed and text columns with separate indexes.
#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "nullable_items"]
pub struct NullableItem {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub signed: Nullable<Int32>,
    #[index]
    pub label: Nullable<Text>,
    pub payload: Text,
}

/// Schema holding every index-planning fixture table.
#[derive(Debug, Clone, Copy, DatabaseSchema)]
#[tables(
    CatalogItem = "catalog_items",
    ShardItem = "shard_items",
    NullableItem = "nullable_items"
)]
pub struct IndexPlanningSchema;

/// Data distribution of a fixture table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    /// `category = i % 100`, `brand = (i / 100) % 20`.
    CatalogUniform,
    /// The first 90% of rows are category 7 / brand 3, the rest uniform.
    CatalogSkewed,
    /// `shard = i % 100`, `cohort = (i / 100) % 100`.
    ShardIndependent,
    /// `shard = i % 100`, `cohort = shard`.
    ShardCorrelated,
    /// Both nullable columns are NULL when `i % 100` is below the percentage.
    Nullable(u32),
}

impl Fixture {
    /// Every fixture, in the order the benchmarks build them.
    pub const ALL: [Fixture; 7] = [
        Self::CatalogUniform,
        Self::CatalogSkewed,
        Self::ShardIndependent,
        Self::ShardCorrelated,
        Self::Nullable(1),
        Self::Nullable(50),
        Self::Nullable(99),
    ];

    /// Returns the table holding this fixture.
    pub fn table(self) -> &'static str {
        match self {
            Self::CatalogUniform | Self::CatalogSkewed => "catalog_items",
            Self::ShardIndependent | Self::ShardCorrelated => "shard_items",
            Self::Nullable(_) => "nullable_items",
        }
    }

    /// Returns the values of zero-based row `i` of a fixture with `rows` rows,
    /// in table column order.
    pub fn row_values(self, i: u32, rows: u32) -> Vec<Value> {
        let id = uint(i + 1);
        match self {
            Self::CatalogUniform => vec![id, uint(i % 100), uint((i / 100) % 20), payload()],
            Self::CatalogSkewed if i < rows / 10 * 9 => {
                vec![id, uint(7), uint(3), payload()]
            }
            Self::CatalogSkewed => Self::CatalogUniform.row_values(i, rows),
            Self::ShardIndependent => vec![id, uint(i % 100), uint((i / 100) % 100), payload()],
            Self::ShardCorrelated => vec![id, uint(i % 100), uint(i % 100), payload()],
            Self::Nullable(percent) if i % 100 < percent => {
                vec![id, Value::Null, Value::Null, payload()]
            }
            Self::Nullable(_) => vec![
                id,
                Value::Int32((-1 - (i % 1_000) as i32).into()),
                Value::Text(Text(format!("value_{}", i % 1_000))),
                payload(),
            ],
        }
    }
}

/// Returns an unsigned value.
pub fn uint(value: u32) -> Value {
    Value::Uint32(Uint32(value))
}

/// Returns the non-indexed payload shared by every fixture row.
pub fn payload() -> Value {
    Value::Text(Text("x".repeat(PAYLOAD_LEN)))
}

/// Pairs `values` with the columns of `T`, in table column order.
fn with_columns<T>(values: Vec<Value>) -> Vec<(ColumnDef, Value)>
where
    T: TableSchema,
{
    T::columns().iter().copied().zip(values).collect()
}

/// Builds a fixture table with `rows` rows on `provider`.
///
/// # Panics
///
/// Panics when registering the schema or inserting a row fails.
pub fn build_fixture<M>(provider: M, fixture: Fixture, rows: u32) -> DbmsContext<M>
where
    M: MemoryProvider,
{
    let ctx = DbmsContext::new(provider);
    IndexPlanningSchema::register_tables(&ctx).expect("failed to register fixture tables");
    let db = WasmDbmsDatabase::oneshot(&ctx, IndexPlanningSchema);
    for i in 0..rows {
        let values = fixture.row_values(i, rows);
        let result = match fixture {
            Fixture::CatalogUniform | Fixture::CatalogSkewed => db.insert::<CatalogItem>(
                CatalogItemInsertRequest::from_values(&with_columns::<CatalogItem>(values))
                    .expect("invalid catalog row"),
            ),
            Fixture::ShardIndependent | Fixture::ShardCorrelated => db.insert::<ShardItem>(
                ShardItemInsertRequest::from_values(&with_columns::<ShardItem>(values))
                    .expect("invalid shard row"),
            ),
            Fixture::Nullable(_) => db.insert::<NullableItem>(
                NullableItemInsertRequest::from_values(&with_columns::<NullableItem>(values))
                    .expect("invalid nullable row"),
            ),
        };
        result.expect("fixture insert failed");
    }
    drop(db);
    ctx
}

/// One timed read workload.
#[derive(Debug, Clone)]
pub struct ReadCase {
    /// Case name, the middle part of `index_planning/<case>/<rows>`.
    pub name: &'static str,
    /// Fixture the case reads.
    pub fixture: Fixture,
    /// Query timed by the benchmark.
    pub query: Query,
    /// Result sizes at 10,000 and 100,000 rows stated by the issue, if any.
    pub expected: Option<[usize; 2]>,
}

impl ReadCase {
    /// Returns the issue's expected result size at `rows` rows, if stated.
    pub fn expected_rows(&self, rows: u32) -> Option<usize> {
        match (self.expected, rows) {
            (Some([small, _]), 10_000) => Some(small),
            (Some([_, large]), 100_000) => Some(large),
            _ => None,
        }
    }
}

fn case(
    name: &'static str,
    fixture: Fixture,
    query: Query,
    expected: Option<[usize; 2]>,
) -> ReadCase {
    ReadCase {
        name,
        fixture,
        query,
        expected,
    }
}

fn eq(column: &str, value: u32) -> Filter {
    Filter::eq(column, uint(value))
}

fn filtered(filter: Filter) -> Query {
    Query::builder().all().filter(Some(filter)).build()
}

fn projected(fields: &[&'static str], filter: Option<Filter>) -> Query {
    Query::builder()
        .fields(fields.iter().copied())
        .filter(filter)
        .build()
}

/// Returns every timed read workload.
pub fn read_cases() -> Vec<ReadCase> {
    use Fixture::{CatalogSkewed, CatalogUniform, Nullable, ShardCorrelated, ShardIndependent};

    vec![
        case(
            "composite_exact",
            CatalogUniform,
            filtered(eq("category", 7).and(eq("brand", 3))),
            Some([5, 50]),
        ),
        case(
            "composite_exact_reversed",
            CatalogUniform,
            filtered(eq("brand", 3).and(eq("category", 7))),
            Some([5, 50]),
        ),
        case(
            "composite_exact_skewed",
            CatalogSkewed,
            filtered(eq("category", 7).and(eq("brand", 3))),
            None,
        ),
        case(
            "composite_prefix",
            CatalogUniform,
            filtered(eq("category", 7)),
            Some([100, 1_000]),
        ),
        case(
            "composite_prefix_range",
            CatalogUniform,
            filtered(
                eq("category", 7)
                    .and(Filter::ge("brand", uint(3)))
                    .and(Filter::lt("brand", uint(5))),
            ),
            Some([10, 100]),
        ),
        case(
            "or_same_column",
            ShardIndependent,
            filtered(eq("shard", 7).or(eq("shard", 8))),
            Some([200, 2_000]),
        ),
        case(
            "or_cross_column",
            ShardIndependent,
            filtered(eq("shard", 7).or(eq("cohort", 3))),
            Some([199, 1_990]),
        ),
        case(
            "intersection",
            ShardIndependent,
            filtered(eq("shard", 7).and(eq("cohort", 3))),
            Some([1, 10]),
        ),
        case(
            "intersection_reversed",
            ShardIndependent,
            filtered(eq("cohort", 3).and(eq("shard", 7))),
            Some([1, 10]),
        ),
        case(
            "intersection_correlated",
            ShardCorrelated,
            filtered(eq("shard", 7).and(eq("cohort", 7))),
            Some([100, 1_000]),
        ),
        case(
            "intersection_broad",
            ShardIndependent,
            filtered(Filter::lt("shard", uint(50)).and(eq("cohort", 3))),
            Some([50, 500]),
        ),
        case(
            "intersection_primary_key",
            ShardIndependent,
            filtered(eq("id", 508).and(eq("shard", 7))),
            Some([1, 1]),
        ),
        case(
            "is_null_sparse_signed",
            Nullable(1),
            filtered(Filter::is_null("signed")),
            Some([100, 1_000]),
        ),
        case(
            "is_null_sparse_text",
            Nullable(1),
            filtered(Filter::is_null("label")),
            Some([100, 1_000]),
        ),
        case(
            "not_null_sparse_signed",
            Nullable(99),
            filtered(Filter::not_null("signed")),
            Some([100, 1_000]),
        ),
        case(
            "not_null_sparse_text",
            Nullable(99),
            filtered(Filter::not_null("label")),
            Some([100, 1_000]),
        ),
        case(
            "is_null_dense_signed",
            Nullable(50),
            filtered(Filter::is_null("signed")),
            Some([5_000, 50_000]),
        ),
        case(
            "not_null_dense_text",
            Nullable(50),
            filtered(Filter::not_null("label")),
            Some([5_000, 50_000]),
        ),
        case(
            "covering_projection",
            CatalogUniform,
            projected(&["category", "brand"], Some(eq("category", 7))),
            Some([100, 1_000]),
        ),
        case(
            "covering_row_fetch_control",
            CatalogUniform,
            projected(&["category", "brand", "payload"], Some(eq("category", 7))),
            Some([100, 1_000]),
        ),
        case(
            "covering_unfiltered",
            CatalogUniform,
            projected(&["category", "brand"], None),
            Some([10_000, 100_000]),
        ),
        case(
            "covering_unfiltered_row_fetch_control",
            CatalogUniform,
            projected(&["category", "brand", "payload"], None),
            Some([10_000, 100_000]),
        ),
        case(
            "fallback_or",
            ShardIndependent,
            filtered(eq("shard", 7).or(Filter::eq("payload", payload()))),
            Some([10_000, 100_000]),
        ),
        case(
            "fallback_dense",
            ShardIndependent,
            filtered(Filter::lt("shard", uint(50))),
            Some([5_000, 50_000]),
        ),
        case(
            "empty_result_composite",
            CatalogUniform,
            filtered(eq("category", 100).and(eq("brand", 3))),
            Some([0, 0]),
        ),
        case(
            "empty_result_shard",
            ShardIndependent,
            filtered(eq("shard", 100)),
            Some([0, 0]),
        ),
    ]
}

/// Converts selected rows into sorted value lists for order-insensitive comparison.
pub fn sorted_values(rows: Vec<Vec<(ColumnDef, Value)>>) -> Vec<Vec<Value>> {
    let mut rows: Vec<Vec<Value>> = rows
        .into_iter()
        .map(|row| row.into_iter().map(|(_, value)| value).collect())
        .collect();
    rows.sort();
    rows
}

/// Returns the rows `query` must produce, computed by a full scan and the
/// filter evaluator, projected and sorted like [`sorted_values`].
///
/// # Panics
///
/// Panics when the scan or the filter evaluation fails.
pub fn scan_oracle<M>(db: &WasmDbmsDatabase<'_, M>, table: &str, query: &Query) -> Vec<Vec<Value>>
where
    M: MemoryProvider,
{
    let fields = query.raw_columns();
    let rows = db
        .select_raw(table, Query::builder().all().build())
        .expect("oracle scan failed")
        .into_iter()
        .filter(|row| {
            query
                .filter
                .as_ref()
                .is_none_or(|filter| filter.matches(row).expect("filter evaluation failed"))
        })
        .map(|row| {
            row.into_iter()
                .filter(|(column, _)| {
                    fields.is_empty() || fields.iter().any(|field| field.as_str() == column.name)
                })
                .collect()
        })
        .collect();
    sorted_values(rows)
}

/// Checks a read case against the scan oracle and the issue's row count.
///
/// # Panics
///
/// Panics when the result differs from the oracle or the stated count.
pub fn validate_read_case<M>(ctx: &DbmsContext<M>, case: &ReadCase, rows: u32)
where
    M: MemoryProvider,
{
    let db = WasmDbmsDatabase::oneshot(ctx, IndexPlanningSchema);
    let table = case.fixture.table();
    let actual = sorted_values(
        db.select_raw(table, case.query.clone())
            .expect("select failed"),
    );
    assert_eq!(
        actual,
        scan_oracle(&db, table, &case.query),
        "case {} differs from the scan oracle at {rows} rows",
        case.name
    );
    if let Some(count) = case.expected_rows(rows) {
        assert_eq!(
            actual.len(),
            count,
            "case {} returned an unexpected row count at {rows} rows",
            case.name
        );
    }
}

/// Queries timed inside an open transaction on the shard fixture.
pub fn overlay_queries() -> [(&'static str, Query); 2] {
    [
        (
            "overlay_union",
            filtered(eq("shard", 7).or(eq("cohort", 3))),
        ),
        (
            "overlay_intersection",
            filtered(eq("shard", 7).and(eq("cohort", 3))),
        ),
    ]
}

/// Opens a transaction on a shard fixture with `rows` rows and changes
/// `changes` rows.
///
/// Changes cycle through moving a row into the `shard = 7` branch, into the
/// `cohort = 3` branch, out of the `shard = 7` branch, and changing its
/// primary key.
///
/// # Panics
///
/// Panics when an update fails.
pub fn open_overlay<M>(ctx: &DbmsContext<M>, rows: u32, changes: u32) -> TransactionId
where
    M: MemoryProvider,
{
    let tx = ctx.begin_transaction();
    let db = WasmDbmsDatabase::from_transaction(ctx, IndexPlanningSchema, tx);
    for change in 0..changes {
        let id = (change * 97) % rows + 1;
        let (column, value) = match change % 4 {
            0 => (1, uint(7)),
            1 => (2, uint(3)),
            2 => (1, uint(8)),
            _ => (0, uint(rows + 1 + change)),
        };
        let patch = [(ShardItem::columns()[column], value)];
        let update = ShardItemUpdateRequest::from_values(&patch, Some(eq("id", id)))
            .expect("invalid overlay patch");
        db.update::<ShardItem>(update)
            .expect("overlay update failed");
    }
    tx
}

/// Replaces the payload of the `category = 7 AND brand = 3` catalog rows.
///
/// # Panics
///
/// Panics when the update fails.
pub fn update_composite<M>(ctx: &DbmsContext<M>) -> u64
where
    M: MemoryProvider,
{
    let db = WasmDbmsDatabase::oneshot(ctx, IndexPlanningSchema);
    let patch = [(
        CatalogItem::columns()[3],
        Value::Text(Text("y".repeat(PAYLOAD_LEN))),
    )];
    let update =
        CatalogItemUpdateRequest::from_values(&patch, Some(eq("category", 7).and(eq("brand", 3))))
            .expect("invalid composite patch");
    db.update::<CatalogItem>(update)
        .expect("composite update failed")
}

/// Deletes the `shard = 7 OR cohort = 3` shard rows.
///
/// # Panics
///
/// Panics when the delete fails.
pub fn delete_union<M>(ctx: &DbmsContext<M>) -> u64
where
    M: MemoryProvider,
{
    let db = WasmDbmsDatabase::oneshot(ctx, IndexPlanningSchema);
    db.delete::<ShardItem>(
        DeleteBehavior::Restrict,
        Some(eq("shard", 7).or(eq("cohort", 3))),
    )
    .expect("union delete failed")
}

/// Memory provider whose pages can be copied into an independent fixture.
///
/// Clones share the same pages, so a fixture built through one clone can be
/// snapshotted through another.
#[derive(Clone, Default)]
pub struct SharedMemoryProvider(Rc<RefCell<HashMapMemoryProvider>>);

impl SharedMemoryProvider {
    /// Wraps a copy of previously captured pages.
    pub fn from_snapshot(snapshot: HashMapMemoryProvider) -> Self {
        Self(Rc::new(RefCell::new(snapshot)))
    }

    /// Copies the current pages.
    pub fn snapshot(&self) -> HashMapMemoryProvider {
        self.0.borrow().clone()
    }
}

impl MemoryProvider for SharedMemoryProvider {
    const PAGE_SIZE: u64 = HashMapMemoryProvider::PAGE_SIZE;

    fn size(&self) -> u64 {
        self.0.borrow().size()
    }

    fn pages(&self) -> u64 {
        self.0.borrow().pages()
    }

    fn grow(&mut self, new_pages: u64) -> MemoryResult<u64> {
        self.0.borrow_mut().grow(new_pages)
    }

    fn read(&mut self, offset: u64, buf: &mut [u8]) -> MemoryResult<()> {
        self.0.borrow_mut().read(offset, buf)
    }

    fn write(&mut self, offset: u64, buf: &[u8]) -> MemoryResult<()> {
        self.0.borrow_mut().write(offset, buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixture_rows_follow_the_issue_formulas() {
        assert_eq!(
            Fixture::CatalogUniform.row_values(1_234, 10_000)[1..3],
            [uint(34), uint(12)]
        );
        assert_eq!(
            Fixture::CatalogSkewed.row_values(8_999, 10_000)[1..3],
            [uint(7), uint(3)]
        );
        assert_eq!(
            Fixture::CatalogSkewed.row_values(9_000, 10_000)[1..3],
            [uint(0), uint(10)]
        );
        assert_eq!(
            Fixture::ShardIndependent.row_values(1_234, 10_000)[1..3],
            [uint(34), uint(12)]
        );
        assert_eq!(
            Fixture::ShardCorrelated.row_values(1_234, 10_000)[1..3],
            [uint(34), uint(34)]
        );
        assert_eq!(
            Fixture::Nullable(50).row_values(1_249, 10_000)[1..3],
            [Value::Null, Value::Null]
        );
        assert_eq!(
            Fixture::Nullable(50).row_values(1_250, 10_000)[1..3],
            [
                Value::Int32((-251).into()),
                Value::Text(Text("value_250".to_string()))
            ]
        );
        let payload = &Fixture::CatalogUniform.row_values(0, 1)[3];
        assert_eq!(payload, &Value::Text(Text("x".repeat(PAYLOAD_LEN))));
    }

    #[test]
    fn test_every_read_case_matches_the_scan_oracle_on_small_fixtures() {
        let cases = read_cases();
        for fixture in Fixture::ALL {
            let ctx = build_fixture(HashMapMemoryProvider::default(), fixture, 1_000);
            for case in cases.iter().filter(|case| case.fixture == fixture) {
                validate_read_case(&ctx, case, 1_000);
            }
        }
    }

    #[test]
    fn test_read_case_names_are_unique() {
        let cases = read_cases();
        let mut names: Vec<_> = cases.iter().map(|case| case.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), cases.len());
    }

    #[test]
    fn test_overlay_queries_match_the_scan_oracle_inside_the_transaction() {
        let ctx = build_fixture(
            HashMapMemoryProvider::default(),
            Fixture::ShardIndependent,
            1_000,
        );
        let tx = open_overlay(&ctx, 1_000, 100);
        let db = WasmDbmsDatabase::from_transaction(&ctx, IndexPlanningSchema, tx);
        for (_, query) in overlay_queries() {
            let actual = sorted_values(db.select_raw("shard_items", query.clone()).unwrap());
            assert_eq!(actual, scan_oracle(&db, "shard_items", &query));
        }
    }

    #[test]
    fn test_mutations_on_snapshots_leave_the_source_fixture_untouched() {
        let source = SharedMemoryProvider::default();
        let catalog = build_fixture(source.clone(), Fixture::CatalogUniform, 1_000);
        drop(catalog);
        let snapshot = source.snapshot();

        let first = DbmsContext::new(SharedMemoryProvider::from_snapshot(snapshot.clone()));
        assert_eq!(update_composite(&first), 1);
        let second = DbmsContext::new(SharedMemoryProvider::from_snapshot(snapshot));
        assert_eq!(update_composite(&second), 1);

        let shards = SharedMemoryProvider::default();
        drop(build_fixture(
            shards.clone(),
            Fixture::ShardIndependent,
            1_000,
        ));
        let fresh = DbmsContext::new(SharedMemoryProvider::from_snapshot(shards.snapshot()));
        assert_eq!(delete_union(&fresh), 109);
        let again = DbmsContext::new(SharedMemoryProvider::from_snapshot(shards.snapshot()));
        assert_eq!(delete_union(&again), 109);
    }
}

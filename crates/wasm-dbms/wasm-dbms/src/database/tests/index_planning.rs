//! Differential tests for composite, union, intersection, NULL, and covering
//! index access paths. Every query is compared with a full-scan oracle, and
//! access counters prove which path ran.

use std::collections::HashSet;

use wasm_dbms_api::prelude::{
    ColumnDef, Database as _, DbmsError, DeleteBehavior, Filter, InsertRecord as _, Int32,
    Nullable, Query, QueryBuilder, QueryError, TableSchema, Text, Uint32, UpdateRecord as _, Value,
};
use wasm_dbms_macros::{DatabaseSchema, Table};
use wasm_dbms_memory::prelude::HeapMemoryProvider;

use crate::AccessStats;
use crate::prelude::{DbmsContext, WasmDbmsDatabase};

type Ctx = DbmsContext<HeapMemoryProvider>;
type Db<'ctx> = WasmDbmsDatabase<'ctx, HeapMemoryProvider>;
type Row = Vec<(ColumnDef, Value)>;

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "brands"]
pub struct Brand {
    #[primary_key]
    pub id: Uint32,
    pub name: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "products"]
pub struct Product {
    #[primary_key]
    pub id: Uint32,
    #[index(group = "idx_category_brand_price")]
    pub category: Text,
    #[index(group = "idx_category_brand_price")]
    #[foreign_key(entity = "Brand", table = "brands", column = "id")]
    pub brand: Uint32,
    #[index(group = "idx_category_brand_price")]
    pub price: Uint32,
    #[index]
    pub stock: Nullable<Int32>,
    #[index]
    pub label: Nullable<Text>,
    #[index]
    pub rank: Nullable<Uint32>,
    pub note: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "tickets"]
pub struct Ticket {
    #[primary_key]
    pub id: Uint32,
    #[index]
    pub shard: Uint32,
    #[index]
    pub cohort: Uint32,
    pub note: Text,
}

#[derive(Debug, Table, Clone, PartialEq, Eq)]
#[table = "readings"]
pub struct Reading {
    #[primary_key]
    pub id: Uint32,
    #[index(group = "idx_sensor_level")]
    pub sensor: Nullable<Text>,
    #[index(group = "idx_sensor_level")]
    pub level: Nullable<Int32>,
}

#[derive(DatabaseSchema)]
#[tables(
    Brand = "brands",
    Product = "products",
    Ticket = "tickets",
    Reading = "readings"
)]
pub struct PlanningSchema;

const PRODUCT_ROWS: u32 = 300;
const TICKET_ROWS: u32 = 200;
const READING_ROWS: u32 = 120;
const CATEGORIES: [&str; 5] = ["book", "game", "toy", "", "Ünïcødé"];

fn text(value: &str) -> Value {
    Value::Text(Text(value.to_string()))
}

fn uint(value: u32) -> Value {
    Value::Uint32(Uint32(value))
}

fn int(value: i32) -> Value {
    Value::Int32(value.into())
}

fn row<T>(values: Vec<Value>) -> Row
where
    T: TableSchema,
{
    T::columns().iter().copied().zip(values).collect()
}

/// Product row `i`: five categories (including empty and non-ASCII text),
/// four brands, fifty prices, and nullable columns on both sides of NULL.
fn product_values(i: u32) -> Vec<Value> {
    vec![
        uint(i + 1),
        text(CATEGORIES[(i % 5) as usize]),
        uint((i / 5) % 4),
        uint((i * 7) % 50),
        if i % 10 == 0 {
            Value::Null
        } else {
            int(6 - (i % 13) as i32)
        },
        if i % 3 == 0 {
            Value::Null
        } else {
            text(&format!("l{}", i % 7))
        },
        if i % 4 == 0 { Value::Null } else { uint(i % 9) },
        text(&format!("note {i}")),
    ]
}

fn ticket_values(i: u32) -> Vec<Value> {
    vec![
        uint(i + 1),
        uint(i % 10),
        uint((i / 10) % 10),
        text(&format!("t{i}")),
    ]
}

fn reading_values(i: u32) -> Vec<Value> {
    let sensor = match i % 4 {
        0 => Value::Null,
        1 => text("north"),
        2 => text("south"),
        _ => text(""),
    };
    let level = if i % 5 == 0 {
        Value::Null
    } else {
        int((i % 7) as i32 - 3)
    };
    vec![uint(i + 1), sensor, level]
}

fn insert_product(db: &Db<'_>, values: Vec<Value>) {
    db.insert::<Product>(ProductInsertRequest::from_values(&row::<Product>(values)).unwrap())
        .unwrap();
}

fn insert_ticket(db: &Db<'_>, values: Vec<Value>) {
    db.insert::<Ticket>(TicketInsertRequest::from_values(&row::<Ticket>(values)).unwrap())
        .unwrap();
}

/// A database holding brands and `rows` products built by `values`.
fn setup_products<F>(rows: u32, values: F) -> Ctx
where
    F: Fn(u32) -> Vec<Value>,
{
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    PlanningSchema::register_tables(&ctx).unwrap();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for id in 0..4 {
        db.insert::<Brand>(
            BrandInsertRequest::from_values(&row::<Brand>(vec![
                uint(id),
                text(&format!("brand {id}")),
            ]))
            .unwrap(),
        )
        .unwrap();
    }
    for i in 0..rows {
        insert_product(&db, values(i));
    }
    drop(db);
    ctx
}

/// A database holding every planning table.
fn setup() -> Ctx {
    let ctx = setup_products(PRODUCT_ROWS, product_values);
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for i in 0..TICKET_ROWS {
        insert_ticket(&db, ticket_values(i));
    }
    for i in 0..READING_ROWS {
        db.insert::<Reading>(
            ReadingInsertRequest::from_values(&row::<Reading>(reading_values(i))).unwrap(),
        )
        .unwrap();
    }
    drop(db);
    ctx
}

fn sorted(rows: Vec<Row>) -> Vec<Vec<Value>> {
    let mut rows: Vec<Vec<Value>> = rows
        .into_iter()
        .map(|row| row.into_iter().map(|(_, value)| value).collect())
        .collect();
    rows.sort();
    rows
}

fn select(db: &Db<'_>, table: &str, filter: &Filter) -> Vec<Row> {
    db.select_raw(
        table,
        Query::builder().all().filter(Some(filter.clone())).build(),
    )
    .unwrap()
}

/// Asserts that `filter` returns the rows of a full-scan oracle; returns how many.
fn assert_same_as_scan(db: &Db<'_>, table: &str, filter: &Filter) -> usize {
    let expected: Vec<Row> = db
        .select_raw(table, Query::builder().all().build())
        .unwrap()
        .into_iter()
        .filter(|row| filter.matches(row).unwrap())
        .collect();
    let actual = select(db, table, filter);
    assert_unique_ids(&actual);
    let count = actual.len();
    assert_eq!(
        sorted(actual),
        sorted(expected),
        "rows differ for {filter:?}"
    );
    count
}

/// Runs `filter` and returns the storage accesses it made.
fn stats_of(ctx: &Ctx, db: &Db<'_>, table: &str, filter: &Filter) -> AccessStats {
    ctx.reset_access_stats();
    select(db, table, filter);
    ctx.access_stats()
}

/// Asserts that `filter` matches the oracle without scanning the table and
/// fetches `fetches` records, or exactly the matching records when `None`.
fn assert_index_path(
    ctx: &Ctx,
    db: &Db<'_>,
    table: &str,
    filter: &Filter,
    fetches: Option<u64>,
) -> usize {
    let matches = assert_same_as_scan(db, table, filter);
    let stats = stats_of(ctx, db, table, filter);
    assert_eq!(stats.scanned_rows, 0, "{filter:?} scanned the table");
    assert_eq!(
        stats.record_fetches,
        fetches.unwrap_or(matches as u64),
        "record fetches for {filter:?}"
    );
    matches
}

/// Asserts that `filter` matches the oracle through a full scan.
fn assert_scan_path(ctx: &Ctx, db: &Db<'_>, table: &str, filter: &Filter) {
    assert_same_as_scan(db, table, filter);
    let stats = stats_of(ctx, db, table, filter);
    assert!(stats.scanned_rows > 0, "{filter:?} did not scan");
    assert_eq!(stats.record_fetches, 0, "{filter:?} fetched by address");
}

fn assert_unique_ids(rows: &[Row]) {
    let mut seen = HashSet::new();
    for row in rows {
        let id = row
            .iter()
            .find(|(column, _)| column.primary_key)
            .map(|(_, value)| value.clone())
            .expect("primary key selected");
        assert!(seen.insert(id.clone()), "row {id:?} returned twice");
    }
}

/// Applies `patch` (column position, value) to the rows of `T` matching `filter`.
fn update_rows<T>(db: &Db<'_>, patch: &[(usize, Value)], filter: Filter) -> u64
where
    T: TableSchema,
    T::Update: wasm_dbms_api::prelude::UpdateRecord<Schema = T>,
{
    let patch: Vec<(ColumnDef, Value)> = patch
        .iter()
        .map(|(column, value)| (T::columns()[*column], value.clone()))
        .collect();
    db.update::<T>(T::Update::from_values(&patch, Some(filter)).unwrap())
        .unwrap()
}

fn category(value: &str) -> Filter {
    Filter::eq("category", text(value))
}

fn brand(value: u32) -> Filter {
    Filter::eq("brand", uint(value))
}

fn price(value: u32) -> Filter {
    Filter::eq("price", uint(value))
}

// -- composite indexes ------------------------------------------------------

#[test]
fn test_composite_exact_equality_fetches_only_matches() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        category("game").and(brand(1)).and(price(42)),
        price(42).and(brand(1).and(category("game"))),
        brand(2).and(category("")).and(price(35)),
    ] {
        assert_index_path(&ctx, &db, "products", &filter, None);
    }
    assert!(
        assert_same_as_scan(
            &db,
            "products",
            &category("game").and(brand(1)).and(price(42))
        ) > 0
    );
}

#[test]
fn test_composite_prefix_and_range_fetch_only_matches() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        category("game"),
        category(""),
        category("Ünïcødé").and(brand(2)),
        category("game").and(brand(1)),
        category("game")
            .and(Filter::ge("brand", uint(1)))
            .and(Filter::lt("brand", uint(3))),
        category("toy")
            .and(Filter::gt("brand", uint(1)))
            .and(Filter::le("brand", uint(3))),
        category("book")
            .and(brand(0))
            .and(Filter::ge("price", uint(10)))
            .and(Filter::lt("price", uint(40))),
        category("missing"),
    ] {
        assert_index_path(&ctx, &db, "products", &filter, None);
    }
}

#[test]
fn test_composite_gap_and_trailing_predicates() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let game_rows = assert_same_as_scan(&db, "products", &category("game")) as u64;
    assert_index_path(
        &ctx,
        &db,
        "products",
        &category("game").and(price(42)),
        Some(game_rows),
    );
    assert_scan_path(&ctx, &db, "products", &brand(1));
    assert_scan_path(&ctx, &db, "products", &price(42));
}

#[test]
fn test_contradictions_return_nothing_without_reading() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        brand(1).and(brand(2)),
        Filter::in_list("category", vec![]),
        Filter::ge("price", uint(5)).and(Filter::lt("price", uint(5))),
        category("game")
            .and(Filter::gt("brand", uint(2)))
            .and(Filter::le("brand", uint(2))),
    ] {
        assert_eq!(assert_same_as_scan(&db, "products", &filter), 0);
        assert_eq!(
            stats_of(&ctx, &db, "products", &filter),
            AccessStats::default(),
            "{filter:?}"
        );
    }
}

#[test]
fn test_in_lists_on_a_composite_prefix() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    assert_index_path(
        &ctx,
        &db,
        "products",
        &Filter::in_list("category", vec![text("game"), text("book"), text("game")]).and(brand(1)),
        None,
    );
    let game_rows = assert_same_as_scan(&db, "products", &category("game")) as u64;
    let many_brands = Filter::in_list("brand", (0..65).map(uint).collect());
    assert_index_path(
        &ctx,
        &db,
        "products",
        &category("game").and(many_brands),
        Some(game_rows),
    );
}

#[test]
fn test_update_and_delete_with_composite_filters_target_oracle_rows() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);

    let target = category("game").and(brand(1));
    let expected = assert_same_as_scan(&db, "products", &target) as u64;
    assert_eq!(
        update_rows::<Product>(&db, &[(7, text("patched"))], target.clone()),
        expected
    );
    assert_eq!(
        assert_same_as_scan(
            &db,
            "products",
            &target.clone().and(Filter::eq("note", text("patched")))
        ) as u64,
        expected
    );

    let moved = category("toy").and(brand(2));
    let moved_count = assert_same_as_scan(&db, "products", &moved) as u64;
    assert_eq!(
        update_rows::<Product>(&db, &[(3, uint(99))], moved.clone()),
        moved_count
    );
    assert_index_path(&ctx, &db, "products", &moved.clone().and(price(99)), None);
    assert_eq!(
        assert_same_as_scan(&db, "products", &moved.and(Filter::lt("price", uint(99)))),
        0
    );

    let deleted = category("book").and(Filter::ge("brand", uint(2)));
    let deleted_count = assert_same_as_scan(&db, "products", &deleted) as u64;
    let before = assert_same_as_scan(&db, "products", &category("book"));
    assert_eq!(
        db.delete::<Product>(DeleteBehavior::Restrict, Some(deleted.clone()))
            .unwrap(),
        deleted_count
    );
    assert_eq!(assert_same_as_scan(&db, "products", &deleted), 0);
    assert_eq!(
        assert_same_as_scan(&db, "products", &category("book")) as u64,
        before as u64 - deleted_count
    );
}

#[test]
fn test_transaction_reads_reconcile_composite_paths() {
    let ctx = setup();
    let filters = [
        category("game"),
        category("game").and(brand(1)),
        category("game")
            .and(brand(1))
            .and(Filter::ge("price", uint(20))),
        category("toy").and(brand(2)),
    ];

    for commit in [false, true] {
        let tx = ctx.begin_transaction();
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
        // move a game row within its key, move a toy row into the game prefix,
        // change only a residual column, delete, insert, and change a primary key
        update_rows::<Product>(&db, &[(3, uint(49))], Filter::eq("id", uint(7)));
        update_rows::<Product>(&db, &[(1, text("game"))], Filter::eq("id", uint(3)));
        update_rows::<Product>(&db, &[(7, text("residual"))], Filter::eq("id", uint(12)));
        db.delete::<Product>(DeleteBehavior::Restrict, Some(Filter::eq("id", uint(17))))
            .unwrap();
        let mut inserted = product_values(1);
        inserted[0] = uint(5_000);
        insert_product(&db, inserted);
        update_rows::<Product>(&db, &[(0, uint(6_000))], Filter::eq("id", uint(22)));

        for filter in &filters {
            assert_same_as_scan(&db, "products", filter);
        }
        if commit {
            db.commit().unwrap();
        } else {
            db.rollback().unwrap();
        }

        let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
        for filter in &filters {
            assert_index_path(&ctx, &db, "products", filter, None);
        }
        let committed = !select(&db, "products", &Filter::eq("id", uint(6_000))).is_empty();
        assert_eq!(committed, commit);
    }
}
// -- OR unions and AND intersections ----------------------------------------

fn shard(value: u32) -> Filter {
    Filter::eq("shard", uint(value))
}

fn cohort(value: u32) -> Filter {
    Filter::eq("cohort", uint(value))
}

#[test]
fn test_or_unions_index_branches_without_duplicates() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        shard(1).or(shard(2)),
        shard(1).or(cohort(2)),
        shard(1).and(cohort(2)).or(shard(3).and(cohort(4))),
        shard(1).or(shard(1)),
    ] {
        assert_index_path(&ctx, &db, "tickets", &filter, None);
    }
    assert_index_path(
        &ctx,
        &db,
        "products",
        &category("game").and(brand(1)).or(category("toy")),
        None,
    );
}

#[test]
fn test_or_with_an_unindexable_branch_never_loses_rows() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    assert_scan_path(
        &ctx,
        &db,
        "tickets",
        &shard(1).or(Filter::eq("note", text("t5"))),
    );
    assert_scan_path(&ctx, &db, "tickets", &shard(1).or(cohort(2).not()));
    let cohort_rows = assert_same_as_scan(&db, "tickets", &cohort(3)) as u64;
    assert_index_path(
        &ctx,
        &db,
        "tickets",
        &cohort(3).and(shard(1).or(Filter::eq("note", text("t31")))),
        Some(cohort_rows),
    );
}

#[test]
fn test_or_over_the_alternative_cap_still_matches_the_oracle() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let or_of = |count: u32| {
        (0..count)
            .map(|id| Filter::eq("id", uint(id + 1)))
            .reduce(Filter::or)
            .unwrap()
    };
    assert_index_path(&ctx, &db, "tickets", &or_of(64), None);
    assert_scan_path(&ctx, &db, "tickets", &or_of(65));
}

#[test]
fn test_and_intersects_separate_indexes_before_fetching() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        shard(1).and(cohort(2)),
        cohort(2).and(shard(1)),
        shard(1).and(cohort(1)),
        Filter::lt("shard", uint(5)).and(cohort(2)),
        shard(1).and(cohort(100)),
    ] {
        assert_index_path(&ctx, &db, "tickets", &filter, None);
    }
}

#[test]
fn test_primary_key_equality_is_read_alone() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let filter = Filter::eq("id", uint(22)).and(shard(1));
    assert_eq!(assert_index_path(&ctx, &db, "tickets", &filter, None), 1);
    assert_eq!(stats_of(&ctx, &db, "tickets", &filter).index_entries, 1);
}

#[test]
fn test_mutations_with_union_and_intersection_targets() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);

    let union = shard(1).or(cohort(2));
    let union_count = assert_same_as_scan(&db, "tickets", &union) as u64;
    assert_eq!(
        update_rows::<Ticket>(&db, &[(3, text("hit"))], union.clone()),
        union_count
    );
    assert_eq!(
        assert_same_as_scan(&db, "tickets", &Filter::eq("note", text("hit"))) as u64,
        union_count
    );

    let intersection = shard(3).and(cohort(4));
    let intersection_count = assert_same_as_scan(&db, "tickets", &intersection) as u64;
    assert_eq!(
        db.delete::<Ticket>(DeleteBehavior::Restrict, Some(intersection.clone()))
            .unwrap(),
        intersection_count
    );
    assert_eq!(assert_same_as_scan(&db, "tickets", &intersection), 0);
    assert_index_path(&ctx, &db, "tickets", &shard(3), None);
}

#[test]
fn test_union_delete_restores_a_row_deleted_before_a_constraint_failure() {
    let ctx = DbmsContext::new(HeapMemoryProvider::default());
    PlanningSchema::register_tables(&ctx).unwrap();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    // Physical address order makes the unreferenced brand the first target.
    for id in [100, 1] {
        db.insert::<Brand>(
            BrandInsertRequest::from_values(&row::<Brand>(vec![uint(id), text("brand")])).unwrap(),
        )
        .unwrap();
    }
    let mut product = product_values(0);
    product[2] = uint(1);
    insert_product(&db, product);
    let filter = Filter::eq("id", uint(100)).or(Filter::eq("id", uint(1)));
    let before = select(&db, "brands", &filter);
    ctx.reset_access_stats();
    let error = db
        .delete::<Brand>(DeleteBehavior::Restrict, Some(filter.clone()))
        .unwrap_err();
    assert!(
        matches!(error, DbmsError::Query(QueryError::ForeignKeyConstraintViolation { referencing_table, field }) if referencing_table == "brands" && field == "id")
    );
    assert!(ctx.access_stats().record_fetches >= 2);
    assert_eq!(sorted(select(&db, "brands", &filter)), sorted(before));
}

#[test]
fn test_transaction_primary_key_read_ignores_unrelated_payload_updates() {
    let ctx = setup();
    let tx = ctx.begin_transaction();
    let db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
    assert_eq!(
        update_rows::<Ticket>(&db, &[(3, text("changed"))], Filter::gt("id", uint(1))),
        199
    );
    let stats = stats_of(&ctx, &db, "tickets", &Filter::eq("id", uint(1)));
    assert_eq!(stats.record_fetches, 1);
    assert_eq!(stats.index_entries, 1);
    assert_eq!(stats.scanned_rows, 0);
}

#[test]
fn test_transaction_fallible_residual_ignores_rows_outside_the_index_path() {
    let ctx = setup();
    let tx = ctx.begin_transaction();
    let db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
    update_rows::<Ticket>(&db, &[(0, uint(1_000))], Filter::eq("id", uint(1)));
    insert_ticket(&db, vec![uint(2_000), uint(1), uint(2), text("inserted")]);
    let filter = Filter::like("id", "x%").and(Filter::eq("id", uint(3_000)));
    assert!(
        db.select_raw(
            "tickets",
            Query::builder().all().filter(Some(filter)).build()
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn test_transaction_fallible_residual_still_errors_on_matching_keys() {
    let ctx = setup();
    let tx = ctx.begin_transaction();
    let db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
    update_rows::<Ticket>(&db, &[(0, uint(1_000))], Filter::eq("id", uint(1)));
    insert_ticket(&db, vec![uint(2_000), uint(1), uint(2), text("inserted")]);
    // Legacy GT scans include their boundary and leave GT in the residual.
    for indexed in [
        Filter::eq("id", uint(1_000)),
        Filter::eq("id", uint(2_000)),
        Filter::gt("id", uint(2_000)),
    ] {
        let filter = Filter::like("id", "x%").and(indexed);
        let error = db
            .select_raw(
                "tickets",
                Query::builder().all().filter(Some(filter)).build(),
            )
            .unwrap_err();
        assert!(
            matches!(error, DbmsError::Query(QueryError::InvalidQuery(message)) if message == "LIKE operator can only be applied to Text values")
        );
    }
}

#[test]
fn test_covering_limit_counts_qualifying_rows_after_residual_filtering() {
    let ctx = setup_products(5, |i| {
        let mut values = product_values(i);
        values[1] = text("game");
        values[2] = uint(0);
        values[3] = uint(i);
        values
    });
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let query = Query::builder()
        .fields(COVERED)
        .filter(Some(category("game").and(Filter::ge("price", uint(3)))))
        .limit(1)
        .build();
    let (rows, stats) = run(&ctx, &db, "products", query);
    assert_eq!(rows, vec![vec![text("game"), uint(0), uint(3)]]);
    assert_eq!(stats.index_entries, 4);
    assert_eq!((stats.record_fetches, stats.scanned_rows), (0, 0));
}

#[test]
fn test_covering_pagination_stops_after_the_requested_matches() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for (filter, offset, limit, entries) in [
        (None, 3, 1, 4),
        (Some(category("game")), 2, 1, 3),
        (Some(category("game")), 0, 0, 0),
    ] {
        let query = Query::builder()
            .fields(COVERED)
            .filter(filter)
            .offset(offset)
            .limit(limit)
            .build();
        let (rows, stats) = run(&ctx, &db, "products", query);
        assert_eq!(rows.len(), limit);
        assert_eq!(stats.index_entries, entries);
        assert_eq!((stats.record_fetches, stats.scanned_rows), (0, 0));
    }
}

#[test]
fn test_transaction_composed_plans_reconcile_moved_rows() {
    let ctx = setup();
    let filters = [
        shard(1).or(cohort(2)),
        shard(1).and(cohort(2)),
        shard(1).or(shard(2)),
        shard(1).and(cohort(6)).or(cohort(2)),
        shard(7).and(cohort(2)),
    ];

    for commit in [false, true] {
        let tx = ctx.begin_transaction();
        let mut db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
        insert_ticket(&db, vec![uint(1_000), uint(1), uint(9), text("new")]);
        update_rows::<Ticket>(&db, &[(1, uint(1))], Filter::eq("id", uint(4))); // into shard 1
        update_rows::<Ticket>(&db, &[(1, uint(5))], Filter::eq("id", uint(2))); // out of shard 1
        update_rows::<Ticket>(&db, &[(3, text("residual"))], Filter::eq("id", uint(12)));
        db.delete::<Ticket>(DeleteBehavior::Restrict, Some(Filter::eq("id", uint(32))))
            .unwrap();
        db.delete::<Ticket>(DeleteBehavior::Restrict, Some(Filter::eq("id", uint(42))))
            .unwrap();
        insert_ticket(&db, vec![uint(42), uint(2), uint(2), text("reinserted")]);
        update_rows::<Ticket>(&db, &[(0, uint(2_000))], Filter::eq("id", uint(52)));
        // leaves the shard branch and enters the cohort branch
        update_rows::<Ticket>(
            &db,
            &[(1, uint(7)), (2, uint(2))],
            Filter::eq("id", uint(62)),
        );

        for filter in &filters {
            assert_same_as_scan(&db, "tickets", filter);
        }
        if commit {
            db.commit().unwrap();
        } else {
            db.rollback().unwrap();
        }

        let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
        for filter in &filters {
            assert_index_path(&ctx, &db, "tickets", filter, None);
        }
    }
}
// -- NULL predicates ----------------------------------------------------------

#[test]
fn test_null_predicates_use_indexes_on_both_sides_of_null() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for column in ["stock", "label", "rank"] {
        assert!(assert_index_path(&ctx, &db, "products", &Filter::is_null(column), None) > 0);
        assert!(assert_index_path(&ctx, &db, "products", &Filter::not_null(column), None) > 0);
    }
}

#[test]
fn test_null_predicates_with_no_and_only_null_rows() {
    for nulls in [false, true] {
        let ctx = setup_products(40, |i| {
            let mut values = product_values(i);
            for column in 4..7 {
                values[column] = match (nulls, column) {
                    (true, _) => Value::Null,
                    (false, 4) => int(i as i32 - 20),
                    (false, 5) => text(&format!("l{i}")),
                    (false, _) => uint(i),
                };
            }
            values
        });
        let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
        for column in ["stock", "label", "rank"] {
            let is_null = assert_index_path(&ctx, &db, "products", &Filter::is_null(column), None);
            let not_null =
                assert_index_path(&ctx, &db, "products", &Filter::not_null(column), None);
            assert_eq!(
                (is_null, not_null),
                if nulls { (40, 0) } else { (0, 40) },
                "{column}"
            );
        }
    }
}

#[test]
fn test_null_predicates_on_a_composite_prefix() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    for filter in [
        Filter::is_null("sensor"),
        Filter::is_null("sensor").and(Filter::is_null("level")),
        Filter::eq("sensor", text("north")).and(Filter::not_null("level")),
        Filter::eq("sensor", text("")).and(Filter::is_null("level")),
        Filter::not_null("sensor"),
    ] {
        assert_index_path(&ctx, &db, "readings", &filter, None);
    }
    assert_scan_path(&ctx, &db, "readings", &Filter::is_null("level"));
}

#[test]
fn test_mutations_with_null_predicates_target_oracle_rows() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let readings = Filter::is_null("sensor").and(Filter::is_null("level"));
    let count = assert_same_as_scan(&db, "readings", &readings) as u64;
    assert_eq!(
        update_rows::<Reading>(&db, &[(2, int(0))], readings.clone()),
        count
    );
    assert_eq!(assert_same_as_scan(&db, "readings", &readings), 0);
    assert_index_path(
        &ctx,
        &db,
        "readings",
        &Filter::is_null("sensor").and(Filter::eq("level", int(0))),
        None,
    );

    let products = Filter::is_null("label").and(brand(2));
    let count = assert_same_as_scan(&db, "products", &products) as u64;
    assert_eq!(
        db.delete::<Product>(DeleteBehavior::Restrict, Some(products.clone()))
            .unwrap(),
        count
    );
    assert_eq!(assert_same_as_scan(&db, "products", &products), 0);
}

// -- covering reads -----------------------------------------------------------

fn values(rows: Vec<Row>) -> Vec<Vec<Value>> {
    rows.into_iter()
        .map(|row| row.into_iter().map(|(_, value)| value).collect())
        .collect()
}

/// Runs `query` and returns its values and storage accesses.
fn run(ctx: &Ctx, db: &Db<'_>, table: &str, query: Query) -> (Vec<Vec<Value>>, AccessStats) {
    ctx.reset_access_stats();
    let rows = db.select_raw(table, query).unwrap();
    (values(rows), ctx.access_stats())
}

/// Computes projected rows with a full scan and the filter evaluator.
fn projected_oracle(
    db: &Db<'_>,
    table: &str,
    fields: &[&str],
    filter: Option<&Filter>,
) -> Vec<Vec<Value>> {
    let mut rows: Vec<Vec<Value>> = db
        .select_raw(table, Query::builder().all().build())
        .unwrap()
        .into_iter()
        .filter(|row| filter.is_none_or(|filter| filter.matches(row).unwrap()))
        .map(|row| {
            row.into_iter()
                .filter(|(column, _)| fields.contains(&column.name))
                .map(|(_, value)| value)
                .collect()
        })
        .collect();
    rows.sort();
    rows
}

fn sorted_values(mut rows: Vec<Vec<Value>>) -> Vec<Vec<Value>> {
    rows.sort();
    rows
}

const COVERED: [&str; 3] = ["category", "brand", "price"];

#[test]
fn test_covering_projection_reads_no_records_and_keeps_duplicates() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let filter = category("game");
    for fields in [&COVERED[..], &["category", "brand"]] {
        let query = Query::builder()
            .fields(fields.iter().copied())
            .filter(Some(filter.clone()))
            .build();
        let (rows, stats) = run(&ctx, &db, "products", query);
        assert_eq!(stats.record_fetches, 0);
        assert_eq!(stats.scanned_rows, 0);
        assert!(stats.index_entries > 0);
        assert_eq!(
            sorted_values(rows),
            projected_oracle(&db, "products", fields, Some(&filter))
        );
    }

    let unfiltered = Query::builder().fields(COVERED).build();
    let (rows, stats) = run(&ctx, &db, "products", unfiltered);
    assert_eq!((stats.record_fetches, stats.scanned_rows), (0, 0));
    assert_eq!(rows.len(), PRODUCT_ROWS as usize);
    assert_eq!(
        sorted_values(rows),
        projected_oracle(&db, "products", &COVERED, None)
    );

    let (rows, stats) = run(
        &ctx,
        &db,
        "brands",
        Query::builder()
            .field("id")
            .filter(Some(Filter::eq("id", uint(1))))
            .build(),
    );
    assert_eq!(rows, vec![vec![uint(1)]]);
    assert_eq!((stats.record_fetches, stats.scanned_rows), (0, 0));
}

#[test]
fn test_covering_projection_keeps_query_post_processing() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let covered = || {
        Query::builder()
            .fields(["category", "brand"])
            .filter(Some(category("game")))
    };
    let fetched = || {
        Query::builder()
            .fields(["category", "brand", "note"])
            .filter(Some(category("game")))
    };
    let shapes: [(fn(QueryBuilder) -> QueryBuilder, &str); 4] = [
        (
            |query| query.distinct(&["brand"]).order_by_asc("brand"),
            "distinct",
        ),
        (
            |query| query.order_by_desc("brand").offset(3).limit(5),
            "order offset limit",
        ),
        (|query| query.limit(0), "zero limit"),
        (
            |query| query.order_by_asc("brand").offset(1_000),
            "offset past the end",
        ),
    ];
    for (shape, name) in shapes {
        let (covered_rows, stats) = run(&ctx, &db, "products", shape(covered()).build());
        assert_eq!(stats.record_fetches, 0, "{name}");
        let (fetched_rows, _) = run(&ctx, &db, "products", shape(fetched()).build());
        let fetched_rows: Vec<Vec<Value>> = fetched_rows
            .into_iter()
            .map(|row| row[..2].to_vec())
            .collect();
        assert_eq!(covered_rows, fetched_rows, "{name}");
    }
}

#[test]
fn test_uncovered_or_ineligible_queries_fetch_records() {
    let ctx = setup();
    let db = WasmDbmsDatabase::oneshot(&ctx, PlanningSchema);
    let game = || Some(category("game"));
    let cases = [
        (
            "primary key not in the index",
            Query::builder()
                .fields(["id", "category"])
                .filter(game())
                .build(),
        ),
        (
            "uncovered residual",
            Query::builder()
                .fields(["category", "brand"])
                .filter(Some(
                    category("game").and(Filter::eq("note", text("note 6"))),
                ))
                .build(),
        ),
        (
            "uncovered order",
            Query::builder()
                .fields(["category", "brand"])
                .filter(game())
                .order_by_asc("note")
                .build(),
        ),
        (
            "composed plan",
            Query::builder()
                .fields(["category", "brand"])
                .filter(Some(category("game").or(category("toy"))))
                .build(),
        ),
        (
            "eager relation",
            Query::builder()
                .fields(["category", "brand"])
                .with("brands")
                .filter(game())
                .build(),
        ),
        ("all columns", Query::builder().all().filter(game()).build()),
    ];
    for (name, query) in cases {
        let fields = query.raw_columns().to_vec();
        let (rows, stats) = run(&ctx, &db, "products", query.clone());
        assert!(stats.record_fetches > 0, "{name} must fetch records");
        if name != "eager relation" {
            let fields: Vec<&str> = fields.iter().map(String::as_str).collect();
            let fields: Vec<&str> = if fields.is_empty() {
                Product::columns()
                    .iter()
                    .map(|column| column.name)
                    .collect()
            } else {
                fields
            };
            assert_eq!(
                sorted_values(rows),
                projected_oracle(&db, "products", &fields, query.filter.as_ref()),
                "{name}"
            );
        }
    }

    let eager = db
        .select_raw(
            "products",
            Query::builder()
                .fields(["category", "brand"])
                .with("brands")
                .filter(game())
                .build(),
        )
        .unwrap();
    assert!(!eager.is_empty());
    let typed = db
        .select::<Product>(
            Query::builder()
                .fields(["category", "brand"])
                .with("brands")
                .filter(game())
                .build(),
        )
        .unwrap();
    assert_eq!(typed.len(), eager.len());

    let tx = ctx.begin_transaction();
    let tx_db = WasmDbmsDatabase::from_transaction(&ctx, PlanningSchema, tx);
    let query = Query::builder().fields(COVERED).filter(game()).build();
    let (rows, stats) = run(&ctx, &tx_db, "products", query);
    assert!(stats.record_fetches > 0, "transactions use row fetches");
    assert_eq!(
        sorted_values(rows),
        projected_oracle(&tx_db, "products", &COVERED, Some(&category("game")))
    );
}

//! Index-planning benchmarks for issue #69.
//!
//! Fixtures are built and validated outside the timed loops. Timed work is
//! planning, index access, record access, and result materialization.

use std::hint::black_box;
use std::time::Duration;

use criterion::measurement::WallTime;
use criterion::{
    BatchSize, BenchmarkGroup, BenchmarkId, Criterion, criterion_group, criterion_main,
};
use wasm_dbms::prelude::{DbmsContext, WasmDbmsDatabase};
use wasm_dbms_api::prelude::Database as _;
use wasm_dbms_bench::index_planning::{
    Fixture, IndexPlanningSchema, MUTATION_ROWS, OVERLAY_CHANGES, ROW_COUNTS, SharedMemoryProvider,
    build_fixture, delete_union, open_overlay, overlay_queries, read_cases, scan_oracle,
    sorted_values, update_composite, validate_read_case,
};
use wasm_dbms_bench::provider::HashMapMemoryProvider;

fn configure(group: &mut BenchmarkGroup<'_, WallTime>) {
    group
        .sample_size(30)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(10))
        .noise_threshold(0.05);
}

fn bench_reads(c: &mut Criterion) {
    let cases = read_cases();
    let mut group = c.benchmark_group("index_planning");
    configure(&mut group);
    for rows in ROW_COUNTS {
        for fixture in Fixture::ALL {
            let fixture_cases: Vec<_> = cases
                .iter()
                .filter(|case| case.fixture == fixture)
                .collect();
            if fixture_cases.is_empty() {
                continue;
            }
            let ctx = build_fixture(HashMapMemoryProvider::default(), fixture, rows);
            for case in fixture_cases {
                validate_read_case(&ctx, case, rows);
                let table = fixture.table();
                group.bench_with_input(
                    BenchmarkId::new(case.name, rows),
                    &case.query,
                    |b, query| {
                        b.iter(|| {
                            let db = WasmDbmsDatabase::oneshot(&ctx, IndexPlanningSchema);
                            black_box(
                                db.select_raw(black_box(table), black_box(query.clone()))
                                    .expect("select failed"),
                            )
                        });
                    },
                );
            }
        }
    }
    group.finish();
}

fn bench_overlay(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_planning");
    configure(&mut group);
    for changes in OVERLAY_CHANGES {
        let ctx = build_fixture(
            HashMapMemoryProvider::default(),
            Fixture::ShardIndependent,
            MUTATION_ROWS,
        );
        let tx = open_overlay(&ctx, MUTATION_ROWS, changes);
        for (name, query) in overlay_queries() {
            let db = WasmDbmsDatabase::from_transaction(&ctx, IndexPlanningSchema, tx);
            assert_eq!(
                sorted_values(
                    db.select_raw("shard_items", query.clone())
                        .expect("select failed")
                ),
                scan_oracle(&db, "shard_items", &query),
                "{name} with {changes} changes differs from the scan oracle"
            );
            let id = BenchmarkId::new(format!("{name}_changes_{changes}"), MUTATION_ROWS);
            group.bench_with_input(id, &query, |b, query| {
                b.iter(|| {
                    let db = WasmDbmsDatabase::from_transaction(&ctx, IndexPlanningSchema, tx);
                    black_box(
                        db.select_raw("shard_items", black_box(query.clone()))
                            .expect("select failed"),
                    )
                });
            });
        }
    }
    group.finish();
}

/// Builds a fixture once and returns a factory of independent copies.
fn snapshot_factory(fixture: Fixture) -> impl Fn() -> DbmsContext<SharedMemoryProvider> {
    let source = SharedMemoryProvider::default();
    drop(build_fixture(source.clone(), fixture, MUTATION_ROWS));
    let snapshot = source.snapshot();
    move || DbmsContext::new(SharedMemoryProvider::from_snapshot(snapshot.clone()))
}

fn bench_mutations(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_planning");
    configure(&mut group);

    let catalog = snapshot_factory(Fixture::CatalogUniform);
    assert_eq!(update_composite(&catalog()), 5);
    group.bench_function(BenchmarkId::new("update_composite", MUTATION_ROWS), |b| {
        b.iter_batched(
            &catalog,
            |ctx| black_box(update_composite(&ctx)),
            BatchSize::PerIteration,
        );
    });

    let shards = snapshot_factory(Fixture::ShardIndependent);
    assert_eq!(delete_union(&shards()), 199);
    group.bench_function(BenchmarkId::new("delete_union", MUTATION_ROWS), |b| {
        b.iter_batched(
            &shards,
            |ctx| black_box(delete_union(&ctx)),
            BatchSize::PerIteration,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_reads, bench_overlay, bench_mutations);
criterion_main!(benches);

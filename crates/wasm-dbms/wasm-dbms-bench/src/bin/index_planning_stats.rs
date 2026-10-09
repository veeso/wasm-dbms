//! Prints storage access counters for every index-planning read case.
//!
//! Run separately from the timed benchmarks so counting never distorts
//! latency: `just bench_index_planning_stats`.

use wasm_dbms::prelude::WasmDbmsDatabase;
use wasm_dbms_api::prelude::Database as _;
use wasm_dbms_bench::index_planning::{Fixture, IndexPlanningSchema, build_fixture, read_cases};
use wasm_dbms_bench::provider::HashMapMemoryProvider;

fn main() {
    let cases = read_cases();
    println!("case,rows,results,record_fetches,scanned_rows,index_entries");
    for rows in [10_000, 100_000] {
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
                let db = WasmDbmsDatabase::oneshot(&ctx, IndexPlanningSchema);
                ctx.reset_access_stats();
                let results = db
                    .select_raw(fixture.table(), case.query.clone())
                    .expect("select failed")
                    .len();
                let stats = ctx.access_stats();
                println!(
                    "{},{rows},{results},{},{},{}",
                    case.name, stats.record_fetches, stats.scanned_rows, stats.index_entries
                );
            }
        }
    }
}

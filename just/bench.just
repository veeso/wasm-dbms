[group('bench')]
bench bench_name="":
    echo "Running benchmark: {{ bench_name }}"
    cargo bench --bench "{{ bench_name }}"

[group('bench')]
bench_compare:
    echo "Running comparison benchmarks"
    cargo bench --package wasm-dbms-bench --bench compare

[group('bench')]
bench_index_planning:
    echo "Running index planning benchmarks"
    cargo bench --package wasm-dbms-bench --bench index_planning

[group('bench')]
bench_index_planning_stats:
    cargo run --release --package wasm-dbms-bench --features access-stats --bin index_planning_stats

[group('bench')]
bench_all:
    just bench_compare
    just bench_index_planning

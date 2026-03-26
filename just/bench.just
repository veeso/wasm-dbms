[group('bench')]
bench bench_name="":
    echo "Running benchmark: {{ bench_name }}"
    cargo bench --bench "{{ bench_name }}"

[group('bench')]
bench_comparison:
    echo "Running comparison benchmarks"
    cargo bench --package wasm-dbms-bench --bench compare

[group('bench')]
bench_all:
    just bench "read_table"
    just bench "eager_relation"
    just bench_comparison

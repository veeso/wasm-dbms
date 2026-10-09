# Run all unit tests and the WIT example
[group('test')]
test_all: test test_wasm_dbms_example test_wasm_dbms_key_value_example

[group('test')]
test test_name="":
  cargo test --lib {{test_name}} --workspace --exclude wasm-dbms-example-guest
  cargo test --doc --workspace --exclude wasm-dbms-example-guest

# Run only wasm-dbms crate tests (fast feedback loop for generic development)
[group('test')]
test_wasm_dbms test_name="":
  cargo test --lib {{test_name}} -p wasm-dbms-api -p wasm-dbms-memory -p wasm-dbms -p wasm-dbms-sql

# Generate an LCOV coverage report with the CI exclusions.
[group('test')]
coverage output="lcov.info" args="":
  cargo llvm-cov \
    --no-fail-fast \
    --workspace \
    --lib \
    --lcov \
    --output-path {{ output }} \
    --ignore-filename-regex '.*(macros|example).*' \
    {{ args }}

# Run the WIT Component Model example (end-to-end test)
[group('test')]
test_wasm_dbms_example: build_wasm_dbms_example
  echo "Running wasm-dbms WIT example..."
  cargo run --release --package "wasm-dbms-example-host" -- "{{WASM_DIR}}/wasm-dbms-example-guest.wasm"

# Run the Wasmtime key-value persistence and fault-injection tests.
[group('test')]
test_wasm_dbms_key_value_example: check_wasm_dbms_example_key_value_wit build_wasm_dbms_example_host
  cargo test --package "wasm-dbms-example-host" --bin "wasm-dbms-example" keyvalue_

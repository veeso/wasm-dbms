# Run all unit tests and integration tests
[group('test')]
test_all: test integration_test test_wasm_dbms_example

[group('test')]
test test_name="":
  cargo test --lib {{test_name}}
  cargo test --doc

# Run only wasm-dbms crate tests (fast feedback loop for generic development)
[group('test')]
test_wasm_dbms test_name="":
  cargo test --lib {{test_name}} -p wasm-dbms-api -p wasm-dbms-memory -p wasm-dbms

[group('test')]
integration_test test_name="":
  echo "Running integration tests..."
  cargo test {{test_name}} -p pocket-ic-tests

# Run the WIT Component Model example (end-to-end test)
[group('test')]
test_wasm_dbms_example: build_wasm_dbms_example
  echo "Running wasm-dbms WIT example..."
  cargo run --release --package "wasm-dbms-example-host" -- "{{WASM_DIR}}/wasm-dbms-example-guest.wasm"

# Cleans the build artifacts
[group('build')]
[confirm("Are you sure you want to clean the build artifacts?")]
clean:
  rm -rf {{WASM_DIR}}
  cargo clean

# Build every host-compatible workspace target in debug mode.
[group('build')]
build args="":
  cargo build --workspace --all-targets --exclude wasm-dbms-example-guest {{ args }}

# Build every host-compatible workspace target in release mode.
[group('build')]
release args="":
  cargo build --workspace --all-targets --release --exclude wasm-dbms-example-guest {{ args }}

[group('build')]
build_all: build_wasm_dbms build_wasi_dbms build_wasm_dbms_example

# Build generic wasm-dbms crates for wasm32-unknown-unknown
[group('build')]
build_wasm_dbms:
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-macros"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-api"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-memory"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-sql"

# Build generic wasi-dbms crates for wasm32-unknown-unknown
[group('build')]
build_wasi_dbms:
  cargo build --target wasm32-unknown-unknown --release --package "wasi-dbms-memory"

# Build the WIT Component Model example (guest + host)
[group('build')]
build_wasm_dbms_example: build_wasm_dbms_example_guest build_wasm_dbms_example_host

# Build the WIT guest component for wasm32-wasip2
[group('build')]
build_wasm_dbms_example_guest: pre_build
  cargo build --target wasm32-wasip2 --release --package "wasm-dbms-example-guest"
  cp "target/wasm32-wasip2/release/wasm_dbms_example_guest.wasm" "{{WASM_DIR}}/wasm-dbms-example-guest.wasm"

# Build the Wasmtime host binary (native)
[group('build')]
build_wasm_dbms_example_host:
  cargo build --release --package "wasm-dbms-example-host"

[private]
pre_build:
  mkdir -p "{{WASM_DIR}}"

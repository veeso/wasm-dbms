# Cleans the build artifacts
[group('build')]
[confirm("Are you sure you want to clean the build artifacts?")]
clean:
  rm -rf {{WASM_DIR}}
  cargo clean

[group('build')]
build_all: build_wasm_dbms build_library_wasm32 build_all_canisters

# Build generic wasm-dbms crates for wasm32-unknown-unknown
[group('build')]
build_wasm_dbms:
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-macros"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-api"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms-memory"
  cargo build --target wasm32-unknown-unknown --release --package "wasm-dbms"

# Build IC-specific library crates for wasm32-unknown-unknown
[group('build')]
build_library_wasm32:
  cargo build --target wasm32-unknown-unknown --release --package "ic-dbms-macros"
  cargo build --target wasm32-unknown-unknown --release --package "ic-dbms-api"
  cargo build --target wasm32-unknown-unknown --release --package "ic-dbms-canister"
  cargo build --target wasm32-unknown-unknown --release --package "ic-dbms-client"

# Build all
[group('build')]
build_all_canisters: pre_build build_example_canister build_dbms_canister_client_integration_canister

[group('build')]
build_example_canister:
  just build_canister "example" "example" "example"

[group('build')]
build_dbms_canister_client_integration_canister:
  just build_canister "dbms-canister-client-integration" "dbms_canister_client_integration" "dbms_canister_client_integration"

[private]
pre_build:
  mkdir -p "{{WASM_DIR}}"

[private]
build_canister package_name canister_name wasm_name:
  mkdir -p "{{WASM_DIR}}"
  echo "Building {{canister_name}} Canister"
  cargo build --target wasm32-unknown-unknown --release --package "{{package_name}}"
  ic-wasm "target/wasm32-unknown-unknown/release/{{canister_name}}.wasm" -o "{{WASM_DIR}}/{{wasm_name}}.wasm" shrink
  candid-extractor "{{WASM_DIR}}/{{wasm_name}}.wasm" > "{{WASM_DIR}}/{{wasm_name}}.did"
  gzip -k "{{WASM_DIR}}/{{wasm_name}}.wasm" --force

# Publish all crates in dependency order
[group('publish')]
publish_all: publish_wasm_dbms_all publish_wasi_dbms_all publish_ic_dbms_all

# Publish all wasm-dbms crates in dependency order
[group('publish')]
publish_wasm_dbms_all: publish_wasm_dbms_macros publish_wasm_dbms_api publish_wasm_dbms_memory publish_wasm_dbms

[group('publish')]
publish_wasm_dbms_macros:
  cargo publish -p wasm-dbms-macros

[group('publish')]
publish_wasm_dbms_api:
  cargo publish -p wasm-dbms-api

[group('publish')]
publish_wasm_dbms_memory:
  cargo publish -p wasm-dbms-memory

[group('publish')]
publish_wasm_dbms:
  cargo publish -p wasm-dbms

# Publish all WASI-specific crates in dependency order
[group('publish')]
publish_wasi_dbms_all: publish_wasi_dbms_memory

[group('publish')]
publish_wasi_dbms_memory:
  cargo publish -p wasi-dbms-memory

# Publish all IC-specific crates in dependency order
[group('publish')]
publish_ic_dbms_all: publish_ic_dbms_macros publish_ic_dbms_api publish_ic_dbms_canister publish_ic_dbms_client

[group('publish')]
publish_ic_dbms_macros:
  cargo publish -p ic-dbms-macros

[group('publish')]
publish_ic_dbms_api:
  cargo publish -p ic-dbms-api

[group('publish')]
publish_ic_dbms_canister:
  cargo publish -p ic-dbms-canister

[group('publish')]
publish_ic_dbms_client:
  cargo publish -p ic-dbms-client

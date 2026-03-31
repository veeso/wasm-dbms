# Publish all crates in dependency order
[group('publish')]
publish_all: publish_wasm_dbms_all publish_wasi_dbms_all publish_ic_dbms_all

# Publish all wasm-dbms crates in dependency order
[group('publish')]
publish_wasm_dbms_all: publish_wasm_dbms_macros publish_wasm_dbms_api publish_wasm_dbms_memory publish_wasm_dbms

[group('publish')]
publish_wasm_dbms_macros: (_publish "wasm-dbms-macros")

[group('publish')]
publish_wasm_dbms_api: (_publish "wasm-dbms-api")

[group('publish')]
publish_wasm_dbms_memory: (_publish "wasm-dbms-memory")

[group('publish')]
publish_wasm_dbms: (_publish "wasm-dbms")

# Publish all WASI-specific crates in dependency order
[group('publish')]
publish_wasi_dbms_all: publish_wasi_dbms_memory

[group('publish')]
publish_wasi_dbms_memory: (_publish "wasi-dbms-memory")

# Publish all IC-specific crates in dependency order
[group('publish')]
publish_ic_dbms_all: publish_ic_dbms_macros publish_ic_dbms_api publish_ic_dbms_canister publish_ic_dbms_client

[group('publish')]
publish_ic_dbms_macros: (_publish "ic-dbms-macros")

[group('publish')]
publish_ic_dbms_api: (_publish "ic-dbms-api")

[group('publish')]
publish_ic_dbms_canister: (_publish "ic-dbms-canister")

[group('publish')]
publish_ic_dbms_client: (_publish "ic-dbms-client")

# Publish a crate with retry logic to handle registry lag
[private]
_publish crate:
  #!/usr/bin/env bash
  set -euo pipefail
  max_retries=5
  delay=15
  for attempt in $(seq 1 $max_retries); do
    echo "Publishing {{crate}} (attempt $attempt/$max_retries)..."
    if cargo publish -p {{crate}}; then
      echo "Successfully published {{crate}}"
      exit 0
    fi
    if [ "$attempt" -lt "$max_retries" ]; then
      echo "Publish failed, retrying in ${delay}s..."
      sleep "$delay"
    fi
  done
  echo "Failed to publish {{crate}} after $max_retries attempts"
  exit 1
